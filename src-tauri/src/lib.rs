//! Teas Launcher — Rust 后端
//!
//! 模块划分:
//! - utils:      JAR 校验、文件操作、UUID 生成、Natives 提取、OS 过滤
//! - http:       HTTP 客户端、代理、取消下载
//! - progress:   进度事件 (download-progress) 的统一发射入口
//! - config:     配置读写 (tc.ini)、.teas 目录
//! - download:   下载引擎 (model, source, engine) — PCL-CE 移植
//! - version:    版本管理 (library, assets, manifest) — PCL-CE 移植
//! - install:    安装引擎 (vanilla, forge, neoforge, fabric, quilt, cleanroom) — PCL-CE 移植
//! - instance:   实例管理 (list, delete, resolve_version)
//! - launch:     Minecraft 启动流程 (mod, natives, args, process, java)
//! - crash:      崩溃检测
//! - system:     系统命令 (版本号、状态、网络、Java 扫描、代理)
//! - modrinth:   Modrinth API (搜索、详情、下载)
//! - curseforge: CurseForge API (搜索、详情、文件、下载)
//! - modpack:    整合包安装
//!
//! 日志: 统一走 `log` 宏，由 tauri-plugin-log 写入 `.teas/launcher.log`。
//! Release 构建没有控制台，因此不要用 println!/eprintln! 输出诊断信息。

mod utils;
mod http;
mod progress;
mod config;
mod download;
mod version;
mod install;
mod instance;
mod launch;
mod crash;
mod system;
mod modrinth;
mod curseforge;
mod modpack;

pub(crate) const BUILD: u32 = 100;

/// 构建日志插件 — 写入 .teas/launcher.log，dev 下同时输出到控制台
///
/// 级别：dev 用 Debug（排查时正需要命令行/natives/log4j 这些细节），
/// release 用 Info。游戏自身的 stdout/stderr 统一打在 debug 级，
/// 否则一次启动就能往日志里灌近 3000 行，把启动器自己的诊断挤掉。
/// 目录是否可写（用探测文件判断，不依赖 ACL 读取）
///
/// 日志目录不可写不应该连累启动：以前直接注册 Folder target，插件初始化失败
/// 会让整个 `Builder::run()` panic（表现为"双击没反应"）。
fn is_dir_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(".teas-write-probe");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(e) => {
            // 把真实原因打出来：只报"不可写"会让排查停在猜测上
            eprintln!("[log] 写入探测失败: {} -> {:?}", probe.display(), e);
            false
        }
    }
}

fn build_logger<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let teas = config::teas_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    // 日志级别：用户设置优先（设置 → 高级 → 日志级别），没设过就按构建类型给默认值。
    // 这个值在启动时读取一次，改完需要重启才生效（UI 上有提示）。
    let default_level = if cfg!(dev) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    let level = config::config_read("user".to_string())
        .ok()
        .and_then(|c| c.get("log_level").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .and_then(|s| match s.trim().to_uppercase().as_str() {
            "TRACE" => Some(log::LevelFilter::Trace),
            "DEBUG" => Some(log::LevelFilter::Debug),
            "INFO" => Some(log::LevelFilter::Info),
            "WARN" | "WARNING" => Some(log::LevelFilter::Warn),
            "ERROR" => Some(log::LevelFilter::Error),
            _ => None,
        })
        .unwrap_or(default_level);

    let mut builder = tauri_plugin_log::Builder::new()
        // Builder::new() 自带 Stdout + LogDir 两个默认 target，不清掉的话：
        // dev 会把 Stdout 注册两遍（每行打印两次），release 会同时写
        // .teas/launcher.log 和系统日志目录里的另一个文件。
        .clear_targets()
        .level(level)
        // 默认是 UTC，日志时间会比本地时间差 8 小时，排查时对不上
        .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
        .max_file_size(5 * 1024 * 1024);

    // 目录不可写（ACL / 只读盘 / 被占用）时降级为只输出 stdout，
    // 而不是让整个启动流程 panic。这类问题必须能在 UI 里被发现，
    // 不能表现为"点了图标什么都没发生"。
    let file_target_ok = is_dir_writable(&teas);
    if file_target_ok {
        builder = builder.target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Folder {
                path: teas.clone(),
                file_name: Some("launcher".into()),
            },
        ));
    } else {
        eprintln!(
            "[log] 日志目录不可写，本次仅输出到控制台: {}",
            teas.display()
        );
    }

    // dev 下保留控制台输出；文件 target 降级时也必须有 stdout，否则一条日志都没了
    if cfg!(dev) || !file_target_ok {
        builder = builder.target(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Stdout,
        ));
    }

    builder.build()
}

