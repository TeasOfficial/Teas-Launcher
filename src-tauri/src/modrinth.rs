//! Modrinth API：搜索、项目详情、版本列表、文件安装

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::http::{apply_source, build_http_client, is_cancelled, reset_cancel};
use std::time::Duration;

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
    // Modrinth 合法的 project_type 只有 mod / modpack / resourcepack / shader /
    // datapack / plugin。前端"地图"分类用的是 "map"，直接下发会恒定返回 0 条，
    // 这里显式映射到世界生成分类。
    match project_type.as_str() {
        "" => {}
        "map" => facets.push("[\"categories:worldgen\"]".to_string()),
        t => facets.push(format!("[\"project_type:{}\"]", t)),
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

    log::debug!("[search] GET {}", url);

    let resp = client.get(&url).send().await.map_err(|e| {
        log::error!("[search] 请求失败: {}", e);
        e.to_string()
    })?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        // 直接 parse body 会把 4xx/5xx 的 HTML 错误页当成 JSON 解析失败，
        // 报出难以理解的错误，这里先判状态码
        log::error!("[search] HTTP {} — {}", status.as_u16(), &text[..text.len().min(200)]);
        return Err(format!("搜索接口返回 HTTP {} ({})", status.as_u16(), source));
    }

    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        log::error!("[search] 响应解析失败: {}", e);
        format!("搜索响应解析失败: {}", e)
    })?;

    log::debug!(
        "[search] {} 命中 {} 条 ({} / {})",
        query, json["total_hits"].as_u64().unwrap_or(0),
        project_type, version
    );
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


/// 下载单个文件（带进度）到对应目录
#[tauri::command]
pub(crate) async fn install_file(
    app: tauri::AppHandle,
    mc_dir: String,
    url: String,
    filename: String,
    project_type: String,
    source: String,
    instance_name: String,
) -> Result<String, String> {
    let original_url = url.clone();
    let mirror_url = apply_source(&url, &source);
    reset_cancel();
    let dot_minecraft = crate::utils::dot_minecraft(&mc_dir);
    // ★ HMCL/PCL: 版本隔离下 gameDir = versions/<name>/
    // mods/resourcepacks/shaderpacks 必须放到 gameDir 下 MC 才能识别
    let game_dir = crate::utils::instance_game_dir(&dot_minecraft, &instance_name);
    let td = match project_type.as_str() {
        "mod" => game_dir.join("mods"),
        "shader" => game_dir.join("shaderpacks"),
        "resourcepack" => game_dir.join("resourcepacks"),
        "datapack" => game_dir.join("datapacks"),
        "map" => game_dir.join("saves"),
        _ => game_dir.to_path_buf(),
    };
    std::fs::create_dir_all(&td).map_err(|e| e.to_string())?;
    let dest = td.join(&filename);

    crate::progress::emit_stage(&app, &filename, 0, &format!("准备下载 {}", filename));

    if is_cancelled() {
        return Err("已取消".to_string());
    }

    let mut dl = DownloadFile::new(
        vec![mirror_url.clone(), original_url.clone()],
        dest.clone(),
        FileChecker::with_min_size(1),
    );
    download_file(&app, &mut dl, false).await?;

    crate::progress::emit_stage(&app, &filename, 100, &format!("已下载 {}", filename));

    Ok(format!("已安装到 {}", dest.display()))
}
