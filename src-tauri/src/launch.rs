//! 游戏启动模块 — 完全参照 HMCL/PCL/PCL-CE/PrismLauncher 重写
//!
//! 架构 (HMCL DefaultLauncher + PCL McLaunch 风格):
//! 1. LaunchContext      — 结构化启动配置
//! 2. pre_launch          — 预检 + 补全文件 + 解压 natives
//! 3. build_flat_args     — 构建 Vec<String> 参数列表 (不做字符串拼接→再分割)
//! 4. spawn_process       — ProcessBuilder 直接使用 Vec<String>
//! 5. monitor_process     — stdout/stderr pump + ExitWaiter (HMCL-style)
//!
//! 关键改进 (vs 旧版):
//! - 参数全程保持 Vec<String>，不再拼接后重新分割
//! - Classpath: OptiFine 到倒数第二位 (PCL-style: insert at len-2)
//! - Natives: PCL-style 非ASCII路径回退 (%APPDATA%\.minecraft\bin\natives)
//! - JVM参数: HMCL-style 优先级 (用户覆盖→内存→生成→版本JSON)
//! - Java版本检测: 回退到 -version 输出解析 (比 -XshowSettings:all 更可靠)
//! - 参数去重: PCL-style (JVM去重同key, 游戏参数覆盖同key)
//! - 正确传递 APPDATA + Path + INST_* 环境变量
//! - 支持 Old Minecraft arguments (minecraftArguments 字段，旧版格式)

use crate::config::teas_dir;
use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_launcher_or_meta;
use crate::instance::resolve_version;
use crate::utils::{offline_uuid, scan_and_fix_jars_with_strategy, JarScanStrategy};
use crate::version::library::mclib_from_instance;
use crate::version::assets::{mcassets_fix_list, mcassets_get_index_name, download_asset_index};
use crate::BUILD;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

/// 嵌入 PCL LUA.jar (LWJGL 3.4.1 兼容层)
const LUA_JAR_BYTES: &[u8] = include_bytes!("../LUA.jar");

// ── 进程管理 ──────────────────────────────────────────────

static RUNNING_PID: Mutex<Option<u32>> = Mutex::new(None);
static CANCEL_FLAG: Mutex<bool> = Mutex::new(false);

/// 终止正在运行的 Minecraft 进程 (PCL-style: /F 再 /T 杀子进程树)
#[tauri::command]
pub(crate) fn kill_instance() -> Result<(), String> {
    let mut p = RUNNING_PID.lock().unwrap();
    if let Some(pid) = *p {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .spawn();
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
    }
    *CANCEL_FLAG.lock().unwrap() = true;
    Ok(())
}

#[tauri::command]
pub(crate) fn is_instance_running() -> bool {
    RUNNING_PID.lock().unwrap().is_some()
}

// ── 启动上下文 ────────────────────────────────────────────

/// 结构化启动参数
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LaunchContext {
    mc_dir: PathBuf,
    dot_minecraft: PathBuf,
    instance_name: String,
    player_name: String,
    java_path: PathBuf,
    max_memory: u32,        // MB
    min_memory: Option<u32>,
    metaspace: Option<u32>, // MB, 默认 256
    jvm_args: String,       // 用户自定义 JVM 参数 (空格分隔)
    game_args: String,      // 用户自定义游戏参数
    window_width: u32,
    window_height: u32,
    java_agents: Vec<String>,
    env_vars: Vec<(String, String)>,
    prefer_official: bool,
    // 从设置读取
    gc_mode: GcMode,
    process_priority: ProcessPriority,
    // 游戏目录是否是独立实例目录（版本隔离）
    is_version_isolated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GcMode {
    /// 0: 自动 — Java 21+ ZGC, 15-20 ZGC, 8-14 G1GC
    Auto,
    /// 1: G1GC only
    G1GC,
    /// 2: 用户自定义
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ProcessPriority {
    Normal,
    High,
    Low,
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

        let max_mem = parse_memory_mb(max_memory);

        let cfg = crate::config::config_read("user".to_string())
            .unwrap_or(serde_json::json!({}));

        // 前端使用 game_source key，值为 "官方源" 或 "BMCLAPI"
        // 默认使用官方源
        let prefer_official = cfg
            .get("game_source")
            .and_then(|v| v.as_str())
            .map(|s| s != "BMCLAPI") // 非 BMCLAPI 即为官方源
            .unwrap_or(true);

        let gc_mode = match cfg.get("gc_mode").and_then(|v| v.as_str()).unwrap_or("auto") {
            "g1gc" => GcMode::G1GC,
            "custom" => GcMode::Custom,
            _ => GcMode::Auto,
        };

        let process_priority = match cfg.get("process_priority").and_then(|v| v.as_str()).unwrap_or("normal") {
            "high" => ProcessPriority::High,
            "low" => ProcessPriority::Low,
            _ => ProcessPriority::Normal,
        };

        // 检查是否版本隔离（实例目录独立于 .minecraft/versions/）
        let version_dir = dot_minecraft.join("versions").join(instance_name);
        let is_version_isolated = base != dot_minecraft && !version_dir.exists();

        LaunchContext {
            mc_dir: base.to_path_buf(),
            dot_minecraft,
            instance_name: instance_name.to_string(),
            player_name: player_name.to_string(),
            java_path: PathBuf::from(java_path),
            max_memory: max_mem,
            min_memory: None,
            metaspace: Some(256),
            jvm_args: jvm_args.to_string(),
            game_args: String::new(),
            window_width,
            window_height,
            java_agents: Vec::new(),
            env_vars: Vec::new(),
            prefer_official,
            gc_mode,
            process_priority,
            is_version_isolated,
        }
    }
}

fn parse_memory_mb(s: &str) -> u32 {
    let s = s.trim().to_uppercase().replace("MB", "M");
    if s.ends_with('G') {
        s.trim_end_matches('G').parse::<f64>().map(|g| (g * 1024.0) as u32).unwrap_or(2048)
    } else if s.ends_with('M') {
        s.trim_end_matches('M').parse::<u32>().unwrap_or(2048)
    } else {
        s.parse::<u32>().unwrap_or(2048)
    }
}

/// 调试用：获取启动参数预览
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
    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    Ok(serde_json::json!({
        "java": ctx.java_path.display().to_string(),
        "jvm": ctx.jvm_args,
        "mem": ctx.max_memory,
        "gameDir": dot_minecraft.display().to_string(),
        "width": ctx.window_width,
        "height": ctx.window_height,
        "player": ctx.player_name,
        "uuid": offline_uuid(&ctx.player_name)
    }))
}

// ── 主启动入口 ──────────────────────────────────────────────

