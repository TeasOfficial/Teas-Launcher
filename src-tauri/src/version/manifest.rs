//! 版本清单获取 — 参照 PCL-CE ModDownload.cs (所有 Dl*List* 函数)
//!
//! 支持的版本列表:
//! - Minecraft (Mojang + BMCLAPI + UVMC)
//! - Forge (Official HTML + BMCLAPI JSON)
//! - NeoForge (Reposilite API)
//! - Fabric / Quilt
//! - Cleanroom (GitHub API)

use crate::http::build_http_client;
use std::time::Duration;

/// Minecraft 版本清单获取
///
/// 参照 PCL-CE DlClientListMojangMain / DlClientListBmclapiMain:
/// - Mojang: https://launchermeta.mojang.com/mc/game/version_manifest.json
/// - BMCLAPI: https://bmclapi2.bangbang93.com/mc/game/version_manifest.json
/// - UVMC 合并: 缓存文件到 temp/Cache/uvmc-download.json
#[tauri::command]
pub async fn fetch_version_manifest() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(30))?;

    // 先尝试 Mojang
    let mojang_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let mojang_result = client.get(mojang_url).send().await;
    let (json, source_name) = match mojang_result {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(j) if j["versions"].as_array().map_or(0, |a| a.len()) >= 200 => {
                    (j, "Mojang")
                }
                _ => {
                    // 回退 BMCLAPI
                    let bmcl = client
                        .get("https://bmclapi2.bangbang93.com/mc/game/version_manifest.json")
                        .send()
                        .await
                        .map_err(|e| e.to_string())?;
                    (bmcl.json().await.map_err(|e| e.to_string())?, "BMCLAPI")
                }
            }
        }
        _ => {
            let bmcl = client
                .get("https://bmclapi2.bangbang93.com/mc/game/version_manifest.json")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            (bmcl.json().await.map_err(|e| e.to_string())?, "BMCLAPI")
        }
    };

    log::info!("[manifest] Minecraft 版本列表来自: {}", source_name);
    Ok(json)
}

