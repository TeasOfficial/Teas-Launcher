//! 游戏启动模块 — 参照 HMCL/PCL/PCL-CE/PrismLauncher 重写
//!
//! 架构:
//! 1. LaunchContext   — 结构化启动配置
//! 2. pre_launch      — 预检+补全文件+解压natives
//! 3. build_arguments — HMCL/PCL 风格的 JVM & Game 参数构建
//! 4. spawn_process   — 直接 ProcessBuilder 启动 (不用 bat)
//! 5. monitor_process — 流监控 + 退出处理

use crate::config::teas_dir;
use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_launcher_or_meta;
use crate::instance::resolve_version;
use crate::utils::{is_jar_valid, offline_uuid, scan_and_fix_jars_with_strategy, JarScanStrategy};
use crate::version::library::mclib_from_instance;
use crate::version::assets::{mcassets_fix_list, mcassets_get_index_name, download_asset_index};
use crate::BUILD;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::Emitter;

// ── 进程管理 ──────────────────────────────────────────────

static RUNNING_PID: Mutex<Option<u32>> = Mutex::new(None);
static CANCEL_FLAG: Mutex<bool> = Mutex::new(false);

/// 终止正在运行的 Minecraft 进程
#[tauri::command]
pub(crate) fn kill_instance() -> Result<(), String> {
    let mut p = RUNNING_PID.lock().unwrap();
    if let Some(pid) = *p {
        #[cfg(target_os = "windows")]
        {
            // PCL-style: 先用 taskkill /F /PID，再用 /T 杀子进程树
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
    // 设置取消标志，中断正在等待的启动流程
    *CANCEL_FLAG.lock().unwrap() = true;
    Ok(())
}

#[tauri::command]
pub(crate) fn is_instance_running() -> bool {
    RUNNING_PID.lock().unwrap().is_some()
}

// ── 启动上下文 ────────────────────────────────────────────

/// 结构化启动参数 — 参照 HMCL LaunchOptions
#[derive(Debug, Clone)]
struct LaunchContext {
    mc_dir: PathBuf,
    dot_minecraft: PathBuf,
    instance_name: String,
    player_name: String,
    java_path: PathBuf,
    max_memory: u32,       // MB
    min_memory: Option<u32>,
    jvm_args: String,       // 用户自定义 JVM 参数 (空格分隔)
    game_args: String,      // 用户自定义游戏参数
    window_width: u32,
    window_height: u32,
    // 从配置文件读取
    java_agents: Vec<String>,
    env_vars: Vec<(String, String)>,
    use_gc_optimize: bool,
    prefer_official: bool,
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

        // 解析内存 (支持 "2048M", "2G", "2048")
        let max_mem = parse_memory_mb(max_memory);

        let cfg = crate::config::config_read("user".to_string()).unwrap_or(serde_json::json!({}));
        let prefer_official = cfg
            .get("download_source")
            .and_then(|v| v.as_str())
            .map(|s| s == "Mojang")
            .unwrap_or(false);

        LaunchContext {
            mc_dir: base.to_path_buf(),
            dot_minecraft,
            instance_name: instance_name.to_string(),
            player_name: player_name.to_string(),
            java_path: PathBuf::from(java_path),
            max_memory: max_mem,
            min_memory: None,
            jvm_args: jvm_args.to_string(),
            game_args: String::new(),
            window_width,
            window_height,
            java_agents: Vec::new(),
            env_vars: Vec::new(),
            use_gc_optimize: true,
            prefer_official,
        }
    }
}

/// 解析内存字符串为 MB
/// 支持: "2048", "2048M", "2G", "2048m", "2g"
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
    let version_dir = dot_minecraft.join("versions").join(&instance_name);
    Ok(serde_json::json!({
        "java": ctx.java_path.display().to_string(),
        "jvm": ctx.jvm_args,
        "mem": ctx.max_memory,
        "gameDir": version_dir.display().to_string(),
        "width": ctx.window_width,
        "height": ctx.window_height,
        "player": ctx.player_name,
        "uuid": offline_uuid(&ctx.player_name)
    }))
}

// ── 主启动入口 ──────────────────────────────────────────────