/// 启动 Minecraft 实例（完整流程）
///
/// 流程 (PCL McLaunchStart 风格):
/// 1. 预检测 (路径/版本/Java)
/// 2. JAR 扫描 (修复损坏文件)
/// 3. 确保父版本 JSON + JAR
/// 4. 补全 Libraries
/// 5. 下载 Asset Index + 补全 Assets
/// 6. 重新解析版本 (合并 inheritsFrom)
/// 7. 解压 Natives (PCL-style)
/// 8. 构建启动参数 → Vec<String> (HMCL-style, 不做拼接→再分割)
/// 9. 预启动处理 (log4j2.xml, options.txt)
/// 10. 启动进程
/// 11. 监控进程 + 崩溃检测
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

    // 清除取消标志
    *CANCEL_FLAG.lock().unwrap() = false;

    // ── 1. 预检测 ────────────────────────────────
    pre_check(&ctx)?;

    // ── 2. JAR 扫描 ──────────────────────────────
    let scan_strategy = {
        let cfg = crate::config::config_read("user".to_string())
            .unwrap_or(serde_json::json!({}));
        JarScanStrategy::from_config(
            cfg.get("jar_scan_strategy").and_then(|v| v.as_str()).unwrap_or("B")
        )
    };
    eprintln!("[launch] JAR 扫描策略: {:?}", scan_strategy);
    let ver_jar = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.jar", ctx.instance_name));
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let deleted = scan_and_fix_jars_with_strategy(scan_strategy, &ver_jar, &libs_dir);
    if deleted > 0 {
        eprintln!("[launch] 已删除 {} 个损坏文件", deleted);
    }

    // ── 3. 确保父版本 JSON + JAR ────────────────
    ensure_parent_version(&app, &ctx).await?;

    // ── 4. 补全 Libraries ───────────────────────
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;
    let lib_files = mclib_from_instance(&version, &ctx.dot_minecraft, ctx.prefer_official);
    if !lib_files.is_empty() {
        let mut files: Vec<DownloadFile> = lib_files;
        crate::download::engine::download_files_parallel(&app, &mut files, 8).await;
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

    // ── 6. 重新解析版本 (父 JSON 可能刚下载) ────
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;

    // ── 7. 解压 Natives (PCL-style) ──────────────
    let natives_dir = prepare_natives(&ctx, &version)?;

    // ── 8. 构建启动参数 (返回 Vec<String>) ────────
    let flat_args = build_flat_args(&ctx, &version, &natives_dir)?;

    // ★ 调试: 导出 .bat 文件 (与 PCL 的 LatestLaunch.bat 对比)
    let bat_path = ctx.dot_minecraft.join("teas-latest-launch.bat");
    let bat_content = format!(
        "@echo off\r\n\
         title TeasLauncher - {}\r\n\
         cd /D \"{}\"\r\n\
         {}\r\n\
         echo Game exited.\r\n\
         pause\r\n",
        ctx.instance_name,
        ctx.dot_minecraft.join("versions").join(&ctx.instance_name).display(),
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
    eprintln!("[launch] 已导出启动脚本: {}", bat_path.display());

    // 打印命令行 (调试用)
    // 调试: 输出 classpath 信息
    if let Some(cp_pos) = flat_args.iter().position(|a| a == "-cp" || a == "-classpath") {
        if cp_pos + 1 < flat_args.len() {
            let cp = &flat_args[cp_pos + 1];
            let jar_count = cp.split(';').count();
            eprintln!("[launch] Classpath: {} JARs ({} bytes total)", jar_count, cp.len());
            // 输出前 3 个和后 3 个 JAR
            let jars: Vec<&str> = cp.split(';').collect();
            for j in jars.iter().take(3) {
                eprintln!("[launch]   cp[first]: {}", j);
            }
            if jars.len() > 6 {
                eprintln!("[launch]   ... {} more ...", jars.len() - 6);
            }
            {
                let mut rev = jars.iter().rev().take(3).collect::<Vec<_>>();
                rev.reverse();
                for j in rev {
                    eprintln!("[launch]   cp[last]: {}", j);
                }
            }
            // ★ 调试: 检查关键 LWJGL JAR 是否在 classpath 中
            let critical_jars = [
                ("lwjgl-glfw-3", "GLFWErrorCallbackI"),
                ("lwjgl-opengl-3", "OpenGL"),
                ("lwjgl-stb-3", "STB"),
                ("lwjgl-jemalloc-3", "jemalloc"),
                ("lwjgl-openal-3", "OpenAL"),
            ];
            for (prefix, desc) in &critical_jars {
                let found: Vec<&&str> = jars.iter().filter(|j| j.contains(prefix)).collect();
                eprintln!("[launch]   cp[{}]: {} JAR(s) found: {:?}", desc, found.len(), found);
                // 检查文件是否存在
                for f in &found {
                    if !std::path::Path::new(f).exists() {
                        eprintln!("[launch]   ★ MISSING FILE: {}", f);
                    }
                }
            }
        }
    }

    eprintln!("[launch] =================== 启动命令行 ===================");
    for (i, a) in flat_args.iter().enumerate() {
        if a.len() > 350 {
            eprintln!("[launch]   [{}] {}...", i, &a[..350]);
        } else {
            eprintln!("[launch]   [{}] {}", i, a);
        }
    }
    eprintln!("[launch] ===================================================");

    // ── 9. 预启动处理 ────────────────────────────
    pre_run_setup(&ctx, &version)?;

    // ── 10. 启动进程 ─────────────────────────────
    let child = spawn_process(&ctx, &flat_args)?;
    let pid = child.id();
    *RUNNING_PID.lock().unwrap() = Some(pid);

    let _ = app.emit("launch-started", serde_json::json!({
        "pid": pid,
        "instance": ctx.instance_name,
    }));

    // ── 11. 监控进程 ─────────────────────────────
    monitor_process(app, child, ctx);

    Ok(())
}

// ── 预检测 ─────────────────────────────────────────────────

fn pre_check(ctx: &LaunchContext) -> Result<(), String> {
    // PCL: ! 和 ; 会导致 bat 解析错误 (虽然我们不用 bat 了，但保持检查)
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
            "版本 JSON 不存在: {}.json\n请先安装该版本。", ctx.instance_name
        ));
    }

    // 检查 Java
    if !ctx.java_path.exists() {
        return Err(format!(
            "Java 路径无效: {}\n请在设置中选择正确的 Java。", ctx.java_path.display()
        ));
    }

    // PCL: 非 ASCII 用户名警告
    if !ctx.player_name.chars().all(|c| c.is_ascii()) {
        eprintln!("[launch] 警告: 玩家名包含非 ASCII 字符，1.10- 版本可能崩溃");
    }

    Ok(())
}

