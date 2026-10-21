//! 原版 Minecraft 安装 — 参照 PCL-CE McDownloadClient / McDownloadClientLoader
//!
//! 安装流程:
//! 1. 下载版本 JSON → 保存到 versions/{name}/{name}.json
//! 2. 分析缺失库文件 (mclib_from_instance)
//! 3. 并行下载所有库文件
//! 4. 下载资源索引 JSON
//! 5. 分析并下载缺失资源文件
//! 6. 写入 clientVersion 字段

use crate::download::engine::{download_file, download_files_parallel};
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_launcher_or_meta;
use crate::version::assets::download_asset_index;
use crate::version::library::mclib_from_instance;

/// 完整安装原版 Minecraft
///
/// 参照 PCL-CE McInstallLoader + McDownloadClientLoader:
/// ```vb
/// ' 1) 下载版本 JSON
/// ' 2) 下载库文件
/// ' 3) 下载资源索引
/// ' 4) 下载资源文件
/// ```
#[tauri::command]
pub async fn install_game(
    app: tauri::AppHandle,
    mc_dir: String,
    instance_name: String,
    mc_version: String,
    loader: Option<String>,
    loader_version: Option<String>,
    source: String,
) -> Result<String, String> {
    let mc_path = crate::utils::dot_minecraft(&mc_dir);
    let target = mc_path.join("versions").join(&instance_name);
    if target.exists() {
        return Err(format!("实例 {} 已存在", instance_name));
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    let prefer_official = source == "Mojang";
    let client = crate::http::build_http_client(std::time::Duration::from_secs(600))?;

    crate::progress::emit_stage(&app, &instance_name, 0, "准备安装原版 Minecraft...");

    // Phase 1: 下载版本 JSON (进度 0-10%)
    let exact_url = crate::version::manifest::resolve_version_url(&client, &mc_version).await?;

    let json_path = target.join(format!("{}.json", &instance_name));
    let json_urls = source_launcher_or_meta(&exact_url, prefer_official);

    let mut json_dl = DownloadFile::new(
        json_urls,
        json_path.clone(),
        FileChecker {
            min_size: 500,
            is_json: true,
            ..Default::default()
        },
    );

    download_file(&app, &mut json_dl, false).await?;
    crate::progress::emit_stage(&app, &instance_name, 10, "版本 JSON 已下载");

    // Phase 2: 下载库文件 (进度 10-50%)
    let json_content = std::fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    let version_json: serde_json::Value =
        serde_json::from_str(&json_content).map_err(|e| e.to_string())?;

    let mut lib_files = mclib_from_instance(&version_json, &mc_path, prefer_official);

    if !lib_files.is_empty() {
        crate::progress::emit_stage(
            &app,
            &instance_name,
            10,
            &format!("下载 {} 个库文件...", lib_files.len()),
        );

        download_files_parallel(&app, &mut lib_files, 8).await;

        crate::progress::emit_stage(&app, &instance_name, 50, "库文件下载完成");
    }

    // Phase 3: 下载资源索引 (进度 50-55%)
    if let Some(mut idx_dl) = download_asset_index(&version_json, &mc_path, prefer_official) {
        download_file(&app, &mut idx_dl, false).await?;
    }
    crate::progress::emit_stage(&app, &instance_name, 55, "资源索引已下载");

    // Phase 4: 下载资源文件 (进度 55-95%)
    let index_name = crate::version::assets::mcassets_get_index_name(&version_json);
    let asset_files =
        crate::version::assets::mcassets_fix_list(&mc_path, &index_name, true, prefer_official)
            .unwrap_or_default();

    if !asset_files.is_empty() {
        crate::progress::emit_stage(
            &app,
            &instance_name,
            55,
            &format!("下载 {} 个资源文件...", asset_files.len()),
        );

        let mut files = asset_files;
        download_files_parallel(&app, &mut files, 8).await;

        crate::progress::emit_stage(&app, &instance_name, 95, "资源文件下载完成");
    }

    // 写入 clientVersion
    if let Ok(mut vj) = serde_json::from_str::<serde_json::Value>(&json_content) {
        vj["clientVersion"] = serde_json::Value::String(mc_version.clone());
        let _ = std::fs::write(&json_path, serde_json::to_string_pretty(&vj).unwrap_or(json_content));
    }

    // 如果有 loader，执行 loader 安装
    if let Some(ref loader_id) = loader {
        // 便于排查「明明选了官方源却走了镜像」：这里直接打出实际策略
        log::info!("[install] 加载器 {} 下载源: {}（prefer_official={}）",
            loader_id, if prefer_official { "官方源" } else { "BMCLAPI 镜像" }, prefer_official);
        let loader_ver = loader_version.as_deref().unwrap_or("");
        match loader_id.as_str() {
            "forge" => {
                crate::install::forge::install_forge_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver, prefer_official
                ).await?;
            }
            "neoforge" => {
                crate::install::neoforge::install_neoforge_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver, prefer_official
                ).await?;
            }
            "fabric" => {
                crate::install::fabric::install_fabric_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver
                ).await?;
            }
            "quilt" => {
                crate::install::quilt::install_quilt_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver
                ).await?;
            }
            "cleanroom" => {
                crate::install::cleanroom::install_cleanroom(
                    &app, &mc_path, &instance_name, loader_ver
                ).await?;
            }
            _ => return Err(format!("不支持的加载器: {}", loader_id)),
        }
    }

    crate::progress::emit_stage(&app, &instance_name, 100, "安装完成!");

    Ok(format!("{} 安装完成", instance_name))
}
