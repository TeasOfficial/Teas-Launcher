//! 游戏启动模块 v2 — 参照 HMCL DefaultLauncher 架构完全重构
//!
//! 核心设计原则（经 HMCL 源码验证）:
//! 1. **信任版本 JSON**: mainClass / arguments.jvm / arguments.game / -p module path /
//!    --launchTarget / -DignoreList 等全部由原版或 Forge/NeoForge/Fabric 安装器生成，
//!    启动器只负责: 解析 → rules 过滤 → 占位符替换 → 参数去重 → 组装。
//!    由此天然覆盖三代 Forge 差异:
//!     - ≤1.12.2:  mainClass = net.minecraft.launchwrapper.Launch + --tweakClass
//!     - 1.13~1.16.5: mainClass = cpw.mods.modlauncher.Launcher
//!     - 1.17+:     mainClass = cpw.mods.bootstraplauncher.BootstrapLauncher (-p module path)
//! 2. **占位符替换**: 一次扫描替换 ${...}，未命中的占位符保留原文 (HMCL StringArgument 语义)
//! 3. **跨段参数去重**: 版本 JSON + 用户自定义 + 启动器生成 三源合并后统一去重，
//!    受保护 key (-cp/-classpath/-p/--module-path/-Djava.library.path) 保留 JSON 版
//! 4. **natives 子目录重定向**: Forge 1.17+ 的 -Djava.library.path=${natives_directory}/java
//!    会把解压目录重定向到 natives-<id>/java (HMCL DefaultLauncher 同款逻辑)
//! 5. **jar 字段重定向**: 1.13- 版本 "jar": "1.12.2" 之类字段决定主 JAR 位置
//!
//! 流程 (launch_instance):
//!   reset_cancel → pre_check → JAR 扫描 → ensure_parent_version (含 jar 重定向)
//!   → 补全 libraries + 主 JAR → assets → resolve_version (合并 inheritsFrom)
//!   → prepare_natives (子目录重定向) → build_flat_args (Vec<String>)
//!   → pre_run_setup (log4j2.xml + options.txt) → spawn_process → monitor_process
//!
//! 子模块划分:
//! - natives: Natives 解压、子目录重定向、孤儿文件清理
//! - args:    启动参数构建（纯函数，可单测）
//! - process: 预启动处理、进程启动与监控
//! - java:    Java 版本探测与工具函数

mod args;
mod java;
mod natives;
mod process;

#[cfg(test)]
mod tests;

use crate::config::teas_dir;
use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_launcher_or_meta;
use crate::instance::resolve_version;
use crate::utils::{offline_uuid, scan_and_fix_jars_with_strategy, JarScanStrategy};
use crate::version::assets::{mcassets_fix_list, mcassets_get_index_name, download_asset_index};
use crate::version::library::mclib_from_instance;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

use args::{build_flat_args, primary_jar_path};
use java::check_java_compatibility;
use natives::prepare_natives;
use process::{monitor_process, pre_run_setup, spawn_process};

/// 嵌入 PCL LUA.jar (LWJGL 3.4.1 兼容层)
const LUA_JAR_BYTES: &[u8] = include_bytes!("../../LUA.jar");

// ── 进程管理 ──────────────────────────────────────────────

static RUNNING_PID: Mutex<Option<u32>> = Mutex::new(None);

/// 本次运行是否由用户主动终止
///
/// `kill_instance` 走 taskkill /F，进程会以非零码退出，与真正的崩溃无法从
/// 退出码区分。用这个标志把"用户点了停止"和"游戏崩了"分开，避免每次主动
/// 退出都误报崩溃。launch_instance 开始时清零。
pub(crate) static USER_KILLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 上一次进程退出是否由用户主动终止（供 crash.rs 判定是否跳过崩溃检测）
pub(crate) fn last_exit_was_user_kill() -> bool {
    USER_KILLED.load(std::sync::atomic::Ordering::SeqCst)
}