// ── 父版本 JSON/JAR 确保 ───────────────────────────────────

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
        serde_json::from_str(&content).ok().unwrap_or_default();

    let parent_name = match v["inheritsFrom"].as_str() {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()),
    };

    let teas = teas_dir()?;
    let cache_dir = teas.join("vanilla");
    std::fs::create_dir_all(&cache_dir).ok();

    let cache_json = cache_dir.join(format!("{}.json", parent_name));
    let parent_json = ctx.dot_minecraft
        .join("versions").join(parent_name)
        .join(format!("{}.json", parent_name));

    // HMCL-style: 缓存优先
    if !cache_json.exists() && !parent_json.exists() {
        eprintln!("[launch] 下载父版本 JSON: {}", parent_name);
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

    // 下载父版本 JAR
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

// ── Natives 准备 ────────────────────────────────────────────

/// PCL-style: 清空 natives 目录后重新解压所有 native DLL
///
/// 参照 PCL McLaunchNatives:
/// - 逐个 native JAR 解压 DLL 到版本专用目录
/// - 记录已解压文件列表
/// - 删除旧的多余文件
/// - 非 ASCII 路径回退 (PCL GetNativesFolder)
fn prepare_natives(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<PathBuf, String> {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let mut natives_dir = version_dir.join(format!("{}-natives", ctx.instance_name));

    // PCL GetNativesFolder: 非 ASCII 路径回退
    if !is_ascii_path(&natives_dir) {
        eprintln!("[launch] Natives 路径含非 ASCII 字符，使用回退路径");
        // 第一回退: %APPDATA%\.minecraft\bin\natives\
        let appdata = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        let fallback1 = appdata.join(".minecraft").join("bin").join("natives");
        if is_ascii_path(&fallback1) {
            natives_dir = fallback1;
        } else {
            // 第二回退: C:\ProgramData\PCL\natives\ (或等价目录)
            let fallback2 = PathBuf::from("C:\\ProgramData\\TeasLauncher\\natives");
            std::fs::create_dir_all(&fallback2).ok();
            if is_ascii_path(&fallback2) {
                natives_dir = fallback2;
            }
        }
    }

    std::fs::create_dir_all(&natives_dir)
        .map_err(|e| format!("创建 natives 目录失败: {}", e))?;

    // ★ PCL: 每次启动都清空并重新解压 Natives
    eprintln!("[launch] 开始解压 Natives...");
    clean_directory(&natives_dir)?;

    // ★ 创建版本 JSON 中指定的 natives 子目录
    // 版本 JSON 使用 ${natives_directory}/java, /jna, /lwjgl, /netty
    // Java 25 需要这些子目录存在，否则 java.library.path 可能失败
    for sub in &["java", "jna", "lwjgl", "netty"] {
        std::fs::create_dir_all(natives_dir.join(sub)).ok();
    }

    let libs_dir = ctx.dot_minecraft.join("libraries");
    let existing_files = extract_all_natives(version, &libs_dir, &natives_dir)?;

    cleanup_orphaned_files(&natives_dir, &existing_files)?;

    eprintln!("[launch] Natives 解压完成: {} 个文件", existing_files.len());

    Ok(natives_dir)
}

/// 从所有 native library JAR 中提取 DLL 文件
fn extract_all_natives(
    version: &serde_json::Value,
    libs_dir: &Path,
    natives_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut existing_files: Vec<PathBuf> = Vec::new();
    let libs = match version["libraries"].as_array() {
        Some(l) => l,
        None => return Ok(existing_files),
    };

    // 收集所有 native library
    let native_libs: Vec<&serde_json::Value> = libs.iter().filter(|lib| {
        lib.get("natives").is_some()
            || lib["name"].as_str().map_or(false, |n| {
                let parts: Vec<&str> = n.split(':').collect();
                parts.len() >= 4 && parts[3].contains("natives")
            })
    }).collect();

    for lib in &native_libs {
        let name = lib["name"].as_str().unwrap_or("");

        // 解析 library 路径
        let lib_path = get_library_path(name, lib, libs_dir);
        if lib_path.is_none() {
            continue;
        }
        let jar_path = lib_path.unwrap();

        if !jar_path.exists() {
            eprintln!("[launch] Natives JAR 不存在: {}", jar_path.display());
            continue;
        }

        // 获取 extract 规则
        let extract_rules = lib.get("extract").cloned();

        match std::fs::File::open(&jar_path) {
            Ok(file) => {
                match zip::ZipArchive::new(file) {
                    Ok(mut archive) => {
                        for i in 0..archive.len() {
                            if let Ok(mut entry) = archive.by_index(i) {
                                let entry_name = entry.name().to_string();

                                // 跳过目录
                                if entry.is_dir() {
                                    continue;
                                }

                                // 获取文件名
                                let dest_name = Path::new(&entry_name)
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string());

                                let dest_name = match dest_name {
                                    Some(n) => n,
                                    None => continue,
                                };

                                // 检查 extract rules
                                if let Some(ref rules) = extract_rules {
                                    if let Some(exclude) = rules.get("exclude").and_then(|e| e.as_array()) {
                                        if exclude.iter().any(|s| s.as_str() == Some(&entry_name)) {
                                            continue;
                                        }
                                    }
                                }

                                // 只提取 native 库文件
                                let is_native_file = dest_name.ends_with(".dll")
                                    || dest_name.ends_with(".so")
                                    || dest_name.ends_with(".dylib")
                                    || dest_name.ends_with(".jnilib");
                                if !is_native_file {
                                    continue;
                                }

                                // 跳过 .sha1 和 .git 文件
                                if dest_name.ends_with(".sha1") || dest_name.ends_with(".git") {
                                    continue;
                                }

                                let dest_path = natives_dir.join(&dest_name);

                                // PCL: 如果已存在且大小相同则跳过
                                if dest_path.exists() {
                                    if let Ok(meta) = std::fs::metadata(&dest_path) {
                                        if meta.len() == entry.size() {
                                            existing_files.push(dest_path);
                                            continue;
                                        }
                                    }
                                    let _ = std::fs::remove_file(&dest_path);
                                }

                                let mut out = std::fs::File::create(&dest_path)
                                    .map_err(|e| format!(
                                        "创建文件失败 {}: {}",
                                        dest_path.display(), e
                                    ))?;
                                std::io::copy(&mut entry, &mut out)
                                    .map_err(|e| format!("解压失败: {}", e))?;
                                existing_files.push(dest_path);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[launch] 无法打开 Natives JAR (可能已损坏): {} ({})",
                            jar_path.display(), e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[launch] 无法读取 Natives JAR: {} ({})",
                    jar_path.display(), e
                );
            }
        }
    }

    Ok(existing_files)
}

/// 根据 Maven 命名获取 library 的实际文件路径
fn get_library_path(name: &str, lib: &serde_json::Value, libs_dir: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }

    let (group, artifact, version_str) = (parts[0], parts[1], parts[2]);

    // 确定 classifier
    let classifier = if parts.len() >= 4 {
        let raw = parts[3];
        // ${arch} 替换
        let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
        raw.replace("${arch}", arch)
    } else {
        // 从 natives 字段获取
        if let Some(natives_obj) = lib.get("natives") {
            let os_key = if cfg!(target_os = "windows") { "windows" }
                else if cfg!(target_os = "macos") { "osx" }
                else { "linux" };
            natives_obj.get(os_key)
                .and_then(|v| v.as_str())
                .map(|c| {
                    let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
                    c.replace("${arch}", arch)
                })?
                .to_string()
        } else {
            return None;
        }
    };

    let jar_name = if classifier.is_empty() {
        format!("{}-{}.jar", artifact, version_str)
    } else {
        format!("{}-{}-{}.jar", artifact, version_str, classifier)
    };

    Some(
        libs_dir
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version_str)
            .join(jar_name)
    )
}

/// PCL-style: 删除 natives 目录中的多余文件
fn cleanup_orphaned_files(
    natives_dir: &Path,
    keep_files: &[PathBuf],
) -> Result<(), String> {
    if !natives_dir.exists() {
        return Ok(());
    }

    if let Ok(entries) = std::fs::read_dir(natives_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && !keep_files.contains(&path) {
                eprintln!("[launch] 删除孤立 natives 文件: {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

/// HMCL-style: cleanDirectoryQuietly
fn clean_directory(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

fn is_ascii_path(p: &Path) -> bool {
    p.to_string_lossy().chars().all(|c| c.is_ascii())
}

// ── 参数构建 (核心) ──────────────────────────────────────────

/// PCL-style: 构建完整的命令行参数列表
///
/// ★ PCL 参数顺序 (从 LatestLaunch.bat 解析):
///   1. java.exe
///   2. 版本 JSON 的 JVM 参数 (含 -cp ${classpath})  ← 优先放置
///   3. 用户自定义 JVM 参数 + 生成的 JVM 参数 (内存/GC/编码等) ← 放在 -cp 之后
///   4. Main class
///   5. 游戏参数
///
/// **关键**: PCL 把 -cp 放在 EARLY 位置，用户 JVM 参数放在 -cp 之后。
/// Java 接收在 -cp 之后、main class 之前的所有 JVM 选项。
fn build_flat_args(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> Result<Vec<String>, String> {
    let java_version = detect_java_version(&ctx.java_path)?;
    let is_64bit = is_64bit_java(&ctx.java_path);

    // ── 构建占位符映射 ──────────────────────────
    let (placeholders, cp_str) = build_placeholders(ctx, version, natives_dir);

    let resolve = |s: &str| -> String {
        let mut result = s.to_string();
        for (key, val) in &placeholders {
            result = result.replace(key.as_str(), val.as_str());
        }
        result
    };

    // ═══════════════════════════════════════════════════════
    // PHASE 1: Java exe + 版本 JSON JVM args (PCL: 优先放置)
    // ═══════════════════════════════════════════════════════

    let mut args: Vec<String> = Vec::new();

    // 1a. Java executable
    let java_exe = ctx.java_path
        .to_string_lossy()
        .replace("javaw.exe", "java.exe")
        .replace("javaw", "java");
    args.push(java_exe);

    // 1b. 版本 JSON 的 JVM 参数 (含 -cp, -Djava.library.path, natives paths, brand)
    // ★ PCL: 版本 JSON args 在命令行中最靠前
    let mut version_json_has_cp = false;
    if let Some(jvm_list) = version["arguments"]["jvm"].as_array() {
        for arg in jvm_list {
            if let Some(s) = arg.as_str() {
                let resolved = resolve(s);
                if resolved == "-cp" || resolved == "-classpath" {
                    version_json_has_cp = true;
                }
                args.push(resolved);
            } else if let Some(obj) = arg.as_object() {
                if !check_os_rules(obj) { continue; }
                if let Some(val) = obj.get("value") {
                    if let Some(s) = val.as_str() {
                        let resolved = resolve(s);
                        if resolved == "-cp" || resolved == "-classpath" {
                            version_json_has_cp = true;
                        }
                        args.push(resolved);
                    } else if let Some(arr) = val.as_array() {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                let resolved = resolve(s);
                                if resolved == "-cp" || resolved == "-classpath" {
                                    version_json_has_cp = true;
                                }
                                args.push(resolved);
                            }
                        }
                    }
                }
            }
        }
    }

    // 如果版本 JSON 没有提供 -cp（旧版格式），补充
    if !version_json_has_cp {
        args.push("-cp".to_string());
        args.push(cp_str.clone());
    }

    // ═══════════════════════════════════════════════════════
    // PHASE 2: 用户自定义 + 生成的 JVM 参数 (PCL: 放在 -cp 之后)
    // ═══════════════════════════════════════════════════════

    let mut user_args: Vec<String> = Vec::new();

    // ★ PCL order 1: -XX:-OmitStackTraceInFastThrow
    user_args.push("-XX:-OmitStackTraceInFastThrow".to_string());

    // ★ PCL order 2: 安全/兼容标志
    user_args.push("-Djdk.lang.Process.allowAmbiguousCommands=True".to_string());
    user_args.push("-Dfml.ignoreInvalidMinecraftCertificates=True".to_string());
    user_args.push("-Dfml.ignorePatchDiscrepancies=True".to_string());

    // ★ PCL order 3: 用户自定义 JVM 参数 + 内存
    if !ctx.jvm_args.is_empty() {
        let mut override_args = split_args_keep_quoting(&ctx.jvm_args);
        user_args.append(&mut override_args);
    }
    user_args.push(format!("-Xmx{}m", ctx.max_memory));
    if let Some(min) = ctx.min_memory {
        if min > 0 && min <= ctx.max_memory {
            user_args.push(format!("-Xms{}m", min));
        }
    }

    // ★ PCL order 4: GC 优化 (先清除已有 GC 再添加)
    // ★ PCL McLaunchArgumentsJVM: 先移除已有的 GC 参数，再添加启动器选择的 GC
    // 避免 "Multiple garbage collectors selected" 错误
    if ctx.gc_mode != GcMode::Custom {
        // PCL: 移除已有的 GC 相关参数 (Use*GC, ZGenerational, UseCompactObjectHeaders, G1*, Max/Min*)
        let gc_patterns = [
            "-XX:+UseG1GC", "-XX:-UseG1GC",
            "-XX:+UseZGC", "-XX:-UseZGC",
            "-XX:+UseSerialGC", "-XX:-UseSerialGC",
            "-XX:+UseParallelGC", "-XX:-UseParallelGC",
            "-XX:+UseConcMarkSweepGC", "-XX:-UseConcMarkSweepGC",
            "-XX:+UseShenandoahGC", "-XX:-UseShenandoahGC",
            "-XX:+ZGenerational", "-XX:-ZGenerational",
            "-XX:+UseCompactObjectHeaders", "-XX:-UseCompactObjectHeaders",
        ];
        let prefix_patterns = [
            "-XX:G1NewSizePercent", "-XX:G1ReservePercent",
            "-XX:G1HeapRegionSize", "-XX:MaxGCPauseMillis",
            "-XX:MinHeapFreeRatio", "-XX:MaxHeapFreeRatio",
            "-XX:G1MixedGCCountTarget", "-XX:+ParallelRefProcEnabled",
            "-XX:+PerfDisableSharedMem",
        ];
        user_args.retain(|a| {
            let a_clean = a.trim();
            !gc_patterns.iter().any(|p| a_clean == *p)
            && !prefix_patterns.iter().any(|p| a_clean.starts_with(p))
        });

        user_args.push("-XX:+UnlockExperimentalVMOptions".to_string());

        // ★ PCL + HMCL: 默认使用 G1GC (ZGC 在新版 LWJGL + Java 25 上可能有兼容性问题)
        let use_g1gc = match ctx.gc_mode {
            GcMode::G1GC => true,
            GcMode::Auto => true,  // 默认 G1GC (PCL 行为)
            GcMode::Custom => false,
        };

        if use_g1gc {
            if java_version >= 24 && is_64bit {
                user_args.push("-XX:+UseCompactObjectHeaders".to_string());
            }
            user_args.push("-XX:+UseG1GC".to_string());
            user_args.push("-XX:G1NewSizePercent=20".to_string());
            user_args.push("-XX:G1ReservePercent=20".to_string());
            user_args.push("-XX:G1HeapRegionSize=32M".to_string());
            user_args.push("-XX:MaxGCPauseMillis=50".to_string());
            user_args.push("-XX:+PerfDisableSharedMem".to_string());
            if java_version >= 12 {
                user_args.push("-XX:MinHeapFreeRatio=25".to_string());
                user_args.push("-XX:MaxHeapFreeRatio=40".to_string());
            }
        } else {
            user_args.push("-XX:+UseZGC".to_string());
            if java_version == 21 || java_version == 22 {
                user_args.push("-XX:+ZGenerational".to_string());
            }
        }

        if !is_64bit {
            user_args.push("-Xss1m".to_string());
        }
    }

    // PCL order 5: log4j
    user_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());

    // PCL order 6: 编码
    if java_version >= 9 {
        user_args.push("-Dstdout.encoding=UTF-8".to_string());
        user_args.push("-Dstderr.encoding=UTF-8".to_string());
    }
    if java_version >= 18 {
        user_args.push("-Dfile.encoding=COMPAT".to_string());
    }

    // Java 版本兼容 (仅当版本 JSON 未提供时)
    let version_json_flags: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    if java_version == 16 && !version_json_flags.contains(&"--illegal-access=permit") {
        user_args.push("--illegal-access=permit".to_string());
    }
    if (java_version == 24 || java_version == 25)
        && !version_json_flags.contains(&"--sun-misc-unsafe-memory-access=allow")
    {
        user_args.push("--sun-misc-unsafe-memory-access=allow".to_string());
    }

    // 2i. 清理问题参数
    user_args.retain(|a| a != "-XX:MaxDirectMemorySize=256M");

    // 用户参数去重
    let user_args = deduplicate_args(&user_args, true);
    args.append(&mut user_args.clone());

    // ═══════════════════════════════════════════════════════
    // PHASE 3: Java agents (在 main class 之前)
    // ═══════════════════════════════════════════════════════

    // ★ PCL: LWJGL 3.4.1 需要 LUA javaagent
    // LWJGL 3.4.1 将 Java 类从主 JAR 移动到了 native JAR，
    // 并重构了回调 API。LUA agent 提供兼容性。
    let needs_lua = version["libraries"].as_array().map_or(false, |libs| {
        libs.iter().any(|lib| {
            lib["name"].as_str().unwrap_or("") == "org.lwjgl:lwjgl:3.4.1"
        })
    });
    if needs_lua {
        // 将嵌入的 LUA.jar 写出到 .minecraft/ 下
        let lua_path = ctx.dot_minecraft.join("lua-agent.jar");
        if !lua_path.exists() {
            if let Ok(mut f) = std::fs::File::create(&lua_path) {
                let _ = f.write_all(LUA_JAR_BYTES);
            }
        }
        if lua_path.exists() {
            eprintln!("[launch] 已添加 LUA agent (LWJGL 3.4.1 兼容层)");
            args.push(format!("-javaagent:{}", lua_path.display()));
        }
    }

    for agent in &ctx.java_agents {
        args.push(format!("-javaagent:{}", agent));
    }

    // ═══════════════════════════════════════════════════════
    // PHASE 4: Main class
    // ═══════════════════════════════════════════════════════
    let main_class = version["mainClass"]
        .as_str()
        .unwrap_or("net.minecraft.client.main.Main");
    args.push(main_class.to_string());

    // ═══════════════════════════════════════════════════════
    // PHASE 5: 游戏参数
    // ═══════════════════════════════════════════════════════
    let mut game_args: Vec<String> = Vec::new();
    let mut has_version_json_game_args = false;

    if let Some(mc_args_str) = version["minecraftArguments"].as_str() {
        if !mc_args_str.is_empty() {
            has_version_json_game_args = true;
            let resolved_game = resolve(mc_args_str);
            game_args.extend(split_args_keep_quoting(&resolved_game));
            game_args.push("--height".to_string());
            game_args.push(ctx.window_height.to_string());
            game_args.push("--width".to_string());
            game_args.push(ctx.window_width.to_string());
        }
    }

    if let Some(game_list) = version["arguments"]["game"].as_array() {
        if !game_list.is_empty() {
            has_version_json_game_args = true;
        }
        for arg in game_list {
            if let Some(s) = arg.as_str() {
                if s.contains("${quickPlay") || s.contains("${QuickPlay") { continue; }
                game_args.push(resolve(s));
            } else if let Some(obj) = arg.as_object() {
                if !check_os_rules(obj) { continue; }
                if let Some(val) = obj.get("value") {
                    if let Some(s) = val.as_str() {
                        game_args.push(resolve(s));
                    } else if let Some(arr) = val.as_array() {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                game_args.push(resolve(s));
                            }
                        }
                    }
                }
            }
        }
    }

    // ★ 当版本 JSON 没有提供游戏参数时（如某些 Fabric 整合包），添加必要的默认参数
    if !has_version_json_game_args {
        let asset_index = version["assets"].as_str().unwrap_or("legacy");
        let version_type = version["type"].as_str().unwrap_or("release");
        let version_id = version["id"].as_str().unwrap_or(&ctx.instance_name);
        game_args.push("--username".to_string());
        game_args.push(ctx.player_name.clone());
        game_args.push("--version".to_string());
        game_args.push(version_id.to_string());
        game_args.push("--gameDir".to_string());
        game_args.push(resolve("${game_directory}"));
        game_args.push("--assetsDir".to_string());
        game_args.push(resolve("${assets_root}"));
        game_args.push("--assetIndex".to_string());
        game_args.push(asset_index.to_string());
        game_args.push("--uuid".to_string());
        game_args.push(resolve("${auth_uuid}"));
        game_args.push("--accessToken".to_string());
        game_args.push(resolve("${auth_access_token}"));
        game_args.push("--versionType".to_string());
        game_args.push(version_type.to_string());
        game_args.push("--width".to_string());
        game_args.push(ctx.window_width.to_string());
        game_args.push("--height".to_string());
        game_args.push(ctx.window_height.to_string());
    }

    // OptiFineForgeTweaker 移到末尾
    if let Some(pos) = game_args.iter().position(|a| a == "--tweakClass") {
        if pos + 1 < game_args.len() {
            let tc = game_args.remove(pos + 1);
            game_args.remove(pos);
            game_args.push("--tweakClass".to_string());
            game_args.push(tc);
        }
    }

    if !ctx.game_args.is_empty() {
        game_args.extend(split_args_keep_quoting(&ctx.game_args));
    }

    let game_args = deduplicate_args(&game_args, false);
    args.extend(game_args);

    Ok(args)
}

// ── 占位符映射 ───────────────────────────────────────────────

fn build_placeholders(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> (Vec<(String, String)>, String) {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let assets_dir = ctx.dot_minecraft.join("assets");
    let game_dir = &ctx.dot_minecraft;

    let uuid = offline_uuid(&ctx.player_name);
    let launcher_ver = format!("v{}.{:04}", env!("CARGO_PKG_VERSION"), BUILD);
    let asset_index = version["assets"].as_str().unwrap_or("legacy");

    // 构建 classpath 字符串
    let cp_entries = build_classpath(ctx, version);
    #[cfg(target_os = "windows")]
    let cp_separator = ";";
    #[cfg(not(target_os = "windows"))]
    let cp_separator = ":";
    let cp_str = cp_entries.join(cp_separator);

    let mut map: Vec<(String, String)> = Vec::new();

    // Mojang 官方占位符
    map.push(("${auth_player_name}".into(), ctx.player_name.clone()));
    map.push(("${auth_uuid}".into(), uuid.clone()));
    map.push(("${auth_access_token}".into(), "0".to_string()));
    map.push(("${access_token}".into(), "0".to_string()));
    map.push(("${auth_session}".into(), "0".to_string()));
    map.push(("${auth_xuid}".into(), "0".to_string()));
    map.push(("${clientid}".into(), "0".to_string()));
    map.push(("${user_type}".into(), "mojang".to_string()));
    map.push(("${user_properties}".into(), "{}".to_string()));
    map.push(("${version_name}".into(), ctx.instance_name.clone()));
    map.push(("${version_type}".into(),
        version["type"].as_str().unwrap_or("release").to_string()));
    map.push(("${assets_index_name}".into(), asset_index.to_string()));
    map.push(("${assets_root}".into(), assets_dir.display().to_string()));
    map.push(("${game_assets}".into(), assets_dir.display().to_string()));
    // game_directory 指向版本目录（版本隔离）
    let game_dir_for_mc = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    map.push(("${game_directory}".into(), game_dir_for_mc.display().to_string()));
    map.push(("${resolution_width}".into(), ctx.window_width.to_string()));
    map.push(("${resolution_height}".into(), ctx.window_height.to_string()));
    map.push(("${library_directory}".into(), libs_dir.display().to_string()));
    map.push(("${libraries_directory}".into(), libs_dir.display().to_string()));
    map.push(("${classpath_separator}".into(), ";".to_string()));
    map.push(("${file_separator}".into(), "\\".to_string()));
    map.push(("${natives_directory}".into(), natives_dir.display().to_string()));
    map.push(("${launcher_name}".into(), "TeasLauncher".to_string()));
    map.push(("${launcher_version}".into(), launcher_ver));
    map.push(("${profile_name}".into(), "TeasLauncher".to_string()));
    map.push(("${language}".into(), "zh_cn".to_string()));
    map.push(("${path}".into(), game_dir.display().to_string()));

    // 主 JAR
    let jar_path = version_dir.join(format!("{}.jar", ctx.instance_name));
    map.push(("${primary_jar}".into(), jar_path.display().to_string()));
    map.push(("${primary_jar_name}".into(), format!("{}.jar", ctx.instance_name)));

    // ★ 关键: classpath 必须作为占位符加入，版本 JSON 的 -cp ${classpath} 需要它
    map.push(("${classpath}".into(), cp_str.clone()));

    // QuickPlay 占位符 (无数据时设空)
    map.push(("${quickPlayPath}".into(), String::new()));
    map.push(("${quickPlaySingleplayer}".into(), String::new()));
    map.push(("${quickPlayMultiplayer}".into(), String::new()));
    map.push(("${quickPlayRealms}".into(), String::new()));

    (map, cp_str)
}

/// PCL-style classpath 构建:
/// - ★ 所有 library JAR 都加入 classpath（包括 native JAR！LWJGL 3.x 的类分布在 native JAR 中）
/// - OptiFine 放到倒数第二位
/// - 主 JAR 放到最后
fn build_classpath(ctx: &LaunchContext, version: &serde_json::Value) -> Vec<String> {
    let mut cp: Vec<String> = Vec::new();
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let version_jar = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.jar", ctx.instance_name));

    let mut optifine_path: Option<String> = None;

    if let Some(libs) = version["libraries"].as_array() {
        for lib in libs {
            let name = lib["name"].as_str().unwrap_or("");
            let parts: Vec<&str> = name.split(':').collect();
            if parts.len() < 3 {
                continue;
            }

            let (group, artifact, ver) = (parts[0], parts[1], parts[2]);

            // 确定 JAR 文件名
            let jar_filename = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
                .and_then(|p| std::path::Path::new(p).file_name())
                .map(|f| f.to_string_lossy().to_string());

            let jar_filename = jar_filename.unwrap_or_else(|| {
                if let Some(classifiers) = lib.get("downloads").and_then(|d| d.get("classifiers")) {
                    let windows_key = if cfg!(target_arch = "x86_64") {
                        "natives-windows"
                    } else if cfg!(target_arch = "aarch64") {
                        "natives-windows-arm64"
                    } else {
                        "natives-windows-x86"
                    };
                    if let Some(path) = classifiers.get(windows_key)
                        .and_then(|c| c.get("path"))
                        .and_then(|p| p.as_str())
                    {
                        if let Some(fname) = std::path::Path::new(path).file_name() {
                            return fname.to_string_lossy().to_string();
                        }
                    }
                }
                format!("{}-{}.jar", artifact, ver)
            });

            // ★ PCL: 使用全反斜杠路径 (Windows — Java 25+ 不接受混合斜杠的 classpath)
            let jar_path = libs_dir
                .join(group.replace('.', "\\"))
                .join(artifact)
                .join(ver)
                .join(&jar_filename);

            if jar_path.exists() {
                if name.starts_with("optifine:OptiFine")
                    || name.starts_with("optifine:optifine")
                {
                    optifine_path = Some(jar_path.display().to_string());
                } else {
                    cp.push(jar_path.display().to_string());
                }
            }
        }
    }

    // PCL: OptiFine 放到倒数第二位
    if let Some(optifine) = optifine_path {
        if cp.len() >= 2 {
            cp.insert(cp.len() - 1, optifine);
        } else {
            cp.push(optifine);
        }
    }

    // 主 JAR 放到最后 (使用标准 Windows 分隔符)
    if version_jar.exists() {
        cp.push(version_jar.display().to_string().replace('/', "\\"));
    }

    // ★ 所有路径统一为反斜杠 (PCL 风格，Java 25 兼容)
    cp.iter_mut().for_each(|p| {
        *p = p.replace('/', "\\");
    });

    cp
}

// ── 预启动处理 ──────────────────────────────────────────────

fn pre_run_setup(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<(), String> {
    // ── 提取 log4j2.xml (HMCL-style) ──────────
    let game_version = guess_game_version(version);
    if let Some(ref gv) = game_version {
        if compare_versions(gv, "1.7") >= std::cmp::Ordering::Equal {
            extract_log4j_config(ctx, version)?;
        }
    }

    // ── 更新 options.txt 语言 ─────────────────
    update_options_txt(ctx);

    Ok(())
}

/// HMCL-style: 提取 log4j2.xml 到版本目录
fn extract_log4j_config(
    _ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<(), String> {
    let version_dir = _ctx.dot_minecraft
        .join("versions").join(&_ctx.instance_name);
    let target = version_dir.join("log4j2.xml");

    if target.exists() {
        return Ok(());
    }

    let game_version = guess_game_version(version).unwrap_or_default();
    let is_legacy = compare_versions(&game_version, "1.12") == std::cmp::Ordering::Less;

    let log4j_xml = if is_legacy {
        // 1.7-1.11: 旧版 log4j 2.0-beta9
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="WARN" packages="com.mojang.util">
    <Appenders>
        <Console name="SysOut" target="SYSTEM_OUT">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
        </Console>
        <Queue name="ServerGuiConsole">
            <PatternLayout pattern="[%d{HH:mm:ss} %level]: %msg{nolookups}%n" />
        </Queue>
        <RollingRandomAccessFile name="File" fileName="logs/latest.log" filePattern="logs/%d{yyyy-MM-dd}-%i.log.gz">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
            <Policies>
                <TimeBasedTriggeringPolicy />
                <OnStartupTriggeringPolicy />
            </Policies>
        </RollingRandomAccessFile>
    </Appenders>
    <Loggers>
        <Root level="info">
            <filters>
                <MarkerFilter marker="NETWORK_PACKETS" onMatch="DENY" onMismatch="NEUTRAL" />
            </filters>
            <AppenderRef ref="SysOut"/>
            <AppenderRef ref="File"/>
        </Root>
    </Loggers>
</Configuration>"#.to_string()
    } else {
        // 1.12+: 新版 log4j 2.x
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="WARN">
    <Appenders>
        <Console name="SysOut" target="SYSTEM_OUT">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
        </Console>
        <Queue name="ServerGuiConsole">
            <PatternLayout pattern="[%d{HH:mm:ss} %level]: %msg{nolookups}%n" />
        </Queue>
        <RollingRandomAccessFile name="File" fileName="logs/latest.log" filePattern="logs/%d{yyyy-MM-dd}-%i.log.gz">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
            <Policies>
                <TimeBasedTriggeringPolicy />
                <OnStartupTriggeringPolicy />
            </Policies>
        </RollingRandomAccessFile>
    </Appenders>
    <Loggers>
        <Root level="info">
            <filters>
                <MarkerFilter marker="NETWORK_PACKETS" onMatch="DENY" onMismatch="NEUTRAL" />
            </filters>
            <AppenderRef ref="SysOut"/>
            <AppenderRef ref="File"/>
            <AppenderRef ref="ServerGuiConsole"/>
        </Root>
    </Loggers>
</Configuration>"#.to_string()
    };

    std::fs::write(&target, &log4j_xml)
        .map_err(|e| format!("写入 log4j2.xml 失败: {}", e))?;

    eprintln!("[launch] 已提取 log4j2.xml");
    Ok(())
}

/// PCL-style: 更新 options.txt 语言设置
fn update_options_txt(ctx: &LaunchContext) {
    let options_path = ctx.dot_minecraft.join("options.txt");

    if !options_path.exists() {
        return;
    }

    if let Ok(content) = std::fs::read_to_string(&options_path) {
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let mut changed = false;
        for line in &mut lines {
            if line.starts_with("lang:") {
                if *line != "lang:zh_cn" {
                    *line = "lang:zh_cn".to_string();
                    changed = true;
                }
            }
        }
        if changed {
            let _ = std::fs::write(&options_path, lines.join("\n"));
            eprintln!("[launch] 已更新 options.txt 语言设置");
        }
    }
}

// ── 进程启动 (HMCL-style) ───────────────────────────────────

/// HMCL/PCL-style: 直接使用 ProcessBuilder 启动进程
///
/// - 接收 Vec<String> (flat args，不做字符串拼接→再分割)
/// - HMCL: ProcessBuilder with directory + environment
/// - PCL: ProcessStartInfo with RedirectStandardOutput + RedirectStandardError
fn spawn_process(
    ctx: &LaunchContext,
    flat_args: &[String],
) -> Result<std::process::Child, String> {
    if flat_args.is_empty() {
        return Err("启动参数为空".to_string());
    }

    let program = &flat_args[0];
    let args: &[String] = &flat_args[1..];

    // 工作目录 = 版本目录（版本隔离）
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let version_dir_str = version_dir.display().to_string();
    let working_dir = version_dir;

    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd.current_dir(&working_dir);

    // ── 环境变量 (匹配 PCL) ──────────────────
    cmd.env("APPDATA", ctx.dot_minecraft.display().to_string());

    if let Some(java_bin) = ctx.java_path.parent() {
        let existing_path = std::env::var("Path").unwrap_or_default();
        cmd.env("Path", format!("{};{}", java_bin.display(), existing_path));
    }

    cmd.env("INST_NAME", &ctx.instance_name);
    cmd.env("INST_ID", &ctx.instance_name);
    cmd.env("INST_DIR", &version_dir_str);
    cmd.env("INST_MC_DIR", ctx.dot_minecraft.display().to_string());
    cmd.env("INST_JAVA", ctx.java_path.display().to_string());

    // 用户自定义环境变量
    for (key, val) in &ctx.env_vars {
        cmd.env(key, val);
    }

    // ── 重定向输出 ────────────────────────────
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());

    // ── Windows: 不显示控制台窗口 ──────────────
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn().map_err(|e| {
        format!(
            "启动 Minecraft 失败: {}\nJava 路径: {}\n\
             请检查 Java 是否正确安装。\n\
             如果是 JDK-8272352 的编码问题，可尝试将游戏放置在全英文路径下。",
            e, ctx.java_path.display()
        )
    })?;

    Ok(child)
}

// ── 进程监控 (HMCL-style) ───────────────────────────────────

/// HMCL-style: 启动 stdout/stderr 流监控 + 退出等待
///
/// 参照 HMCL DefaultLauncher.startMonitors:
/// - stdout pump thread
/// - stderr pump thread
/// - ExitWaiter thread (检测退出码 + 崩溃检测)
fn monitor_process(
    app: tauri::AppHandle,
    mut child: std::process::Child,
    ctx: LaunchContext,
) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // HMCL: stdout pump thread
    if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let instance = ctx.instance_name.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(text) = line {
                    eprintln!("[MC:stdout] {}", text);
                    let _ = app_clone.emit("game-log", serde_json::json!({
                        "instance": instance,
                        "stream": "stdout",
                        "message": text,
                    }));
                }
            }
        });
    }

    // HMCL: stderr pump thread
    if let Some(stderr) = stderr {
        let app_clone = app.clone();
        let instance = ctx.instance_name.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(text) = line {
                    eprintln!("[MC:stderr] {}", text);
                    let _ = app_clone.emit("game-log", serde_json::json!({
                        "instance": instance,
                        "stream": "stderr",
                        "message": text,
                    }));
                }
            }
        });
    }

    // HMCL: ExitWaiter
    let app_clone = app;
    let instance_name = ctx.instance_name.clone();
    let mc_dir_str = ctx.mc_dir.display().to_string();

    tokio::spawn(async move {
        let status = child.wait();
        let exit_code = status.as_ref().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

        // 清除 PID
        *RUNNING_PID.lock().unwrap() = None;

        eprintln!("[launch] Minecraft 进程已退出，退出码: {}", exit_code);

        if exit_code == 0 {
            eprintln!("[launch] {} 正常退出", instance_name);
            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "crashed": false,
            }));
        } else {
            eprintln!("[launch] {} 异常退出 ({}), 开始崩溃检测...", instance_name, exit_code);

            // 等待日志文件写完
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let crash_result = crate::crash::check_crash(
                mc_dir_str, instance_name.clone(),
            );

            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "crashed": crash_result.is_some(),
                "crashInfo": crash_result,
            }));
        }
    });
}