/// Forge Minecraft 版本列表 (哪些 MC 版本有 Forge)
///
/// 参照 PCL-CE DlForgeListMain:
/// - Official: https://files.minecraftforge.net/maven/net/minecraftforge/forge/index_1.2.4.html
/// - BMCLAPI: https://bmclapi2.bangbang93.com/forge/minecraft
#[tauri::command]
pub async fn fetch_forge_mc_versions() -> Result<Vec<String>, String> {
    let client = build_http_client(Duration::from_secs(30))?;

    // 尝试 BMCLAPI
    let bmcl_url = "https://bmclapi2.bangbang93.com/forge/minecraft";
    if let Ok(resp) = client.get(bmcl_url).send().await {
        if let Ok(text) = resp.text().await {
            if text.len() >= 200 {
                // JSON 解析: 如 "1.20.1", "1.19.4", ...
                if let Ok(arr) = serde_json::from_str::<Vec<String>>(&text) {
                    return Ok(arr);
                }
            }
        }
    }

    // 回退 Official HTML 解析
    let official_url =
        "https://files.minecraftforge.net/maven/net/minecraftforge/forge/index_1.2.4.html";
    let resp = client
        .get(official_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let html = resp.text().await.map_err(|e| e.to_string())?;

    // 解析 HTML: <a href="index_1.20.1.html">
    let mut versions: Vec<String> = Vec::new();
    let mut search_start = 0;
    while let Some(href_pos) = html[search_start..].find(r#"a href="index_"#) {
        let start = search_start + href_pos + r#"a href="index_"#.len();
        if let Some(end_pos) = html[start..].find(".html") {
            let ver = &html[start..start + end_pos];
            let ver = ver.trim().to_string();
            if !ver.is_empty() && !versions.contains(&ver) {
                versions.push(ver);
            }
            search_start = start + end_pos + 5;
        } else {
            break;
        }
    }
    versions.push("1.2.4".to_string()); // 不会被循环匹配
    versions.sort();
    versions.dedup();

    if versions.len() < 10 {
        return Err("Forge 版本列表获取失败".to_string());
    }

    Ok(versions)
}

/// Forge 版本详情 (某 MC 版本的 Forge 加载器版本列表)
///
/// 参照 PCL-CE DlForgeVersionMain:
/// - Official HTML: https://files.minecraftforge.net/maven/net/minecraftforge/forge/index_{mc_version}.html
/// - BMCLAPI JSON: https://bmclapi2.bangbang93.com/forge/minecraft/{mc_version}
#[tauri::command]
pub async fn fetch_forge_versions(
    mc_version: String,
) -> Result<Vec<serde_json::Value>, String> {
    let client = build_http_client(Duration::from_secs(30))?;
    let mc_clean = mc_version.replace('-', "_");

    // 尝试 BMCLAPI
    let bmcl_url = format!(
        "https://bmclapi2.bangbang93.com/forge/minecraft/{}",
        mc_clean
    );
    if let Ok(resp) = client.get(&bmcl_url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json.as_array() {
                let mut versions = Vec::new();
                for entry in arr {
                    let version = entry["version"].as_str().unwrap_or("");
                    let branch = entry["branch"].as_str().unwrap_or("");
                    let modified = entry["modified"].as_str().unwrap_or("");
                    // 优先 installer → universal → client
                    let (hash, category) = if let Some(files) = entry["files"].as_array() {
                        let mut hash = "";
                        let mut cat = "unknown";
                        for f in files {
                            let fc = f["category"].as_str().unwrap_or("");
                            let ff = f["format"].as_str().unwrap_or("");
                            match (fc, ff) {
                                ("installer", "jar") if cat != "installer" => {
                                    hash = f["hash"].as_str().unwrap_or("");
                                    cat = "installer";
                                }
                                ("universal", "zip") if cat != "installer" => {
                                    hash = f["hash"].as_str().unwrap_or("");
                                    cat = "universal";
                                }
                                ("client", "zip") if cat == "unknown" => {
                                    hash = f["hash"].as_str().unwrap_or("");
                                    cat = "client";
                                }
                                _ => {}
                            }
                        }
                        (hash.to_string(), cat.to_string())
                    } else {
                        (String::new(), "unknown".to_string())
                    };

                    if version.is_empty() { continue; }
                    versions.push(serde_json::json!({
                        "version": version,
                        "branch": branch,
                        "category": category,
                        "hash": hash,
                        "releaseTime": modified,
                    }));
                }
                return Ok(versions);
            }
        }
    }

    // 回退 Official HTML
    let official_url = format!(
        "https://files.minecraftforge.net/maven/net/minecraftforge/forge/index_{}.html",
        mc_clean
    );
    let resp = client
        .get(&official_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let html = resp.text().await.map_err(|e| e.to_string())?;

    // 简单解析 HTML 中的版本号
    let mut versions = Vec::new();
    // 在 class="download-version" 区域中查找版本号
    for section in html.split(r#"class="download-version"#).skip(1) {
        // 版本号格式: XX.XX.X.XXXX (如 36.2.34 或 14.23.5.2859)
        let chars = section.chars();
        let _buf = String::new();
        let _dot_count = 0u32;
        let mut digit_seq = String::new();
        for c in chars.take(2000) {
            // 只扫描每个 section 的前 2000 字符
            if c.is_ascii_digit() {
                digit_seq.push(c);
            } else if c == '.' && !digit_seq.is_empty() {
                digit_seq.push(c);
            } else {
                if digit_seq.len() >= 5 && digit_seq.matches('.').count() >= 2 {
                    let ver = digit_seq.trim_end_matches('.').to_string();
                    if !versions.iter().any(|v: &serde_json::Value| v["version"] == ver) {
                        versions.push(serde_json::json!({
                            "version": ver,
                            "category": if section.contains("installer.jar") { "installer" }
                                        else if section.contains("universal.zip") { "universal" }
                                        else if section.contains("client.zip") { "client" }
                                        else { "unknown" },
                            "hash": "",
                        }));
                    }
                }
                digit_seq.clear();
                if c != '.' { break; }
            }
        }
    }

    if versions.is_empty() {
        return Err(format!("找不到 {} 的 Forge 版本", mc_version));
    }

    Ok(versions)
}

/// NeoForge 版本列表
///
/// 参照 PCL-CE DlNeoForgeListMain:
/// - Reposilite API: https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge
/// - Legacy: https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge (1.20.1)
///
/// 版本号解析 MC 版本: Major≥26 → 直接用作 MC 版本, ≤25 → "1.{major}.{minor}"
#[tauri::command]
pub async fn fetch_neoforge_versions() -> Result<Vec<serde_json::Value>, String> {
    let client = build_http_client(Duration::from_secs(30))?;

    let mut all_versions = Vec::new();

    // 新版 (NeoForge 1.20.2+)
    let latest_url =
        "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
    if let Ok(resp) = client.get(latest_url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json["versions"].as_array() {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        let mc_ver = parse_neoforge_mc_version(s);
                        let is_beta = s.contains("-beta") || s.contains("-alpha");
                        all_versions.push(serde_json::json!({
                            "version": s,
                            "mc_version": mc_ver,
                            "stable": !is_beta,
                            "is_legacy": false
                        }));
                    }
                }
            }
        }
    }

    // 旧版 (NeoForge 1.20.1)
    let legacy_url =
        "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge";
    if let Ok(resp) = client.get(legacy_url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json["versions"].as_array() {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        // Legacy: "1.20.1-47.1.99" 格式
                        if s.starts_with("1.20.1-") {
                            let ver = s.trim_start_matches("1.20.1-");
                            let is_beta = ver.contains("-beta");
                            all_versions.push(serde_json::json!({
                                "version": s,
                                "mc_version": "1.20.1",
                                "stable": !is_beta,
                                "is_legacy": true
                            }));
                        }
                    }
                }
            }
        }
    }

    if all_versions.is_empty() {
        return Err("NeoForge 版本列表获取失败".to_string());
    }

    // 按版本号降序
    all_versions.sort_by(|a, b| {
        let va = a["version"].as_str().unwrap_or("");
        let vb = b["version"].as_str().unwrap_or("");
        parse_version_for_cmp(vb).cmp(&parse_version_for_cmp(va))
    });

    Ok(all_versions)
}