/// 终止正在运行的 Minecraft 进程（带 /T 一次即可杀进程树，避免 PID 重用误杀）
#[tauri::command]
pub(crate) fn kill_instance() -> Result<(), String> {
    let mut p = RUNNING_PID.lock().unwrap();
    if let Some(pid) = *p {
        USER_KILLED.store(true, std::sync::atomic::Ordering::SeqCst);
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .spawn();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .spawn();
        }
        *p = None;
        log::info!("[launch] 用户请求终止进程 {}", pid);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn is_instance_running() -> bool {
    RUNNING_PID.lock().unwrap().is_some()
}

// ── 账户信息 ──────────────────────────────────────────────
// 目前仅离线模式；微软登录接入后替换 access_token/uuid/user_type 即可

#[derive(Debug, Clone)]
pub(crate) struct AuthInfo {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub user_properties: String,
    pub xuid: String,
    pub client_id: String,
}

impl AuthInfo {
    pub(crate) fn offline(player_name: &str) -> Self {
        Self {
            player_name: player_name.to_string(),
            uuid: offline_uuid(player_name),
            access_token: "0".to_string(),
            user_type: "mojang".to_string(),
            user_properties: "{}".to_string(),
            xuid: "0".to_string(),
            client_id: "0".to_string(),
        }
    }
}

// ── 启动上下文 ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum GcMode {
    /// 0: 自动 — 默认 G1GC（ZGC 在新版 LWJGL + Java 25 上有兼容性问题）
    Auto,
    /// 1: G1GC only
    G1GC,
    /// 2: 用户自定义（不干预 GC 参数）
    Custom,
}

#[derive(Debug, Clone)]
struct LaunchContext {
    dot_minecraft: PathBuf,
    instance_name: String,
    auth: AuthInfo,
    java_path: PathBuf,
    max_memory: u32,     // MB
    jvm_args: String,    // 用户自定义 JVM 参数
    window_width: u32,
    window_height: u32,
    prefer_official: bool,
    gc_mode: GcMode,
    /// 版本隔离: 游戏目录 = versions/<id>，否则 = .minecraft 根
    version_isolation: bool,
}

impl LaunchContext {
    fn parse(
        mc_dir: &str,
        instance_name: &str,
        player_name: &str,
        java_path: &str,
        max_memory: &str,
        jvm_args: &str,
        window_width: u32,
        window_height: u32,
    ) -> Self {
        let base = Path::new(mc_dir);
        let dot_minecraft = if base.join(".minecraft").exists() {
            base.join(".minecraft")
        } else {
            base.to_path_buf()
        };

        let cfg = crate::config::config_read("user".to_string())
            .unwrap_or(serde_json::json!({}));

        // 前端使用 game_source key，值为 "官方源" 或 "BMCLAPI"
        let prefer_official = cfg
            .get("game_source")
            .and_then(|v| v.as_str())
            .map(|s| s != "BMCLAPI")
            .unwrap_or(true);

        let gc_mode = match cfg.get("gc_mode").and_then(|v| v.as_str()).unwrap_or("auto") {
            "g1gc" => GcMode::G1GC,
            "custom" => GcMode::Custom,
            _ => GcMode::Auto,
        };

        // 版本隔离（默认开启）
        let version_isolation = cfg
            .get("version_isolation")
            .and_then(|v| v.as_str())
            .map(|s| s != "否")
            .unwrap_or(true);

        LaunchContext {
            dot_minecraft,
            instance_name: instance_name.to_string(),
            auth: AuthInfo::offline(player_name),
            java_path: PathBuf::from(java_path),
            max_memory: parse_memory_mb(max_memory),
            jvm_args: jvm_args.to_string(),
            window_width,
            window_height,
            prefer_official,
            gc_mode,
            version_isolation,
        }
    }

    /// 游戏运行目录（saves/mods/options.txt 所在处）
    fn game_directory(&self) -> PathBuf {
        if self.version_isolation {
            self.dot_minecraft
                .join("versions")
                .join(&self.instance_name)
        } else {
            self.dot_minecraft.clone()
        }
    }
}

/// 解析内存设置字符串 → MB
///
/// 兼容格式: "8192"、"8192 MB"、"8G"、"8GB"、"1.5G"、"4 GB"（前端滑块保存为 "N MB"）
fn parse_memory_mb(s: &str) -> u32 {
    let up = s.trim().to_uppercase();
    let parse_num = |t: &str| -> Option<f64> {
        t.trim().parse::<f64>().ok()
    };
    if let Some(rest) = up.strip_suffix("GB") {
        return parse_num(rest).map(|v| (v * 1024.0) as u32).unwrap_or(2048);
    }
    if let Some(rest) = up.strip_suffix("MB") {
        return parse_num(rest).map(|v| v as u32).unwrap_or(2048);
    }
    if let Some(rest) = up.strip_suffix('G') {
        return parse_num(rest).map(|v| (v * 1024.0) as u32).unwrap_or(2048);
    }
    if let Some(rest) = up.strip_suffix('M') {
        return parse_num(rest).map(|v| v as u32).unwrap_or(2048);
    }
    parse_num(&up).map(|v| v as u32).unwrap_or(2048)
}