/// 把 panic 写入日志
///
/// release 设了 `panic = "abort"` 且 Windows GUI 进程没有控制台，默认的 panic 输出
/// 会彻底丢失——用户只看到启动器突然消失。装一个 hook，至少留下可查的线索。
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "未知 panic".to_string());
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "未知位置".to_string());
        // 先写 stderr：panic 可能发生在日志插件初始化之前（那时 log::error! 会被丢弃，
        // 表现就是"进程直接消失、日志里什么都没有"）。GUI 子系统下 stderr 可被重定向捕获。
        eprintln!("[panic] {} @ {}", msg, loc);
        log::error!("[panic] {} @ {}", msg, loc);
    }));
}

pub fn run() {
    let _ = config::teas_dir();
    install_panic_hook();

    let state = system::SystemState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(build_logger())
        // 插件初始化之后再写，否则这几行会落在 logger 建立之前而被丢弃。
        // 有了这段，日志文件开头就能看出是哪次运行、什么环境。
        .setup(|_app| {
            log::info!(
                "[app] Teas Launcher v{} (build {}) — dev={} pid={}",
                env!("CARGO_PKG_VERSION"),
                BUILD,
                cfg!(dev),
                std::process::id()
            );
            match std::env::current_exe() {
                Ok(p) => log::info!("[app] exe: {}", p.display()),
                Err(e) => log::warn!("[app] 无法获取 exe 路径: {}", e),
            }
            match config::teas_dir() {
                Ok(d) => log::info!("[app] 数据目录: {}", d.display()),
                Err(e) => log::warn!("[app] 无法确定数据目录: {}", e),
            }
            Ok(())
        })
        .manage(std::sync::Mutex::new(state))
        .invoke_handler(tauri::generate_handler![
            // ── 实例管理 ──
            instance::fix_instance,
            instance::list_instances,
            instance::delete_instance,
            instance::open_instance_folder,
            instance::instance_config_read,
            instance::instance_config_write,
            // ── 启动 ──
            launch::launch_instance,
            launch::kill_instance,
            launch::is_instance_running,
            launch::get_launch_args,
            // ── 崩溃检测 ──
            crash::check_crash,
            // ── Modrinth API ──
            modrinth::search_bbsmc,
            modrinth::get_project,
            modrinth::get_project_versions,
            modrinth::install_file,
            // ── CurseForge API ──
            curseforge::search_curseforge,
            curseforge::get_curseforge_project,
            curseforge::get_curseforge_files,
            curseforge::install_curseforge_file,
            // ── CurseForge API Key（本地密钥，不回传） ──
            curseforge::curseforge_key_status,
            curseforge::curseforge_save_key,
            curseforge::curseforge_clear_key,
            curseforge::curseforge_test_key,
            // ── 整合包安装 ──
            modpack::install_modpack,
            modpack::install_local_modpack,
            // ── 游戏安装 (新) ──
            install::vanilla::install_game,
            // ── HTTP ──
            http::cancel_download,
            // ── 系统命令 ──
            system::get_version,
            system::get_full_version,
            system::is_dev,
            system::get_minecraft_dir,
            system::get_teas_dir,
            system::get_system_stats,
            system::check_network,
            system::test_proxy_connectivity,
            system::detect_system_proxy,
            system::scan_java,
            system::offline_uuid,
            system::get_memory_info,
            system::calc_auto_memory,
            // ── 版本清单获取 ──
            version::manifest::fetch_version_manifest,
            version::manifest::fetch_forge_mc_versions,
            version::manifest::fetch_forge_versions,
            version::manifest::fetch_neoforge_versions,
            version::manifest::fetch_fabric_versions,
            version::manifest::fetch_quilt_versions,
            version::manifest::fetch_cleanroom_versions,
            // ── 配置 ──
            config::config_read,
            config::config_write,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Teas Launcher 失败");
}
