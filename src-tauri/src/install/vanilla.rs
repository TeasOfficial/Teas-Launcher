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
use std::path::Path;
use tauri::Emitter;

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
    let base = Path::new(&mc_dir);
    let mc_path = if base.join(".minecraft").exists() { base.join(".minecraft") } else { base.to_path_buf() };
    let target = mc_path.join("versions").join(&instance_name);
    if target.exists() {
        return Err(format!("实例 {} 已存在", instance_name));
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    let prefer_official = source == "Mojang";
    let client = crate::http::build_http_client(std::time::Duration::from_secs(600))?;

    app.emit("download-progress", serde_json::json!({
        "filename": instance_name, "downloaded": 0, "total": 100, "percent": 0,
        "step": "准备安装原版 Minecraft..."
    })).ok();

    // Phase 1: 下载版本 JSON (进度 0-10%)
    let _json_url = format!(
        "https://piston-meta.mojang.com/v1/packages/{}/manifest?version={}",
        mc_version, mc_version
    );
    // 从版本清单获取精确 JSON URL
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let exact_url = manifest["versions"]
        .as_array()
        .ok_or("无法解析版本清单")?
        .iter()
        .find(|v| v["id"].as_str() == Some(&mc_version))
        .and_then(|v| v["url"].as_str())
        .ok_or(format!("找不到版本 {}", mc_version))?;

    let json_path = target.join(format!("{}.json", &instance_name));
    let json_urls = source_launcher_or_meta(exact_url, prefer_official);

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
    app.emit("download-progress", serde_json::json!({
        "filename": instance_name, "downloaded": 10, "total": 100, "percent": 10,
        "step": "版本 JSON 已下载"
    })).ok();

    // Phase 2: 下载库文件 (进度 10-50%)
    let json_content = std::fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    let version_json: serde_json::Value =
        serde_json::from_str(&json_content).map_err(|e| e.to_string())?;

    let mut lib_files = mclib_from_instance(&version_json, &mc_path, prefer_official);

    if !lib_files.is_empty() {
        app.emit("download-progress", serde_json::json!({
            "filename": instance_name, "downloaded": 10, "total": 100, "percent": 10,
            "step": format!("下载 {} 个库文件...", lib_files.len())
        })).ok();

        download_files_parallel(&app, &mut lib_files, 8).await;

        app.emit("download-progress", serde_json::json!({
            "filename": instance_name, "downloaded": 50, "total": 100, "percent": 50,
            "step": "库文件下载完成"
        })).ok();
    }

    // Phase 3: 下载资源索引 (进度 50-55%)
    if let Some(mut idx_dl) = download_asset_index(&version_json, &mc_path, prefer_official) {
        download_file(&app, &mut idx_dl, false).await?;
    }
    app.emit("download-progress", serde_json::json!({
        "filename": instance_name, "downloaded": 55, "total": 100, "percent": 55,
        "step": "资源索引已下载"
    })).ok();

    // Phase 4: 下载资源文件 (进度 55-95%)
    let index_name = crate::version::assets::mcassets_get_index_name(&version_json);
    let asset_files =
        crate::version::assets::mcassets_fix_list(&mc_path, &index_name, true, prefer_official)
            .unwrap_or_default();

    if !asset_files.is_empty() {
        app.emit("download-progress", serde_json::json!({
            "filename": instance_name, "downloaded": 55, "total": 100, "percent": 55,
            "step": format!("下载 {} 个资源文件...", asset_files.len())
        })).ok();

        let mut files = asset_files;
        download_files_parallel(&app, &mut files, 8).await;

        app.emit("download-progress", serde_json::json!({
            "filename": instance_name, "downloaded": 95, "total": 100, "percent": 95,
            "step": "资源文件下载完成"
        })).ok();
    }

    // 写入 clientVersion
    if let Ok(mut vj) = serde_json::from_str::<serde_json::Value>(&json_content) {
        vj["clientVersion"] = serde_json::Value::String(mc_version.clone());
        let _ = std::fs::write(&json_path, serde_json::to_string_pretty(&vj).unwrap_or(json_content));
    }

    // 如果有 loader，执行 loader 安装
    if let Some(ref loader_id) = loader {
        let loader_ver = loader_version.as_deref().unwrap_or("");
        match loader_id.as_str() {
            "forge" => {
                crate::install::forge::install_forge_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver
                ).await?;
            }
            "neoforge" => {
                crate::install::neoforge::install_neoforge_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver
                ).await?;
            }
            "fabric" => {
                crate::install::fabric::install_fabric_loader(
                    &app, &mc_path, &instance_name, &mc_version, loader_ver
                ).await?;
            }
            _ => return Err(format!("不支持的加载器: {}", loader_id)),
        }
    }

    app.emit("download-progress", serde_json::json!({
        "filename": instance_name, "downloaded": 100, "total": 100, "percent": 100,
        "step": "安装完成!"
    })).ok();

    Ok(format!("{} 安装完成", instance_name))
}