/// 解析 NeoForge 版本号对应的 MC 版本
///
/// 参照 PCL-CE DlNeoForgeListEntry 构造函数:
/// - Major≥24: 直接 "{major}.{minor}[.{build}]" (+ snapshot 后缀)
/// - Major≤23: "1.{major}[.{minor}]"
pub fn parse_neoforge_mc_version(version: &str) -> String {
    let segments: Vec<&str> = version.split('.').collect();
    if segments.is_empty() { return String::new(); }

    let major: i32 = segments[0].parse().unwrap_or(0);
    if major >= 24 {
        let dots: Vec<&str> = version
            .split(|c: char| c == '-' || c == '+')
            .next()
            .unwrap_or(version)
            .split('.')
            .collect();
        match dots.len() {
            1 => format!("{major}"),
            2 => format!("{}.{}", dots[0], dots[1]),
            _ => format!("{}.{}.{}", dots[0], dots[1], dots[2]),
        }
    } else if major == 0 {
        // Snapshot 版本
        if segments.len() >= 2 {
            segments[1].to_string()
        } else {
            segments[0].to_string()
        }
    } else {
        if segments.len() >= 2 {
            let minor: i32 = segments[1].parse().unwrap_or(0);
            if minor == 0 {
                format!("1.{major}")
            } else {
                format!("1.{major}.{minor}")
            }
        } else {
            format!("1.{major}")
        }
    }
}

/// 解析版本号为数值数组用于排序
fn parse_version_for_cmp(version: &str) -> Vec<u32> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().unwrap_or(0))
        .collect()
}

/// Fabric 版本列表
///
/// 参照 PCL-CE DlFabricListMain:
/// - Official: https://meta.fabricmc.net/v2/versions
/// - BMCLAPI: https://bmclapi2.bangbang93.com/fabric-meta/v2/versions
///
/// 返回 { game: [...], loader: [...], installer: [...] }
#[tauri::command]
pub async fn fetch_fabric_versions() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(15))?;

    let urls = [
        "https://meta.fabricmc.net/v2/versions",
        "https://bmclapi2.bangbang93.com/fabric-meta/v2/versions",
    ];

    for url in &urls {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json.get("game").is_some()
                    && json.get("loader").is_some()
                    && json.get("installer").is_some()
                {
                    return Ok(json);
                }
            }
        }
    }

    Err("Fabric 版本列表获取失败".to_string())
}

/// Quilt 版本列表
///
/// 参照 PCL-CE DlQuiltListMain:
/// - Official: https://meta.quiltmc.org/v3/versions
#[tauri::command]
pub async fn fetch_quilt_versions() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(15))?;
    let resp = client
        .get("https://meta.quiltmc.org/v3/versions")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if json.get("game").is_none() || json.get("loader").is_none() {
        return Err("Quilt 版本列表不完整".to_string());
    }
    Ok(json)
}

/// Cleanroom 版本列表 (GitHub API)
///
/// 参照 PCL-CE DlCleanroomListMain:
/// - https://api.github.com/repos/CleanroomMC/Cleanroom/releases
#[tauri::command]
pub async fn fetch_cleanroom_versions() -> Result<Vec<serde_json::Value>, String> {
    let client = build_http_client(Duration::from_secs(15))?;
    let resp = client
        .get("https://api.github.com/repos/CleanroomMC/Cleanroom/releases")
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "TeasLauncher/1.0")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json = resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())?;

    let versions: Vec<serde_json::Value> = json
        .as_array()
        .ok_or("无法解析 Cleanroom 版本")?
        .iter()
        .map(|release| {
            let tag = release["tag_name"].as_str().unwrap_or("");
            serde_json::json!({
                "version": tag,
                "inherit": "1.12.2",
                "isBeta": tag.contains("alpha"),
            })
        })
        .collect();

    Ok(versions)
}
