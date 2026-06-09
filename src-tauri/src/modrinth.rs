//! Modrinth API：搜索、项目详情、版本列表、文件安装

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::http::{apply_source, build_http_client, is_cancelled, reset_cancel};
use std::time::Duration;
use tauri::Emitter;

/// 搜索 Modrinth
#[tauri::command]
pub(crate) async fn search_bbsmc(
    query: String,
    project_type: String,
    version: String,
    sort: String,
    limit: u32,
    offset: u32,
    source: String,
) -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(10))?;

    let mut facets = Vec::new();
    if !project_type.is_empty() {
        facets.push(format!("[\"project_type:{}\"]", project_type));
    }
    if !version.is_empty() {
        facets.push(format!("[\"versions:{}\"]", version));
    }

    let facets_param = format!("[{}]", facets.join(","));
    let index = if sort.is_empty() {
        "relevance".to_string()
    } else {
        sort
    };

    let base_url = apply_source("https://api.modrinth.com/v2/", &source);
    let mut url = format!(
        "{}search?limit={}&offset={}&index={}&facets={}",
        base_url, limit, offset, index, facets_param
    );
    if !query.is_empty() {
        url.push_str(&format!("&query={}", urlencoding::encode(&query)));
    }

    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(json)
}

/// 获取项目详情
#[tauri::command]
pub(crate) async fn get_project(project_id: String) -> Result<serde_json::Value, String> {
    let c = build_http_client(Duration::from_secs(10))?;
    c.get(&format!("https://api.modrinth.com/v2/project/{}", project_id))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

/// 获取项目版本列表
#[tauri::command]
pub(crate) async fn get_project_versions(
    project_id: String,
) -> Result<serde_json::Value, String> {
    let c = build_http_client(Duration::from_secs(10))?;
    c.get(&format!(
        "https://api.modrinth.com/v2/project/{}/version",
        project_id
    ))
    .send()
    .await
    .map_err(|e| e.to_string())?
    .json()
    .await
    .map_err(|e| e.to_string())
}

/// 下载并安装项目文件（旧版简化接口）
#[tauri::command]
pub(crate) async fn install_project(
    mc_dir: String,
    project_id: String,
    project_type: String,
) -> Result<String, String> {
    let client = build_http_client(Duration::from_secs(30))?;

    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version",
        project_id
    );
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let versions: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let first = versions
        .as_array()
        .and_then(|a| a.first())
        .ok_or("没有可用的版本")?;

    let file = first["files"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or("没有可下载的文件")?;

    let file_url = file["url"].as_str().ok_or("文件 URL 不存在")?;
    let file_name = file["filename"].as_str().ok_or("文件名不存在")?;

    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let target_dir = match project_type.as_str() {
        "mod" => dot_minecraft.join("mods"),
        "shader" => dot_minecraft.join("shaderpacks"),
        "resourcepack" => dot_minecraft.join("resourcepacks"),
        "datapack" => dot_minecraft.join("datapacks"),
        "map" => dot_minecraft.join("saves"),
        _ => dot_minecraft.to_path_buf(),
    };
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let content = client
        .get(file_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let dest = target_dir.join(file_name);
    std::fs::write(&dest, content).map_err(|e| e.to_string())?;

    Ok(format!("已安装到 {}", dest.display()))
}

/// 下载单个文件（带进度）到对应目录
#[tauri::command]
pub(crate) async fn install_file(
    app: tauri::AppHandle,
    mc_dir: String,
    url: String,
    filename: String,
    project_type: String,
    source: String,
) -> Result<String, String> {
    let original_url = url.clone();
    let mirror_url = apply_source(&url, &source);
    reset_cancel();
    let base = std::path::Path::new(&mc_dir);
    let dm = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let td = match project_type.as_str() {
        "mod" => dm.join("mods"),
        "shader" => dm.join("shaderpacks"),
        "resourcepack" => dm.join("resourcepacks"),
        "datapack" => dm.join("datapacks"),
        "map" => dm.join("saves"),
        _ => dm.to_path_buf(),
    };
    std::fs::create_dir_all(&td).map_err(|e| e.to_string())?;
    let dest = td.join(&filename);

    app.emit(
        "download-progress",
        serde_json::json!({
            "filename": filename,
            "downloaded": 0u64,
            "total": 0u64,
            "percent": 0u32
        }),
    )
    .ok();

    if is_cancelled() {
        return Err("已取消".to_string());
    }

    let mut dl = DownloadFile::new(
        vec![mirror_url.clone(), original_url.clone()],
        dest.clone(),
        FileChecker::with_min_size(1),
    );
    download_file(&app, &mut dl, false).await?;
    let downloaded = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);

    app.emit(
        "download-progress",
        serde_json::json!({
            "filename": filename,
            "downloaded": downloaded,
            "total": downloaded,
            "percent": 100u32
        }),
    )
    .ok();

    Ok(format!("已安装到 {}", dest.display()))
}
