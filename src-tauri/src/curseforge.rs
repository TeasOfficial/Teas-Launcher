//! CurseForge API：搜索、项目详情、文件下载
//! 参照 PCL-CE ResourceProject/Curseforge 模块

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::http::build_http_client;
use std::time::Duration;

// ── CurseForge API Key ────────────────────────────────────────
//
// 安全约定（改代码时务必遵守）：
//   1. 绝不复述 key：不进日志、不进错误串、不作为命令返回值回传前端
//   2. 绝不硬编码：本文件历史上硬编码过 PCL 的公共 key，已移除
//   3. 优先级：环境变量 TEAS_CURSEFORGE_API_KEY > %APPDATA%\TeasLauncher\curseforge.key
//      （本地文件由 scripts/set-cf-key.ps1 创建，ACL 仅当前用户可读）
//   4. key 文件不进版本库：.gitignore 已排除 *.key

const KEY_ENV_VAR: &str = "TEAS_CURSEFORGE_API_KEY";

/// key 文件路径（%APPDATA%\TeasLauncher\curseforge.key）
pub(crate) fn key_file_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("TeasLauncher").join("curseforge.key"))
}

/// 读取 key：环境变量优先，其次本地文件；都为空则返回 None
///
/// 返回值仅用于拼请求头，调用方不得记录或回传。
pub(crate) fn api_key() -> Option<String> {
    if let Ok(v) = std::env::var(KEY_ENV_VAR) {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let path = key_file_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let t = raw.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// 登录成功后尽量收紧文件 ACL（仅当前用户）。失败不影响功能。
#[cfg(windows)]
fn harden_key_file_acl(path: &std::path::Path) {
    let user = match std::env::var("USERNAME") {
        Ok(u) if !u.trim().is_empty() => u,
        _ => return,
    };
    let _ = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{}:F", user))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(not(windows))]
fn harden_key_file_acl(_path: &std::path::Path) {}

/// 查询 key 是否已配置（只返回状态，不返回 key 本身）
#[tauri::command]
pub(crate) fn curseforge_key_status() -> serde_json::Value {
    let from_env = std::env::var(KEY_ENV_VAR)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let file = key_file_path();
    let file_exists = file.as_ref().map(|p| p.exists()).unwrap_or(false);
    serde_json::json!({
        "configured": from_env || file_exists,
        "source": if from_env { "env" } else if file_exists { "file" } else { "none" },
        "file": file.map(|p| p.display().to_string()).unwrap_or_default(),
    })
}

/// 保存 key 到本地文件（前端只提交一次，后端落盘后不再回传）
#[tauri::command]
pub(crate) fn curseforge_save_key(key: String) -> Result<serde_json::Value, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("API Key 为空".into());
    }
    if k.len() < 20 || k.len() > 200 || k.chars().any(|c| c.is_whitespace()) {
        return Err("API Key 格式不正确（长度 20-200，且不能包含空白字符）".into());
    }
    let path = key_file_path().ok_or("无法确定配置目录")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    std::fs::write(&path, k).map_err(|e| format!("写入失败: {}", e))?;
    harden_key_file_acl(&path);
    log::info!("[cf] API Key 已保存到本地文件（不记录内容）");
    Ok(serde_json::json!({ "configured": true, "file": path.display().to_string() }))
}

/// 清除本地文件里的 key（环境变量不受影响）
#[tauri::command]
pub(crate) fn curseforge_clear_key() -> Result<(), String> {
    if let Some(path) = key_file_path() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("删除失败: {}", e))?;
        }
    }
    Ok(())
}