/// 启动 Minecraft 实例（完整流程）
///
/// 流程参照 PCL 的步骤链:
/// 1. 预检测 (路径/版本)
/// 2. 扫描 JAR
/// 3. 确保父版本 JSON/JAR
/// 4. 补全 Libraries
/// 5. 下载 Asset Index + 补全 Assets
/// 6. 构建启动参数
/// 7. 解压 Natives
/// 8. 预启动处理 (log4j, launcher_profiles, options.txt)
/// 9. 启动进程
/// 10. 等待退出 + 崩溃检测
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
        let cfg = crate::config::config_read("user".to_string()).unwrap_or(serde_json::json!({}));
        JarScanStrategy::from_config(cfg.get("jar_scan_strategy").and_then(|v| v.as_str()).unwrap_or("B"))
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
    if let Some(asset_index_dl) = download_asset_index(&version, &ctx.dot_minecraft, ctx.prefer_official) {
        let mut dl = asset_index_dl;
        let _ = download_file(&app, &mut dl, false).await;
    }
    let index_name = mcassets_get_index_name(&version);
    if let Ok(assets) = mcassets_fix_list(&ctx.dot_minecraft, &index_name, true, ctx.prefer_official) {
        if !assets.is_empty() {
            let mut files: Vec<DownloadFile> = assets;
            crate::download::engine::download_files_parallel(&app, &mut files, 8).await;
        }
    }

    // ── 6. 重新解析版本 (父 JSON 可能刚下载) ────
    let version = resolve_version(&ctx.dot_minecraft, &ctx.instance_name)?;

    // ── 7. 解压 Natives (PCL-style: 清空后重新解压) ──
    let natives_dir = prepare_natives(&ctx, &version)?;

    // ── 8. 构建启动参数 ──────────────────────────
    let (java_cmd, game_args, _classpath_entries) = build_arguments(
        &ctx, &version, &natives_dir,
    )?;

    // ── 9. 预启动处理 ────────────────────────────
    pre_run_setup(&ctx, &version)?;

    // ── 10. 启动进程 ─────────────────────────────
    let child = spawn_process(&ctx, &java_cmd, &game_args)?;
    let pid = child.id();
    *RUNNING_PID.lock().unwrap() = Some(pid);

    // 发射启动成功事件到前端
    let _ = app.emit("launch-started", serde_json::json!({
        "pid": pid,
        "instance": ctx.instance_name,
    }));

    // ── 11. 监控进程 ─────────────────────────────
    monitor_process(app, child, ctx);

    Ok(())
}

// ── 预检测 ─────────────────────────────────────────────────

/// PCL-style 预检测: 检查路径合法性、版本 JSON 存在性
fn pre_check(ctx: &LaunchContext) -> Result<(), String> {
    // 检查路径中是否含有非法字符 (PCL: ! 和 ; 会导致 bat 解析错误)
    let path_str = ctx.dot_minecraft.display().to_string();
    if path_str.contains('!') || path_str.contains(';') {
        return Err(format!(
            "游戏路径含有非法字符 (! 或 ;): {}",
            path_str
        ));
    }

    // 检查版本 JSON
    let json_path = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.json", ctx.instance_name));
    if !json_path.exists() {
        return Err(format!(
            "版本 JSON 不存在: {}.json\n请先安装该版本。",
            ctx.instance_name
        ));
    }

    // 检查 Java
    if !ctx.java_path.exists() {
        return Err(format!(
            "Java 路径无效: {}\n请在设置中选择正确的 Java。",
            ctx.java_path.display()
        ));
    }

    // PCL: 检查用户名是否只含 ASCII (非 ASCII 用户名可能导致 NPE)
    if !ctx.player_name.chars().all(|c| c.is_ascii()) {
        eprintln!("[launch] 警告: 玩家名包含非 ASCII 字符，可能导致 1.10- 版本崩溃");
    }

    Ok(())
}

// ── 父版本 JSON/JAR 确保 ───────────────────────────────────