// ── 主启动入口 ──────────────────────────────────────────────

/// 启动 Minecraft 实例（完整流程）
#[tauri::command]
pub(crate) async fn launch_instance(
    app: tauri::AppHandle,
    mc_dir: String,
    instance_name: String,
    player_name: String,
    java_path: String,
    max_memory: String,
    jvm_args: String,
    window_width: u32,
    window_height: u32,
) -> Result<(), String> {
    let ctx = LaunchContext::parse(
        &mc_dir, &instance_name, &player_name, &java_path,
        &max_memory, &jvm_args, window_width, window_height,
    );

    // ★ 修复取消粘滞: 之前用户取消过下载会导致 CANCEL_DOWNLOAD 永久为 true
    crate::http::reset_cancel();

    // ★ 新的一次启动: 清掉上一次的"用户主动终止"标记
    USER_KILLED.store(false, std::sync::atomic::Ordering::SeqCst);

    // 把本次启动的关键上下文写进日志——没有这段，事后排查时只能看到一堆游戏输出
    log::info!(
        "[launch] 启动请求: 实例=\"{}\" 玩家={} 内存={}MB Java={}",
        ctx.instance_name,
        ctx.auth.player_name,
        ctx.max_memory,
        ctx.java_path.display()
    );
    log::info!(
        "[launch] .minecraft={} 游戏目录={} 版本隔离={} 优先官方源={}",
        ctx.dot_minecraft.display(),
        ctx.game_directory().display(),
        ctx.version_isolation,
        ctx.prefer_official
    );

    // ── 1. 预检测 ────────────────────────────────
    pre_check(&ctx)?;

    // ── 2. JAR 扫描（修复损坏文件）──────────────
    let scan_strategy = {
        let cfg = crate::config::config_read("user".to_string())
            .unwrap_or(serde_json::json!({}));
        JarScanStrategy::from_config(
            cfg.get("jar_scan_strategy").and_then(|v| v.as_str()).unwrap_or("B")
        )
    };
    let ver_jar = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.jar", ctx.instance_name));
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let deleted = scan_and_fix_jars_with_strategy(scan_strategy, &ver_jar, &libs_dir);
    if deleted > 0 {
        log::info!("[launch] 已删除 {} 个损坏文件", deleted);
    }

    // ── 3. 确保父版本 JSON + JAR（含 jar 字段重定向）──
    ensure_parent_version(&app, &ctx).await?;

    // ── 4. 补全 Libraries + 主 JAR ───────────────
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;

    // ★ Java 版本校验：放在下载之前，避免用户等完一堆下载才在 spawn 时才失败
    check_java_compatibility(&ctx.java_path, &version)?;

    let mut lib_files = mclib_from_instance(&version, &ctx.dot_minecraft, ctx.prefer_official);
    if !lib_files.is_empty() {
        crate::download::engine::download_files_parallel(&app, &mut lib_files, 8).await;
    }

    // ★ 主 JAR 存在性检查（jar 字段 / inheritsFrom 重定向后）
    let primary = primary_jar_path(&ctx, &version);
    if !primary.exists() {
        return Err(format!(
            "主游戏 JAR 缺失: {}\n请先在下载中心安装该版本，或点击「修复实例」。",
            primary.display()
        ));
    }

    // ── 5. 下载 Asset Index + 补全 Assets ──────────
    if let Some(asset_index_dl) = download_asset_index(
        &version, &ctx.dot_minecraft, ctx.prefer_official
    ) {
        let mut dl = asset_index_dl;
        let _ = download_file(&app, &mut dl, false).await;
    }
    let index_name = mcassets_get_index_name(&version);
    if let Ok(assets) = mcassets_fix_list(
        &ctx.dot_minecraft, &index_name, true, ctx.prefer_official
    ) {
        if !assets.is_empty() {
            let mut files: Vec<DownloadFile> = assets;
            crate::download::engine::download_files_parallel(&app, &mut files, 8).await;
        }
    }

    // ── 6. 重新解析版本（父 JSON 可能刚下载）──
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;

    // ── 7. 解压 Natives（含子目录重定向）────────
    let natives = {
        let ctx_clone = ctx.clone();
        let version_clone = version.clone();
        tokio::task::spawn_blocking(move || prepare_natives(&ctx_clone, &version_clone))
            .await
            .map_err(|e| format!("Natives 解压线程异常: {}", e))??
    };

    // ── 8. 构建启动参数 (Vec<String>) ────────────
    let flat_args = build_flat_args(&ctx, &version, &natives.base_dir)?;

    // ★ 调试: 导出 .bat 文件（与 PCL 的 LatestLaunch.bat 对比）
    let bat_path = ctx.dot_minecraft.join("teas-latest-launch.bat");
    let bat_content = format!(
        "@echo off\r\n\
         title TeasLauncher - {}\r\n\
         cd /D \"{}\"\r\n\
         {}\r\n\
         echo Game exited.\r\n\
         pause\r\n",
        ctx.instance_name,
        ctx.game_directory().display(),
        flat_args.iter()
            .map(|a| {
                if a.contains(' ') || a.contains('&') || a.contains('|') || a.contains('<') || a.contains('>') {
                    format!("\"{}\"", a)
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    );
    let _ = std::fs::write(&bat_path, &bat_content);

    // 打印命令行（调试用，截断超长参数）
    log::debug!("[launch] =================== 启动命令行 ===================");
    for (i, a) in flat_args.iter().enumerate() {
        if a.len() > 350 {
            log::debug!("[launch]   [{}] {}...", i, &a[..350]);
        } else {
            log::debug!("[launch]   [{}] {}", i, a);
        }
    }
    log::debug!("[launch] ===================================================");

    // ── 9. 预启动处理（log4j2.xml + options.txt）──
    pre_run_setup(&ctx, &version)?;

    // ── 10. 启动进程 ─────────────────────────────
    let child = spawn_process(&ctx, &flat_args, &version)?;
    let pid = child.id();
    *RUNNING_PID.lock().unwrap() = Some(pid);

    let _ = app.emit("launch-started", serde_json::json!({
        "pid": pid,
        "instance": ctx.instance_name,
    }));

    // ── 11. 监控进程 + 崩溃检测 ──────────────────
    monitor_process(app, child, ctx, mc_dir);

    Ok(())
}

/// 调试用: 获取启动参数预览（兼容旧字段 + 新增完整 args 列表）
#[tauri::command]
pub(crate) fn get_launch_args(
    mc_dir: String,
    instance_name: String,
    player_name: String,
    java_path: String,
    max_memory: String,
    jvm_args: String,
    window_width: u32,
    window_height: u32,
) -> Result<serde_json::Value, String> {
    let ctx = LaunchContext::parse(
        &mc_dir, &instance_name, &player_name, &java_path,
        &max_memory, &jvm_args, window_width, window_height,
    );
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;
    let natives_base = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}-natives", ctx.instance_name));
    let flat_args = build_flat_args(&ctx, &version, &natives_base)?;

    Ok(serde_json::json!({
        "java": flat_args.first().cloned().unwrap_or_default(),
        "args": flat_args.iter().skip(1).cloned().collect::<Vec<_>>(),
        "jvm": ctx.jvm_args,
        "mem": ctx.max_memory,
        "gameDir": ctx.game_directory().display().to_string(),
        "width": ctx.window_width,
        "height": ctx.window_height,
        "player": ctx.auth.player_name,
        "uuid": ctx.auth.uuid,
    }))
}

// ── 预检测 ─────────────────────────────────────────────────

fn pre_check(ctx: &LaunchContext) -> Result<(), String> {
    // 路径非法字符检查（! 和 ; 会导致命令行解析错误）
    let path_str = ctx.dot_minecraft.display().to_string();
    if path_str.contains('!') || path_str.contains(';') {
        return Err(format!(
            "游戏路径含有非法字符 (! 或 ;): {}", path_str
        ));
    }

    // 检查版本 JSON
    let json_path = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.json", ctx.instance_name));
    if !json_path.exists() {
        return Err(format!(
            "版本 JSON 不存在: {}.json\n请先在下载中心安装该版本。", ctx.instance_name
        ));
    }

    // 检查 Java
    if !ctx.java_path.exists() {
        return Err(format!(
            "Java 路径无效: {}\n请在设置中选择正确的 Java。", ctx.java_path.display()
        ));
    }

    // 非 ASCII 玩家名警告
    if !ctx.auth.player_name.chars().all(|c| c.is_ascii()) {
        log::warn!("[launch] 警告: 玩家名包含非 ASCII 字符，旧版本可能崩溃");
    }

    Ok(())
}

// ── 父版本 JSON/JAR 确保（含 jar 字段重定向）────────────────

async fn ensure_parent_version(
    app: &tauri::AppHandle,
    ctx: &LaunchContext,
) -> Result<(), String> {
    let json_path = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.json", ctx.instance_name));

    let content = std::fs::read_to_string(&json_path)
        .map_err(|e| format!("无法读取版本 JSON: {}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("版本 JSON 解析失败: {}", e))?;

    // 收集需要确保的父版本: inheritsFrom + jar 字段（去重、排除自身）
    let mut parents: Vec<String> = Vec::new();
    if let Some(p) = v["inheritsFrom"].as_str() {
        if !p.is_empty() && p != ctx.instance_name {
            parents.push(p.to_string());
        }
    }
    if let Some(j) = v["jar"].as_str() {
        if !j.is_empty() && j != ctx.instance_name && !parents.iter().any(|x| x == j) {
            parents.push(j.to_string());
        }
    }

    for parent in parents {
        ensure_vanilla_version(app, ctx, &parent).await?;
    }

    Ok(())
}

/// 确保某个原版版本（父版本）的 JSON + JAR 存在于 versions/<name>/
/// 缓存优先（.teas/vanilla/），避免重复下载
async fn ensure_vanilla_version(
    app: &tauri::AppHandle,
    ctx: &LaunchContext,
    parent_name: &str,
) -> Result<(), String> {
    let teas = teas_dir()?;
    let cache_dir = teas.join("vanilla");
    std::fs::create_dir_all(&cache_dir).ok();

    let cache_json = cache_dir.join(format!("{}.json", parent_name));
    let parent_json = ctx.dot_minecraft
        .join("versions").join(parent_name)
        .join(format!("{}.json", parent_name));

    // JSON: 缓存优先，其次本地，最后从版本清单下载
    if !cache_json.exists() && !parent_json.exists() {
        log::info!("[launch] 下载父版本 JSON: {}", parent_name);
        let client = crate::http::build_http_client(
            std::time::Duration::from_secs(30)
        )?;
        let manifest: serde_json::Value = client
            .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
            .send().await
            .map_err(|e| format!("获取版本清单失败: {}", e))?
            .json().await
            .map_err(|e| format!("解析版本清单失败: {}", e))?;

        if let Some(ver_url) = manifest["versions"].as_array()
            .and_then(|a| a.iter().find(|pv| pv["id"].as_str() == Some(parent_name)))
            .and_then(|pv| pv["url"].as_str())
        {
            let urls = source_launcher_or_meta(ver_url, ctx.prefer_official);
            let mut dl = DownloadFile::new(
                urls, cache_json.clone(),
                FileChecker { min_size: 500, is_json: true, ..Default::default() }
            );
            let _ = download_file(app, &mut dl, false).await;
        }
    }

    if !parent_json.exists() && cache_json.exists() {
        if let Some(p) = parent_json.parent() {
            std::fs::create_dir_all(p).ok();
        }
        let _ = std::fs::copy(&cache_json, &parent_json);
    }

    // 主 JAR: 缓存优先，其次本地，最后下载
    let cache_jar = cache_dir.join(format!("{}.jar", parent_name));
    let parent_jar = ctx.dot_minecraft
        .join("versions").join(parent_name)
        .join(format!("{}.jar", parent_name));

    if !cache_jar.exists() && !parent_jar.exists() {
        let json_to_read = if cache_json.exists() { &cache_json } else { &parent_json };
        if let Ok(pj) = std::fs::read_to_string(json_to_read) {
            if let Ok(pv) = serde_json::from_str::<serde_json::Value>(&pj) {
                if let Some(jar_url) = pv["downloads"]["client"]["url"].as_str() {
                    let urls = source_launcher_or_meta(jar_url, ctx.prefer_official);
                    let mut dl = DownloadFile::new(
                        urls, cache_jar.clone(),
                        FileChecker::with_min_size(1024)
                    );
                    let _ = download_file(app, &mut dl, false).await;
                }
            }
        }
    }

    if !parent_jar.exists() && cache_jar.exists() {
        if let Some(p) = parent_jar.parent() {
            std::fs::create_dir_all(p).ok();
        }
        let _ = std::fs::copy(&cache_jar, &parent_jar);
    }

    Ok(())
}