/// 用当前 key 访问一个最小接口，验证是否可用
///
/// 只返回游戏名/状态码；任何情况下都把 key 本身排除在返回值和日志之外。
#[tauri::command]
pub(crate) async fn curseforge_test_key() -> Result<serde_json::Value, String> {
    let key = api_key().ok_or("未配置 API Key")?;
    let client = build_http_client(Duration::from_secs(15))?;
    let resp = client
        .get("https://api.curseforge.com/v1/games/432")
        .header("x-api-key", &key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("CurseForge 返回 HTTP {}（key 无效或已过期）", status.as_u16()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {}", e))?;
    Ok(serde_json::json!({
        "ok": true,
        "game": json["data"]["name"].as_str().unwrap_or(""),
    }))
}


/// PCL DlModRequest: 多源重试 + 不同超时
async fn cf_request(url: &str, body: Option<&serde_json::Value>, method: &str) -> Result<serde_json::Value, String> {
    let key = api_key();

    // MCIMirror 已停止运营：只走官方接口。没有 key 就直接给出可操作的错误，
    // 而不是让用户等两次必然 403 的请求。
    if key.is_none() {
        return Err("未配置 CurseForge API Key（官方接口必需）。请运行 scripts/set-cf-key.ps1 配置后重启启动器。".to_string());
    }
    let attempts: Vec<(String, u64, bool)> = vec![
        (url.to_string(), 20, true),
        (url.to_string(), 20, true),
    ];

    let mut last_err = String::new();
    for (u, timeout, needs_key) in &attempts {
        let client = match build_http_client(Duration::from_secs(*timeout)) {
            Ok(c) => c,
            Err(e) => { log::warn!("[cf] client build failed: {}", e); last_err = format!("client: {}", e); continue; }
        };
        // attempts 里存的是 String，reqwest 的 IntoUrl 只实现了 &str / String / Url
        let mut req = match method {
            "POST" => client.post(u.as_str()).header("Content-Type", "application/json"),
            _ => client.get(u.as_str()),
        };
        req = req.header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36");
        req = req.header("Accept", "application/json");
        req = req.header("Accept-Language", "zh-CN,zh;q=0.9");
        if *needs_key {
            match &key {
                Some(k) => req = req.header("x-api-key", k),
                None => {}
            }
        }
        if let Some(b) = body { req = req.json(b); }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                log::debug!("[cf] {} -> HTTP {}", u, status.as_u16());
                let body = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => { log::warn!("[cf] {} text() error: {}", u, e); String::new() }
                };
                log::debug!("[cf] {} body len={}", u, body.len());
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
                log::warn!("[cf] {} HTTP error: {}", u, e);
                last_err = format!("{}: {}", u, e);
            }
        }
    }
    log::error!("[cf] all attempts failed: {}", last_err);
    Err(format!("CurseForge 接口请求失败：{}", last_err))
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
    // CurseForge 的 classId（官方枚举，写死错了会静默返回空列表）：
    //   6=Mods 12=Resource Packs 17=Worlds 4471=Modpacks 6552=Shaders 6945=Data Packs
    let class_id = match project_type.as_str() {
        "mod" | "mods" | "" => 6,
        "modpack" => 4471,
        "resourcepack" => 12,
        "shader" => 6552,
        "datapack" => 6945,
        "map" | "world" => 17,
        _ => 6,
    };
    // sortField: 1=Featured 2=Popularity 3=LastUpdated 4=Name 6=TotalDownloads 11=ReleasedDate
    let sort_field = match sort.as_str() {
        "relevance" | "" => 1,
        "downloads" => 6,
        "follows" => 2,
        "newest" => 11,
        "updated" => 3,
        "name" => 4,
        _ => 1,
    };

    // index 是「第几个条目」，不是页码；以前用 offset/limit 会导致第 2 页起重复。
    let mut path = format!("/v1/mods/search?gameId=432&classId={}&sortField={}&sortOrder=desc&pageSize={}&index={}",
        class_id, sort_field, limit, offset);
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
    let dm = crate::utils::dot_minecraft(&mc_dir);
    // source 参数预留（镜像源切换）
    let _ = &source;
    let game_dir = crate::utils::instance_game_dir(&dm, &instance_name);
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

    // 直连官方 CDN（镜像已停运）。CDN 不校验 API Key，fileId 拼接的地址即可下载。

    crate::progress::emit_stage(&app, &file_name, 0, &format!("准备下载 {}", file_name));

    let mut dl = DownloadFile::new(vec![dl_url], dest.clone(), FileChecker::with_min_size(1));
    download_file(&app, &mut dl, false).await?;

    Ok(format!("已安装到 {}", dest.display()))
}