/// HMCL/PCL-style: 确保 inheritsFrom 父版本 JSON 和 JAR 已下载
async fn ensure_parent_version(
    app: &tauri::AppHandle,
    ctx: &LaunchContext,
) -> Result<(), String> {
    let json_path = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.json", ctx.instance_name));

    let content = std::fs::read_to_string(&json_path)
        .map_err(|e| format!("无法读取版本 JSON: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&content).ok().unwrap_or_default();

    let parent_name = match v["inheritsFrom"].as_str() {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()), // 无父版本
    };

    let teas = teas_dir()?;
    let cache_dir = teas.join("vanilla");
    std::fs::create_dir_all(&cache_dir).ok();

    let cache_json = cache_dir.join(format!("{}.json", parent_name));
    let parent_json = ctx.dot_minecraft
        .join("versions").join(parent_name)
        .join(format!("{}.json", parent_name));

    // HMCL-style: 缓存优先，减少网络请求
    if !cache_json.exists() && !parent_json.exists() {
        eprintln!("[launch] 下载父版本 JSON: {}", parent_name);
        let client = crate::http::build_http_client(std::time::Duration::from_secs(30))?;
        let manifest: serde_json::Value = client
            .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
            .send().await.map_err(|e| format!("获取版本清单失败: {}", e))?
            .json().await.map_err(|e| format!("解析版本清单失败: {}", e))?;

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

    // 复制缓存到 versions 目录 (如果目标不存在)
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
        // 从 version JSON 获取 JAR URL
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
/// - 逐个 native JAR 解压 DLL 到版本目录
/// - 记录已解压文件列表
/// - 删除旧的多余文件
/// - 支持非 ASCII 路径回退
fn prepare_natives(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<PathBuf, String> {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let natives_dir = version_dir.join(format!("{}-natives", ctx.instance_name));

    // PCL: 获取 natives 文件夹路径 (ASCII 路径检查)
    let natives_dir = if !is_ascii_path(&natives_dir) {
        // 非 ASCII 路径：回退到 APPDATA 目录 (PCL-style)
        eprintln!("[launch] Natives 路径含非 ASCII 字符，使用回退路径");
        let appdata = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let fallback = appdata.join(".minecraft").join("bin").join("natives");
        std::fs::create_dir_all(&fallback).ok();
        fallback
    } else {
        natives_dir
    };

    std::fs::create_dir_all(&natives_dir).map_err(|e| format!("创建 natives 目录失败: {}", e))?;

    // 检查是否需要重新解压 (PCL: 如果 lwjgl_opengl.dll 存在且版本匹配则跳过)
    let needs_extract = !natives_dir.join("lwjgl_opengl.dll").exists()
        && !natives_dir.join("lwjgl.dll").exists()
        && !natives_dir.join("lwjgl3.dll").exists();

    if needs_extract {
        eprintln!("[launch] 开始解压 Natives...");
        // 清空旧文件 (HMCL/PCL-style)
        clean_directory(&natives_dir)?;

        // 解压所有 native JAR
        let libs_dir = ctx.dot_minecraft.join("libraries");
        let existing_files = extract_all_natives(version, &libs_dir, &natives_dir)?;

        // 删除多余文件 (PCL: 不在列表中的旧文件)
        cleanup_orphaned_files(&natives_dir, &existing_files)?;

        eprintln!("[launch] Natives 解压完成: {} 个文件", existing_files.len());
    } else {
        eprintln!("[launch] Natives 已存在，跳过解压");
    }

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

    for lib in libs {
        let name = lib["name"].as_str().unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 4 || !parts[3].contains("natives") {
            continue;
        }

        let (group, artifact, version_str) = (parts[0], parts[1], parts[2]);
        let natives_classifier = parts[3];

        // ${arch} 替换 (PCL-style)
        let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
        let classifier = natives_classifier.replace("${arch}", arch);

        let jar_path = libs_dir
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version_str)
            .join(format!("{}-{}-{}.jar", artifact, version_str, classifier));

        if !jar_path.exists() {
            eprintln!("[launch] Natives JAR 不存在: {}", jar_path.display());
            continue;
        }

        // 解压 JAR 中的 DLL
        match std::fs::File::open(&jar_path) {
            Ok(file) => {
                match zip::ZipArchive::new(file) {
                    Ok(mut archive) => {
                        for i in 0..archive.len() {
                            if let Ok(mut entry) = archive.by_index(i) {
                                let entry_name = entry.name().to_string();
                                // 只提取 DLL (Windows)
                                if !entry_name.ends_with(".dll") && !entry_name.ends_with(".so") && !entry_name.ends_with(".dylib") {
                                    continue;
                                }

                                let dest_path = natives_dir.join(
                                    Path::new(&entry_name).file_name().unwrap_or_default()
                                );

                                // PCL: 如果文件已存在且大小相同则跳过
                                if dest_path.exists() {
                                    if let Ok(meta) = std::fs::metadata(&dest_path) {
                                        if meta.len() == entry.size() {
                                            existing_files.push(dest_path);
                                            continue;
                                        }
                                    }
                                    // 大小不同，删除旧文件
                                    let _ = std::fs::remove_file(&dest_path);
                                }

                                let mut out = std::fs::File::create(&dest_path)
                                    .map_err(|e| format!("创建文件失败 {}: {}", dest_path.display(), e))?;
                                std::io::copy(&mut entry, &mut out)
                                    .map_err(|e| format!("解压失败: {}", e))?;
                                existing_files.push(dest_path);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[launch] 无法打开 Natives JAR (可能已损坏): {} ({})", jar_path.display(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("[launch] 无法读取 Natives JAR: {} ({})", jar_path.display(), e);
            }
        }
    }

    Ok(existing_files)
}

/// 删除 natives 目录中的孤立文件 (PCL-style cleanup)
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

/// 清空目录内容 (HMCL-style: cleanDirectoryQuietly)
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

/// 检查路径是否全部为 ASCII 字符
fn is_ascii_path(p: &Path) -> bool {
    p.to_string_lossy().chars().all(|c| c.is_ascii())
}

// ── 参数构建 ────────────────────────────────────────────────

/// 构建完整的 JVM 和 Game 参数
///
/// 参照 HMCL DefaultLauncher.generateCommandLine + PCL McLaunchArgumentsJVM/Game:
/// 优先级顺序:
///   1. Java executable
///   2. User override JVM args (最高优先级)
///   3. Memory (-Xmx/-Xms)
///   4. User custom JVM args
///   5. Generated JVM args (GC, encoding, security, log4j)
///   6. Version JSON JVM args (with rule checking)
///   7. Auth arguments
///   8. Classpath
///   9. Main class
///  10. Game arguments (with rule checking)
///  11. User custom game args
fn build_arguments(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> Result<(String, String, Vec<String>), String> {
    // ── 解析 Java 版本 ──────────────────────────
    let java_version = detect_java_version(&ctx.java_path)?;

    // ── 构建替换映射 ────────────────────────────
    let (placeholders, cp_entries) = build_placeholders(ctx, version, natives_dir);

    let resolve = |s: &str| -> String {
        let mut result = s.to_string();
        for (key, val) in &placeholders {
            result = result.replace(key.as_str(), val.as_str());
        }
        result
    };

    // ── 1. Java executable ─────────────────────
    // PCL: 总是使用 java.exe (不用 javaw.exe，否则 #6263 崩溃)
    let java_exe = ctx.java_path
        .to_string_lossy()
        .replace("javaw.exe", "java.exe")
        .replace("javaw", "java");

    // ── 构建 JVM 参数 ──────────────────────────
    let mut jvm_args: Vec<String> = Vec::new();

    // ── 2. User override JVM args (最高优先级) ──
    // 这些是用户在"设置→高级JVM参数"中配置的
    if !ctx.jvm_args.is_empty() {
        jvm_args.extend(split_args(&ctx.jvm_args));
    }

    // ── 3. Memory ──────────────────────────────
    jvm_args.push(format!("-Xmx{}m", ctx.max_memory));
    if let Some(min) = ctx.min_memory {
        jvm_args.push(format!("-Xms{}m", min));
    }

    // ── 4. Metaspace (HMCL-style) ──────────────
    if java_version >= 9 {
        jvm_args.push("-XX:MetaspaceSize=256m".to_string());
    } else if java_version >= 7 {
        jvm_args.push("-XX:PermSize=256m".to_string());
    }

    // ── 5. GC 优化 (HMCL/PCL-style) ────────────
    if ctx.use_gc_optimize
        && jvm_args.iter().all(|a| !a.starts_with("-XX:+Use") || !a.ends_with("GC"))
    {
        // PCL-style GC 管理
        if java_version >= 21 {
            // Java 21+: 分代 ZGC
            jvm_args.push("-XX:+UseZGC".to_string());
        } else if java_version >= 15 {
            // Java 15-20: ZGC
            jvm_args.push("-XX:+UseZGC".to_string());
        } else if java_version >= 8 {
            // Java 8-14: G1GC
            jvm_args.push("-XX:+UseG1GC".to_string());
            jvm_args.push("-XX:G1NewSizePercent=20".to_string());
            jvm_args.push("-XX:G1ReservePercent=20".to_string());
            jvm_args.push("-XX:G1HeapRegionSize=32M".to_string());
            jvm_args.push("-XX:MaxGCPauseMillis=50".to_string());
        }

        jvm_args.push("-XX:+UnlockExperimentalVMOptions".to_string());
        jvm_args.push("-XX:+UnlockDiagnosticVMOptions".to_string());

        // Java 24+: CompactObjectHeaders (PCL-style)
        if java_version >= 24 {
            jvm_args.push("-XX:+UseCompactObjectHeaders".to_string());
        }

        // PCL: 32-bit JVM 栈大小修正
        if !is_64bit_java(&ctx.java_path) {
            jvm_args.push("-Xss1m".to_string());
        }
    }

    // ── 6. OmitStackTraceInFastThrow (PCL-style) ──
    jvm_args.push("-XX:-OmitStackTraceInFastThrow".to_string());

    // ── 7. 编码 (HMCL/PCL-style) ────────────────
    if java_version < 19 {
        jvm_args.push("-Dsun.stdout.encoding=UTF-8".to_string());
        jvm_args.push("-Dsun.stderr.encoding=UTF-8".to_string());
    } else {
        jvm_args.push("-Dstdout.encoding=UTF-8".to_string());
        jvm_args.push("-Dstderr.encoding=UTF-8".to_string());
    }

    // PCL-style: file.encoding = COMPAT for Java 18+
    if java_version >= 18 {
        let has_file_encoding = jvm_args.iter().any(|a| a.starts_with("-Dfile.encoding="));
        if !has_file_encoding {
            jvm_args.push("-Dfile.encoding=COMPAT".to_string());
        }
    }

    // ── 8. Log4j RCE 漏洞防御 (HMCL/PCL-style) ──
    jvm_args.push("-Djava.rmi.server.useCodebaseOnly=true".to_string());
    jvm_args.push("-Dcom.sun.jndi.rmi.object.trustURLCodebase=false".to_string());
    jvm_args.push("-Dcom.sun.jndi.cosnaming.object.trustURLCodebase=false".to_string());
    jvm_args.push("-Dlog4j2.formatMsgNoLookups=true".to_string());

    // ── 9. 安全/兼容参数 ────────────────────────
    jvm_args.push("-Djdk.lang.Process.allowAmbiguousCommands=True".to_string());
    jvm_args.push("-Dfml.ignoreInvalidMinecraftCertificates=True".to_string());
    jvm_args.push("-Dfml.ignorePatchDiscrepancies=True".to_string());

    // ──10. 启动器品牌 ──────────────────────────
    jvm_args.push("-Dminecraft.launcher.brand=TeasLauncher".to_string());
    jvm_args.push(format!("-Dminecraft.launcher.version={}", BUILD));
    jvm_args.push(format!("-Dminecraft.client.jar={}", resolve("${primary_jar}")));

    // ──11. Java 16 illegal-access ──────────────
    if java_version == 16 {
        jvm_args.push("--illegal-access=permit".to_string());
    }

    // ──12. Java 24/25 sun.misc.unsafe (HMCL-style) ──
    if java_version == 24 || java_version == 25 {
        jvm_args.push("--sun-misc-unsafe-memory-access=allow".to_string());
    }

    // ──13. Version JSON JVM args (with rule checking) ──
    // HMCL-style: parse version JSON arguments.jvm
    if let Some(jvm_list) = version["arguments"]["jvm"].as_array() {
        for arg in jvm_list {
            if let Some(s) = arg.as_str() {
                // HMCL: 跳过 version JSON 中已有的内存和 natives 参数
                if s.starts_with("-Xmx") || s.starts_with("-Xms")
                    || s.starts_with("-Djava.library.path=")
                {
                    continue;
                }
                jvm_args.push(resolve(s));
            } else if let Some(obj) = arg.as_object() {
                if !check_os_rules(obj) { continue; }
                if let Some(val) = obj.get("value") {
                    if let Some(s) = val.as_str() {
                        jvm_args.push(resolve(s));
                    } else if let Some(arr) = val.as_array() {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                jvm_args.push(resolve(s));
                            }
                        }
                    }
                }
            }
        }
    } else {
        // 旧版: 添加 natives 和 classpath
        jvm_args.push(format!("-Djava.library.path={}", natives_dir.display()));
    }

    // ──14. Natives 系统属性 ─────────────────────
    // PCL-style: 确保 natives 路径在所有相关属性中设置
    let natives_str = natives_dir.display().to_string();
    if !jvm_args.iter().any(|a| a.starts_with("-Djava.library.path=")) {
        jvm_args.push(format!("-Djava.library.path={}", natives_str));
    }
    // HMCL: 只在 Windows 下添加 jna.tmpdir 和 io.netty.native.workdir
    jvm_args.push(format!("-Djna.tmpdir={}", natives_str));
    jvm_args.push(format!("-Dorg.lwjgl.system.SharedLibraryExtractPath={}", natives_str));
    jvm_args.push(format!("-Dio.netty.native.workdir={}", natives_str));

    // ──15. HeapDumpPath (PCL-style) ────────────
    jvm_args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".to_string());

    // ──16. Classpath ────────────────────────────
    // HMCL/PCL-style: -cp 必须在 version JSON JVM args 之后重新声明
    // 确保 classpath 优先级最高
    jvm_args.push("-cp".to_string());
    jvm_args.push(cp_entries.join(";"));

    // ──17. Java agents ─────────────────────────
    for agent in &ctx.java_agents {
        jvm_args.push(format!("-javaagent:{}", agent));
    }

    // ──18. Main class ──────────────────────────
    let main_class = version["mainClass"]
        .as_str()
        .unwrap_or("net.minecraft.client.main.Main");
    jvm_args.push(main_class.to_string());

    // ── 构建 Game 参数 ─────────────────────────
    let mut game_args_vec: Vec<String> = Vec::new();

    if let Some(game_list) = version["arguments"]["game"].as_array() {
        for arg in game_list {
            if let Some(s) = arg.as_str() {
                // PCL: 保留 --demo 和 --quickPlay 参数，只过滤有问题的占位符
                if s.contains("${quickPlay") || s.contains("${QuickPlay") {
                    continue; // 跳过需要 quickPlay 值的占位符（我们没有 quickPlay 数据）
                }
                game_args_vec.push(resolve(s));
            } else if let Some(obj) = arg.as_object() {
                if !check_os_rules(obj) { continue; }
                if let Some(val) = obj.get("value") {
                    if let Some(s) = val.as_str() {
                        game_args_vec.push(resolve(s));
                    } else if let Some(arr) = val.as_array() {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                game_args_vec.push(resolve(s));
                            }
                        }
                    }
                }
            }
        }
    } else if let Some(mc_args) = version["minecraftArguments"].as_str() {
        // 旧版参数格式
        game_args_vec.extend(
            resolve(mc_args).split_whitespace().map(String::from)
        );
    }

    // ── User custom game args ──────────────────
    if !ctx.game_args.is_empty() {
        game_args_vec.extend(split_args(&ctx.game_args));
    }

    // ── 对 game args 中可能含空格的参数加引号 (PCL-style) ──
    let game_args_str = quote_args_if_needed(&game_args_vec).join(" ");

    // JVM 参数序列化为命令行字符串
    let jvm_args_str = quote_args_if_needed(&jvm_args).join(" ");

    Ok((format!("{} {}", java_exe, jvm_args_str), game_args_str, cp_entries))
}

// ── 占位符映射 ───────────────────────────────────────────────

/// 构建占位符替换映射 (HMCL DefaultLauncher.getConfigurations + PCL McLaunchArgumentsReplace)
fn build_placeholders(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> (Vec<(String, String)>, Vec<String>) {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let assets_dir = ctx.dot_minecraft.join("assets");
    let game_dir = ctx.dot_minecraft.clone(); // 默认 gameDir 就是 .minecraft
    let asset_index = version["assets"].as_str().unwrap_or("legacy");

    let uuid = offline_uuid(&ctx.player_name);
    let launcher_ver = format!("v{}.{:04}", env!("CARGO_PKG_VERSION"), BUILD);

    // ── 构建 classpath (PCL-style: 含正确排序) ──
    let cp_entries = build_classpath(ctx, version);

    let mut map: Vec<(String, String)> = Vec::new();

    // Mojang 官方占位符
    map.push(("${auth_player_name}".into(), ctx.player_name.clone()));
    map.push(("${auth_uuid}".into(), uuid));
    map.push(("${auth_access_token}".into(), "0".to_string()));
    map.push(("${auth_session}".into(), "0".to_string()));
    map.push(("${auth_xuid}".into(), "0".to_string()));
    map.push(("${clientid}".into(), "0".to_string()));
    map.push(("${user_type}".into(), "mojang".to_string()));
    map.push(("${user_properties}".into(), "{}".to_string()));
    map.push(("${version_name}".into(), ctx.instance_name.clone()));
    map.push(("${version_type}".into(), version["type"].as_str().unwrap_or("release").to_string()));
    map.push(("${assets_index_name}".into(), asset_index.to_string()));
    map.push(("${assets_root}".into(), assets_dir.display().to_string()));
    map.push(("${game_assets}".into(), assets_dir.display().to_string()));
    map.push(("${game_directory}".into(), game_dir.display().to_string()));
    map.push(("${resolution_width}".into(), ctx.window_width.to_string()));
    map.push(("${resolution_height}".into(), ctx.window_height.to_string()));
    map.push(("${library_directory}".into(), libs_dir.display().to_string()));
    map.push(("${libraries_directory}".into(), libs_dir.display().to_string()));
    map.push(("${classpath_separator}".into(), ";".to_string()));
    map.push(("${file_separator}".into(), "\\".to_string()));
    map.push(("${natives_directory}".into(), natives_dir.display().to_string()));
    map.push(("${launcher_name}".into(), "Teas Launcher".to_string()));
    map.push(("${launcher_version}".into(), launcher_ver));
    map.push(("${profile_name}".into(), "TeasLauncher".to_string()));
    map.push(("${language}".into(), "zh_cn".to_string()));
    map.push(("${path}".into(), game_dir.display().to_string()));

    // 主 JAR
    let jar_path = version_dir.join(format!("{}.jar", ctx.instance_name));
    map.push(("${primary_jar}".into(), jar_path.display().to_string()));
    map.push(("${primary_jar_name}".into(), format!("{}.jar", ctx.instance_name)));

    // QuickPlay 占位符 (无数据时设空)
    map.push(("${quickPlayPath}".into(), String::new()));
    map.push(("${quickPlaySingleplayer}".into(), String::new()));
    map.push(("${quickPlayMultiplayer}".into(), String::new()));
    map.push(("${quickPlayRealms}".into(), String::new()));

    (map, cp_entries)
}

/// 构建 classpath 条目列表 (PCL-style: 正确排序)
///
/// 参照 PCL McLaunchArgumentsReplace:
/// - 从 mclib_list_get 获取库列表
/// - 过滤 natives
/// - OptiFine 放到倒数第二位
/// - 主 JAR 放最后
fn build_classpath(ctx: &LaunchContext, version: &serde_json::Value) -> Vec<String> {
    let mut cp: Vec<String> = Vec::new();
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let mut optifine_cp: Option<String> = None;

    if let Some(libs) = version["libraries"].as_array() {
        for lib in libs {
            let name = lib["name"].as_str().unwrap_or("");
            let parts: Vec<&str> = name.split(':').collect();
            if parts.len() < 3 { continue; }

            // 跳过 natives
            if lib.get("natives").is_some() { continue; }

            let (group, artifact, ver) = (parts[0], parts[1], parts[2]);
            let jar_path = libs_dir
                .join(group.replace('.', "/"))
                .join(artifact)
                .join(ver)
                .join(format!("{}-{}.jar", artifact, ver));

            if jar_path.exists() && is_jar_valid(&jar_path) {
                // PCL: OptiFine 需要放到倒数第二位
                if name.starts_with("optifine:OptiFine") || name.starts_with("optifine:optifine") {
                    optifine_cp = Some(jar_path.display().to_string());
                } else {
                    cp.push(jar_path.display().to_string());
                }
            }
        }
    }

    // OptiFine 放到倒数第二位 (PCL-style)
    if let Some(optifine) = optifine_cp {
        if cp.len() >= 2 {
            cp.insert(cp.len() - 1, optifine);
        } else {
            cp.push(optifine);
        }
    }

    // 主 JAR 放最后 (HMCL-style)
    let ver_jar = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join(format!("{}.jar", ctx.instance_name));
    if ver_jar.exists() && is_jar_valid(&ver_jar) {
        cp.push(ver_jar.display().to_string());
    }

    cp
}

// ── 预启动处理 ──────────────────────────────────────────────

/// PCL-style 预启动处理:
/// - 提取 log4j2.xml (HMCL-style)
/// - 更新 options.txt 语言设置
/// - 设置 GPU 偏好 (Windows)
fn pre_run_setup(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<(), String> {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);

    // ── 提取 log4j2.xml (HMCL-style) ──────────
    // 仅对 >= 1.7 版本提取
    let game_version = guess_game_version(version);
    if let Some(ref gv) = game_version {
        if compare_versions(gv, "1.7") >= std::cmp::Ordering::Equal {
            extract_log4j_config(ctx, version, &version_dir)?;
        }
    }

    // ── 更新 options.txt 语言 ─────────────────
    update_options_txt(ctx, version);

    Ok(())
}

/// HMCL-style: 提取 log4j2.xml 到版本目录
fn extract_log4j_config(
    _ctx: &LaunchContext,
    version: &serde_json::Value,
    version_dir: &Path,
) -> Result<(), String> {
    let target = version_dir.join("log4j2.xml");

    // 如果已存在就不重复写入
    if target.exists() {
        return Ok(());
    }

    let game_version = guess_game_version(version).unwrap_or_default();
    let is_legacy = compare_versions(&game_version, "1.12") == std::cmp::Ordering::Less;

    // 写入 log4j2.xml 配置 (内嵌 HMCL 同款配置)
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
fn update_options_txt(ctx: &LaunchContext, _version: &serde_json::Value) {
    let options_path = ctx.dot_minecraft.join("options.txt");

    // 简单处理: 如果 options.txt 不存在，不做任何事
    // (PCL 会在这里处理语言编码兼容问题)
    if !options_path.exists() {
        return;
    }

    // 读取并确保语言设置正确 (PCL: zh_cn vs zh_CN)
    if let Ok(content) = std::fs::read_to_string(&options_path) {
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let mut changed = false;
        for line in &mut lines {
            if line.starts_with("lang:") {
                // PCL: 1.6-1.10 需要大写后缀 zh_CN，1.11+ 需要小写 zh_cn
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

// ── 进程启动 ────────────────────────────────────────────────

/// HMCL/PCL-style: 直接使用 ProcessBuilder 启动进程
///
/// HMCL: ProcessBuilder with directory + inheritIO
/// PCL:  ProcessStartInfo with RedirectStandardOutput + RedirectStandardError
/// Prism: QProcess with detachable + environment
fn spawn_process(
    ctx: &LaunchContext,
    java_cmd: &str,
    game_args: &str,
) -> Result<std::process::Child, String> {
    // 解析命令行参数
    let all_args = format!("{} {}", java_cmd, game_args);
    let parsed = split_args(&all_args);

    if parsed.is_empty() {
        return Err("启动参数为空".to_string());
    }

    let program = &parsed[0];
    let args: Vec<&str> = parsed[1..].iter().map(|s| s.as_str()).collect();

    eprintln!("[launch] =================== 启动命令行 ===================");
    eprintln!("[launch] 程序: {}", program);
    for (i, a) in args.iter().enumerate() {
        if a.len() > 300 {
            eprintln!("[launch]   [{}] {}...", i, &a[..300]);
        } else {
            eprintln!("[launch]   [{}] {}", i, a);
        }
    }
    eprintln!("[launch] ===================================================");

    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let game_dir = &ctx.dot_minecraft;

    let mut cmd = std::process::Command::new(program);
    cmd.args(&args);
    cmd.current_dir(game_dir);

    // ── 环境变量 (HMCL/PCL-style) ──────────────
    // PCL: APPDATA 指向 .minecraft 父目录
    cmd.env("APPDATA", game_dir
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| game_dir.display().to_string()));

    // PCL: Path 扩展，包含 Java bin 目录
    if let Some(java_bin) = ctx.java_path.parent() {
        let existing_path = std::env::var("Path").unwrap_or_default();
        cmd.env("Path", format!("{};{}", java_bin.display(), existing_path));
    }

    // HMCL-style: 实例环境变量
    cmd.env("INST_NAME", &ctx.instance_name);
    cmd.env("INST_ID", &ctx.instance_name);
    cmd.env("INST_DIR", version_dir.display().to_string());
    cmd.env("INST_MC_DIR", game_dir.display().to_string());
    cmd.env("INST_JAVA", ctx.java_path.display().to_string());

    // 用户自定义环境变量
    for (key, val) in &ctx.env_vars {
        cmd.env(key, val);
    }

    // ── 重定向输出 (HMCL/PCL-style) ────────────
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());

    // ── 不显示控制台窗口 (Windows) ────────────
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn().map_err(|e| {
        format!(
            "启动 Minecraft 失败: {}\nJava 路径: {}\n请检查 Java 是否正确安装。",
            e, ctx.java_path.display()
        )
    })?;

    Ok(child)
}

// ── 进程监控 ────────────────────────────────────────────────

/// HMCL-style: 启动 stdout/stderr 流监控 + 退出等待
///
/// 参照 HMCL DefaultLauncher.startMonitors:
/// - stdout/stderr pump threads
/// - ExitWaiter thread
/// - Post-exit command support
fn monitor_process(
    app: tauri::AppHandle,
    mut child: std::process::Child,
    ctx: LaunchContext,
) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // HMCL-style: stdout pump thread
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

    // stderr pump thread
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

    // ExitWaiter (HMCL-style)
    let app_clone = app;
    let instance_name = ctx.instance_name.clone();
    let mc_dir_str = ctx.mc_dir.display().to_string();

    tokio::spawn(async move {
        let status = child.wait();
        let exit_code = status.as_ref().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

        // 清除 PID
        *RUNNING_PID.lock().unwrap() = None;

        eprintln!("[launch] Minecraft 进程已退出，退出码: {}", exit_code);

        // PCL-style: 输出启动日志摘要
        let success = status.map_or(false, |s| s.success());

        if success {
            eprintln!("[launch] {} 正常退出", instance_name);
            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "crashed": false,
            }));
        } else {
            eprintln!("[launch] {} 异常退出 ({}), 开始崩溃检测...", instance_name, exit_code);

            // 延迟一点，等待日志文件写完
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            // 触发崩溃检测
            let crash_result = crate::crash::check_crash(
                mc_dir_str.clone(),
                instance_name.clone(),
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

/// 检测 Java 主版本号
fn detect_java_version(java_path: &Path) -> Result<u32, String> {
    let output = std::process::Command::new(java_path)
        .args(["-XshowSettings:all", "-version"])
        .output()
        .map_err(|_| "无法执行 Java 版本检测".to_string())?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    // 尝试多种模式 (HMCL-style)
    for line in combined.lines() {
        // "java version "1.8.0_371"" → 8
        // "openjdk version "17.0.9" 2023-10-17" → 17
        if let Some(ver_str) = line.split("version").nth(1) {
            let ver_str = ver_str.trim().trim_matches('"');
            if let Some(dot_pos) = ver_str.find('.') {
                let major = if ver_str.starts_with("1.") {
                    // Java 1.8 → 8
                    ver_str[2..dot_pos].parse().unwrap_or(8)
                } else {
                    ver_str[..dot_pos].parse().unwrap_or(8)
                };
                return Ok(major);
            }
        }
    }

    // 回退: -version 输出
    for line in combined.lines() {
        if line.contains("Runtime") || line.contains("64-Bit") || line.contains("32-Bit") {
            // 从 "64-Bit Server VM" 等推断不出来版本号，使用默认值
        }
    }

    Ok(8) // 默认: Java 8
}

/// 检测 Java 是否为 64-bit
fn is_64bit_java(java_path: &Path) -> bool {
    if let Ok(output) = std::process::Command::new(java_path)
        .args(["-version"])
        .output()
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        format!("{}\n{}", stderr, stdout).contains("64-Bit")
    } else {
        cfg!(target_arch = "x86_64") // 默认跟随系统
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
            }

            // features 检查
            if matches {
                if let Some(features) = rule.get("features") {
                    // has_custom_resolution: 我们有分辨率设置
                    if features.get("has_custom_resolution").is_some() { matches = true; }
                    // is_demo_user: 我们不是
                    if features.get("is_demo_user").is_some() { matches = false; }
                    // quick_play 相关: 我们不使用
                    if let Some(obj) = features.as_object() {
                        for key in obj.keys() {
                            if key.contains("quick_play") { matches = false; }
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

/// 分割 Java 参数字符串 (PCL-style SplitJavaArguments)
///
/// 正确处理:
/// - 引号内的空格不分割
/// - 转义引号 \"
/// - 多个连续空格
fn split_args(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
            // 转义引号
            current.push('\\');
            current.push('"');
            i += 1;
        } else if c == '"' {
            in_quotes = !in_quotes;
            current.push(c);
        } else if c == ' ' && !in_quotes {
            if !current.is_empty() {
                result.push(current);
                current = String::new();
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.is_empty() {
        result.push(current);
    }

    // 清理每个参数的首尾引号
    result.iter().map(|a| {
        let trimmed = a.trim();
        if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
            trimmed[1..trimmed.len() - 1].to_string()
        } else {
            trimmed.to_string()
        }
    }).collect()
}

/// PCL-style: 对含特殊字符的参数加双引号
fn quote_args_if_needed(args: &[String]) -> Vec<String> {
    args.iter().map(|a| {
        if a.contains(' ') || a.contains('&') || a.contains('|') || a.contains('<')
            || a.contains('>') || a.contains('^')
        {
            if !a.starts_with('"') && !a.ends_with('"') {
                format!("\"{}\"", a.replace('"', "\\\""))
            } else {
                a.clone()
            }
        } else {
            a.clone()
        }
    }).collect()
}

/// 从 version JSON 猜测 Minecraft 游戏版本号
fn guess_game_version(version: &serde_json::Value) -> Option<String> {
    // 方式1: inheritsFrom 通常是 MC 版本号
    if let Some(parent) = version["inheritsFrom"].as_str() {
        return Some(parent.to_string());
    }
    // 方式2: id 字段 (原版就是版本号)
    if let Some(id) = version["id"].as_str() {
        // 如果 id 不包含加载器关键字，可能就是原版版本号
        if !id.contains("forge") && !id.contains("fabric") && !id.contains("quilt") && !id.contains("neoforge") {
            return Some(id.to_string());
        }
    }
    // 方式3: clientVersion 或 releaseTime
    if let Some(cv) = version["clientVersion"].as_str() {
        return Some(cv.to_string());
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
