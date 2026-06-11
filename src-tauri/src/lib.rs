//! Teas Launcher — Rust 后端
//!
//! 模块划分:
//! - utils:      JAR 校验、文件操作、UUID 生成、Natives 提取、OS 过滤
//! - http:       HTTP 客户端、代理、取消下载
//! - config:     配置读写 (tc.ini)、.teas 目录
//! - download:   下载引擎 (model, source, engine, tracker) — PCL-CE 移植
//! - version:    版本管理 (library, assets, manifest) — PCL-CE 移植
//! - install:    安装引擎 (vanilla, forge, neoforge, fabric, ...) — PCL-CE 移植
//! - instance:   实例管理 (list, delete, resolve_version)
//! - launch:     Minecraft 启动流程
//! - crash:      崩溃检测
//! - system:     系统命令 (版本号、状态、网络、Java 扫描、代理)
//! - modrinth:   Modrinth API (搜索、详情、下载)
//! - modpack:    整合包安装

mod utils;
mod http;
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

pub(crate) const BUILD: u32 = 75;

pub fn run() {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let _ = config::teas_dir();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(std::sync::Mutex::new(sys))
        .invoke_handler(tauri::generate_handler![
            // ── 实例管理 ──
            instance::fix_instance,
            instance::list_instances,
            instance::delete_instance,
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
            modrinth::install_project,
            modrinth::install_file,
            // ── CurseForge API ──
            curseforge::search_curseforge,
            curseforge::get_curseforge_project,
            curseforge::get_curseforge_files,
            curseforge::install_curseforge_file,
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
            system::get_memory_info,
            system::calc_auto_memory,
            // ── 版本清单获取 ──
            version::manifest::fetch_version_manifest,
            version::manifest::fetch_forge_mc_versions,
            version::manifest::fetch_forge_versions,
            version::manifest::fetch_neoforge_versions,
            version::manifest::fetch_fabric_versions,
            version::manifest::fetch_quilt_versions,
            version::manifest::fetch_optifine_versions,
            version::manifest::fetch_liteloader_versions,
            version::manifest::fetch_cleanroom_versions,
            version::manifest::fetch_legacyfabric_versions,
            version::manifest::fetch_labymod_versions,
            // ── 配置 ──
            config::config_read,
            config::config_write,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Teas Launcher 失败");
}
