//! CurseForge API：搜索、项目详情、文件下载
//! 参照 PCL-CE ResourceProject/Curseforge 模块

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::http::build_http_client;
use std::time::Duration;
use tauri::Emitter;

/// PCL DlSourceModGet: URL 镜像转换
fn cf_mirror(url: &str) -> String {
    url.replace("api.curseforge.com", "mod.mcimirror.top/curseforge")
       .replace("edge.forgecdn.net", "mod.mcimirror.top")
       .replace("mediafilez.forgecdn.net", "mod.mcimirror.top")
       .replace("media.forgecdn.net", "mod.mcimirror.top")
}

/// PCL DlModRequest: 多源重试 + 不同超时
async fn cf_request(url: &str, body: Option<&serde_json::Value>, method: &str) -> Result<serde_json::Value, String> {
    let cf_key = "$2a$10$imvudMuNk5ycqn5MTRUvRua3DfgOMk28hJSpWAENkG8bVJWDO5sRW";
    let mirror = cf_mirror(url);

    // PCL: 镜像优先(10s) → 镜像(10s) → 官方(30s) → 官方(30s)
    let attempts: Vec<(&str, u64, bool)> = vec![
        (&mirror, 8, false),
        (&mirror, 8, false),
        (url, 15, true),
        (url, 15, true),
    ];

    let mut last_err = String::new();
    for (u, timeout, needs_key) in &attempts {
        let client = match build_http_client(Duration::from_secs(*timeout)) {
            Ok(c) => c,
            Err(e) => { eprintln!("[cf] client build failed: {}", e); last_err = format!("client: {}", e); continue; }
        };
        let mut req = match method {
            "POST" => client.post(*u).header("Content-Type", "application/json"),
            _ => client.get(*u),
        };
        req = req.header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36");
        req = req.header("Accept", "application/json");
        req = req.header("Accept-Language", "zh-CN,zh;q=0.9");
        if *needs_key { req = req.header("x-api-key", cf_key); }
        if let Some(b) = body { req = req.json(b); }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                eprintln!("[cf] {} -> HTTP {}", u, status.as_u16());
                let body = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => { eprintln!("[cf] {} text() error: {}", u, e); String::new() }
                };
                eprintln!("[cf] {} body len={}", u, body.len());
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if json["data"].is_array() || json["data"].is_object() {
                        return Ok(json);
                    }
                    last_err = format!("{} unexpected response structure", u);
                } else {
                    last_err = format!("{} returned non-JSON: {}", u, body.chars().take(100).collect::<String>());
                }
            }
            Err(e) => {
                eprintln!("[cf] {} HTTP error: {}", u, e);
                last_err = format!("{}: {}", u, e);
            }
        }
    }
    eprintln!("[cf] all attempts failed: {}", last_err);
    Err(last_err)
}

/// 搜索 CurseForge — Minecraft gameId=432
#[tauri::command]
pub(crate) async fn search_curseforge(
    query: String,
    project_type: String,
    version: String,
    sort: String,
    limit: u32,
    offset: u32,
    _source: String,
) -> Result<serde_json::Value, String> {
    let class_id = match project_type.as_str() {
        "mod" | "mods" | "" => 6, "resourcepack" => 12, "map" => 17, "datapack" => 6,
        "modpack" => 4471, "shader" => 0, _ => 6,
    };
    let sort_field = match sort.as_str() {
        "downloads" => 2, "popularity" | "" => 1, "name" => 3, "recent" => 0, _ => 1,
    };

    let mut path = format!("/v1/mods/search?gameId=432&classId={}&sortField={}&sortOrder=desc&pageSize={}&index={}",
        class_id, sort_field, limit, offset / limit.max(1));
    if !query.is_empty() { path.push_str(&format!("&searchFilter={}", urlencoding::encode(&query))); }
    if !version.is_empty() { path.push_str(&format!("&gameVersion={}", urlencoding::encode(&version))); }

    let url = format!("https://api.curseforge.com{}", path);
    cf_request(&url, None, "GET").await
}

/// 获取 CurseForge 项目详情
#[tauri::command]
pub(crate) async fn get_curseforge_project(mod_id: u32, _source: String) -> Result<serde_json::Value, String> {
    cf_request(&format!("https://api.curseforge.com/v1/mods/{}", mod_id), None, "GET").await
}

/// 获取 CurseForge 项目文件列表
#[tauri::command]
pub(crate) async fn get_curseforge_files(mod_id: u32, _source: String) -> Result<serde_json::Value, String> {
    cf_request(&format!("https://api.curseforge.com/v1/mods/{}/files?pageSize=50", mod_id), None, "GET").await
}

/// 下载并安装 CurseForge 文件（带进度）
#[tauri::command]
pub(crate) async fn install_curseforge_file(
    app: tauri::AppHandle,
    mc_dir: String,
    file_id: u32,
    file_name: String,
    project_type: String,
    source: String,
    instance_name: String,
) -> Result<String, String> {
    let base = std::path::Path::new(&mc_dir);
    let dm = if base.join(".minecraft").exists() { base.join(".minecraft") } else { base.to_path_buf() };
    let vd = dm.join("versions").join(&instance_name);
    let game_dir = if !instance_name.is_empty() && vd.exists() { &vd } else { &dm };
    let td = match project_type.as_str() {
        "mod" => game_dir.join("mods"),
        "shader" => game_dir.join("shaderpacks"),
        "resourcepack" => game_dir.join("resourcepacks"),
        "datapack" => game_dir.join("datapacks"),
        "map" => game_dir.join("saves"),
        _ => game_dir.to_path_buf(),
    };
    std::fs::create_dir_all(&td).map_err(|e| e.to_string())?;
    let dest = td.join(&file_name);

    // CurseForge 下载 URL 格式: edge.forgecdn.net/files/{fileId/1000}/{fileId%1000}/{fileName}
    let first = file_id / 1000;
    let second = file_id % 1000;
    let dl_url = format!("https://edge.forgecdn.net/files/{}/{}/{}", first, second, urlencoding::encode(&file_name));

    // CurseForge CDN 需要有效 APIKey → 仅用镜像
    let dl_url = cf_mirror(&dl_url);

    let _ = app.emit("download-progress", serde_json::json!({
        "filename": &file_name, "downloaded": 0u64, "total": 0u64, "percent": 0u32
    }));

    let mut dl = DownloadFile::new(vec![dl_url], dest.clone(), FileChecker::with_min_size(1));
    download_file(&app, &mut dl, false).await?;

    Ok(format!("已安装到 {}", dest.display()))
}
