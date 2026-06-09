//! CurseForge API：搜索、项目详情、文件下载
//! 参照 PCL-CE ResourceProject/Curseforge 模块

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::http::build_http_client;
use std::time::Duration;
use tauri::Emitter;

const CF_BASE: &str = "https://api.curseforge.com/v1";
const CF_MIRROR: &str = "https://mod.mcimirror.top/curseforge/v1";

fn cf_url(path: &str, use_mirror: bool) -> String {
    let base = if use_mirror { CF_MIRROR } else { CF_BASE };
    format!("{}{}", base, path)
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
    source: String,
) -> Result<serde_json::Value, String> {
    let use_mirror = source == "MCIMirror";
    let client = build_http_client(Duration::from_secs(15))?;

    // CurseForge 分类映射
    let class_id = match project_type.as_str() {
        "mod" | "mods" | "" => 6,       // Mods
        "resourcepack" => 12,             // Resource Packs
        "shader" => 0,                    // 无单独分类
        "map" => 17,                      // Worlds
        "datapack" => 6,                  // 同 Mods
        _ => 6,
    };

    let sort_field = match sort.as_str() {
        "downloads" => 2,
        "popularity" | "" => 1,
        "name" => 3,
        "recent" => 0,
        _ => 1,
    };

    let desc = sort_field == 0;

    let mut url = cf_url("/mods/search", use_mirror);
    url.push_str(&format!("?gameId=432&classId={}&sortField={}&sortOrder=desc", class_id, sort_field));
    if desc { url.push_str("&sortOrder=desc"); } else { url.push_str("&sortOrder=desc"); }
    if !query.is_empty() {
        url.push_str(&format!("&searchFilter={}", urlencoding::encode(&query)));
    }
    if !version.is_empty() {
        url.push_str(&format!("&gameVersion={}", urlencoding::encode(&version)));
    }
    url.push_str(&format!("&pageSize={}&index={}", limit, offset / limit.max(1)));

    let resp = client.get(&url)
        .header("x-api-key", "$2a$10$imvudMuNk5ycqn5MTRUvRua3DfgOMk28hJSpWAENkG8bVJWDO5sRW")
        .send().await.map_err(|e| e.to_string())?;
    resp.json().await.map_err(|e| e.to_string())
}

/// 获取 CurseForge 项目详情
#[tauri::command]
pub(crate) async fn get_curseforge_project(mod_id: u32, source: String) -> Result<serde_json::Value, String> {
    let use_mirror = source == "MCIMirror";
    let client = build_http_client(Duration::from_secs(10))?;
    client.get(&cf_url(&format!("/mods/{}", mod_id), use_mirror))
        .header("x-api-key", "$2a$10$imvudMuNk5ycqn5MTRUvRua3DfgOMk28hJSpWAENkG8bVJWDO5sRW")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())
}

/// 获取 CurseForge 项目文件列表
#[tauri::command]
pub(crate) async fn get_curseforge_files(mod_id: u32, source: String) -> Result<serde_json::Value, String> {
    let use_mirror = source == "MCIMirror";
    let client = build_http_client(Duration::from_secs(10))?;
    client.get(&cf_url(&format!("/mods/{}/files?pageSize=50", mod_id), use_mirror))
        .header("x-api-key", "$2a$10$imvudMuNk5ycqn5MTRUvRua3DfgOMk28hJSpWAENkG8bVJWDO5sRW")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())
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
) -> Result<String, String> {
    let base = std::path::Path::new(&mc_dir);
    let dm = if base.join(".minecraft").exists() { base.join(".minecraft") } else { base.to_path_buf() };
    let td = match project_type.as_str() {
        "mod" => dm.join("mods"),
        "shader" => dm.join("shaderpacks"),
        "resourcepack" => dm.join("resourcepacks"),
        "datapack" => dm.join("datapacks"),
        "map" => dm.join("saves"),
        _ => dm.to_path_buf(),
    };
    std::fs::create_dir_all(&td).map_err(|e| e.to_string())?;
    let dest = td.join(&file_name);

    // CurseForge 下载 URL 格式: edge.forgecdn.net/files/{fileId/1000}/{fileId%1000}/{fileName}
    let first = file_id / 1000;
    let second = file_id % 1000;
    let dl_url = format!("https://edge.forgecdn.net/files/{}/{}/{}", first, second, urlencoding::encode(&file_name));

    let use_mirror = source == "MCIMirror";
    let mirror_url = dl_url.replace("edge.forgecdn.net", "mod.mcimirror.top");
    let urls: Vec<String> = if use_mirror {
        vec![mirror_url, dl_url]
    } else {
        vec![dl_url, mirror_url]
    };

    let _ = app.emit("download-progress", serde_json::json!({
        "filename": &file_name, "downloaded": 0u64, "total": 0u64, "percent": 0u32
    }));

    let mut dl = DownloadFile::new(urls, dest.clone(), FileChecker::with_min_size(1));
    download_file(&app, &mut dl, false).await?;

    Ok(format!("已安装到 {}", dest.display()))
}