// ── 工具函数 ────────────────────────────────────────────────

/// HMCL/PCL-style: 检测 Java 主版本号
///
/// 从 `java -version` 的 stderr 输出解析
/// Java 输出格式:
///   java version "1.8.0_371"     → Major = 8
///   openjdk version "17.0.9"     → Major = 17
///   java version "21.0.1"        → Major = 21
fn detect_java_version(java_path: &Path) -> Result<u32, String> {
    let output = std::process::Command::new(java_path)
        .arg("-version")
        .output()
        .map_err(|_| format!("无法执行 Java: {}", java_path.display()))?;

    // Java 把版本信息输出到 stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    // 尝试从 "version" 后提取版本号
    for line in combined.lines() {
        if let Some(ver_str) = line.split("version").nth(1) {
            let ver_str = ver_str.trim().trim_matches('"').trim_matches('\'');
            // "1.8.0_371" → major=8
            // "17.0.9" → major=17
            if let Some(dot_pos) = ver_str.find('.') {
                if ver_str.starts_with("1.") {
                    // Java 1.x → 取第二个数字
                    if let Some(rest) = ver_str.strip_prefix("1.") {
                        if let Some(next_dot) = rest.find('.') {
                            return rest[..next_dot].parse().map_err(|_| "无法解析 Java 版本".to_string());
                        }
                        return rest.parse().map_err(|_| "无法解析 Java 版本".to_string());
                    }
                } else {
                    // Java 9+ → 取第一个数字
                    return ver_str[..dot_pos].parse().map_err(|_| "无法解析 Java 版本".to_string());
                }
            }
        }
    }

    // 回退: 尝试 XshowSettings
    let output2 = std::process::Command::new(java_path)
        .args(["-XshowSettings:all", "-version"])
        .output();
    if let Ok(out) = output2 {
        let stderr2 = String::from_utf8_lossy(&out.stderr);
        for line in stderr2.lines() {
            if line.contains("java.runtime.version") || line.contains("java.version") {
                for part in line.split_whitespace() {
                    if part.contains('.') && part.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                        if let Some(dot_pos) = part.find('.') {
                            if part.starts_with("1.") {
                                let rest = &part[2..];
                                if let Some(next_dot) = rest.find('.') {
                                    return rest[..next_dot].parse().map_err(|_| "无法解析 Java 版本".to_string());
                                }
                            } else {
                                return part[..dot_pos].parse().map_err(|_| "无法解析 Java 版本".to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // 最后回退
    eprintln!("[launch] 无法检测 Java 版本，默认使用 Java 8");
    Ok(8)
}

/// 检测 Java 是否为 64-bit
fn is_64bit_java(java_path: &Path) -> bool {
    if let Ok(output) = std::process::Command::new(java_path)
        .arg("-version")
        .output()
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        format!("{}\n{}", stderr, stdout).contains("64-Bit")
    } else {
        cfg!(target_arch = "x86_64")
    }
}

/// HMCL-style: 检查 JSON rules 中的 OS 和 features
fn check_os_rules(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    if let Some(rules) = obj.get("rules").and_then(|r| r.as_array()) {
        let mut required = false;

        for rule in rules {
            let action = rule.get("action").and_then(|a| a.as_str()).unwrap_or("allow");
            let mut matches = true;

            // OS 检查
            if let Some(os) = rule.get("os") {
                let os_name = os.get("name").and_then(|n| n.as_str());

                #[cfg(target_os = "windows")]
                { matches = os_name == Some("windows") || os_name.is_none(); }
                #[cfg(target_os = "macos")]
                { matches = os_name == Some("osx") || os_name == Some("macos") || os_name.is_none(); }
                #[cfg(target_os = "linux")]
                { matches = os_name == Some("linux") || os_name.is_none(); }

                // arch 检查
                if matches {
                    if let Some(arch) = os.get("arch").and_then(|a| a.as_str()) {
                        let want_32bit = arch == "x86";
                        let is_32bit = !cfg!(target_arch = "x86_64");
                        matches = want_32bit == is_32bit;
                    }
                }

                // version 检查 (Windows)
                #[cfg(target_os = "windows")]
                if matches {
                    if let Some(ver) = os.get("version").and_then(|v| v.as_str()) {
                        // 简单处理: ^ 开头的正则不做精确匹配，直接允许
                        matches = ver.starts_with('^') || ver == "10.0" || ver.contains("Windows");
                    }
                }
            }

            // features 检查
            if matches {
                if let Some(features) = rule.get("features") {
                    if let Some(obj) = features.as_object() {
                        // has_custom_resolution: 我们有分辨率设置
                        if obj.contains_key("has_custom_resolution") { matches = true; }
                        // is_demo_user: 我们不是 demo 用户
                        if obj.contains_key("is_demo_user") { matches = false; }
                        // quick_play 相关: 不支持
                        for key in obj.keys() {
                            if key.contains("quick_play") || key.contains("quickPlay") {
                                matches = false;
                            }
                        }
                    }
                }
            }

            match action {
                "allow" => { if matches { required = true; } }
                "disallow" => { if matches { required = false; } }
                _ => {}
            }
        }

        return required;
    }

    true // 无 rules → 允许
}

/// 分割 Java 参数字符串 (保留引号内的空格)
///
/// PCL SplitJavaArguments:
/// - 引号内的空格不分割
/// - 转义引号 \" 正确处理
fn split_args_keep_quoting(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
            // 转义引号: 保留 \" 不切换 in_quotes
            current.push('\\');
            current.push('"');
            i += 1;
        } else if c == '"' {
            in_quotes = !in_quotes;
            // 不保留引号本身到参数中
        } else if c == ' ' && !in_quotes {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// PCL-style: 检查 args 中是否已存在某个前缀的参数
#[allow(dead_code)]
fn has_arg_prefix(args: &[String], prefix: &str) -> bool {
    args.iter().any(|a| a.starts_with(prefix))
}

/// PCL-style: Java 参数去重
///
/// - JVM 参数: 相同键直接删除重复项 (保留第一个)
/// - 游戏参数: 相同键用新值覆盖旧值 (除 --tweakClass 外)
fn deduplicate_args(args: &[String], is_jvm: bool) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let key = &args[i];

        // 非 - 开头或单独参数
        if !key.starts_with('-')
            || i + 1 >= args.len()
            || (args[i + 1].starts_with('-') && args[i + 1].chars().nth(1).map_or(false, |c| !c.is_ascii_digit()))
        {
            if !result.contains(key) {
                result.push(key.clone());
            }
            i += 1;
        } else {
            // 键值对
            let value = &args[i + 1];
            let to_skip;

            if !is_jvm && key != "--tweakClass" {
                // 游戏参数: 查找并覆盖
                let mut found = false;
                let mut j = 0;
                while j < result.len() {
                    if j + 1 < result.len() && result[j] == *key {
                        result[j + 1] = value.clone();
                        found = true;
                        break;
                    }
                    j += 1;
                }
                to_skip = found;
            } else {
                // JVM 参数或 --tweakClass: 查找并跳过重复
                let mut found = false;
                let mut j = 0;
                while j < result.len() {
                    if j + 1 < result.len() && result[j] == *key && result[j + 1] == *value {
                        found = true;
                        break;
                    }
                    j += 1;
                }
                to_skip = found;
            }

            if !to_skip {
                result.push(key.clone());
                result.push(value.clone());
            }
            i += 2;
        }
    }

    result
}

/// 从 version JSON 猜测 Minecraft 游戏版本号
fn guess_game_version(version: &serde_json::Value) -> Option<String> {
    if let Some(parent) = version["inheritsFrom"].as_str() {
        return Some(parent.to_string());
    }
    if let Some(id) = version["id"].as_str() {
        if !id.contains("forge") && !id.contains("fabric") && !id.contains("quilt")
            && !id.contains("neoforge") && !id.contains("liteloader")
        {
            return Some(id.to_string());
        }
    }
    if let Some(cv) = version["clientVersion"].as_str() {
        return Some(cv.to_string());
    }
    if let Some(jar) = version["jar"].as_str() {
        return Some(jar.to_string());
    }
    None
}

/// 简单语义版本比较
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}
