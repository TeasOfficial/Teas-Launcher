//! 版本清单获取 — 参照 PCL-CE ModDownload.cs (所有 Dl*List* 函数)
//!
//! 支持的版本列表:
//! - Minecraft (Mojang + BMCLAPI + UVMC)
//! - Forge (Official HTML + BMCLAPI JSON)
//! - NeoForge (Reposilite API)
//! - Fabric / Quilt / LegacyFabric
//! - OptiFine / LiteLoader / Cleanroom / LabyMod

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

    eprintln!("[manifest] Minecraft 版本列表来自: {}", source_name);
    Ok(json)
}

/// 根据版本名获取版本 JSON URL
///
/// 参照 PCL-CE DlClientListGet
#[allow(dead_code)]
pub async fn get_version_url(
    version: &str,
) -> Result<String, String> {
    let manifest = fetch_version_manifest().await?;
    let normalized = version
        .replace('_', "-");
    let normalized = if normalized != "1.0" && normalized.ends_with(".0") {
        &normalized[..normalized.len() - 2]
    } else {
        &normalized
    };

    manifest["versions"]
        .as_array()
        .ok_or("版本列表为空")?
        .iter()
        .find(|v| v["id"].as_str() == Some(normalized) || v["id"].as_str() == Some(version))
        .and_then(|v| v["url"].as_str().map(|s| s.to_string()))
        .ok_or(format!("找不到版本: {}", version))
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

/// 获取特定 MC 版本兼容的 Fabric Loader 版本
///
/// 参照 PCL-CE: fabric_versions["loader"] → 筛选稳定/不稳定版
#[allow(dead_code)]
pub async fn fetch_fabric_loader_versions(
    mc_version: &str,
) -> Result<Vec<serde_json::Value>, String> {
    let all = fetch_fabric_versions().await?;
    let game_normalized = mc_version
        .replace('∞', "infinite")
        .replace("Combat Test 7c", "1.16_combat-3");

    // 检查 game 数组中是否有此版本
    let game_supported = all["game"].as_array().map_or(false, |arr| {
        arr.iter().any(|v| {
            v["version"].as_str().map_or(false, |s| s == game_normalized)
        })
    });

    if !game_supported {
        return Ok(Vec::new());
    }

    let loaders = all["loader"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|v| {
            let version = v["version"].as_str().unwrap_or("");
            let stable = v["stable"].as_bool().unwrap_or(false);
            serde_json::json!({"version": version, "stable": stable})
        })
        .collect();

    Ok(loaders)
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

/// OptiFine 版本列表
///
/// 参照 PCL-CE DlOptiFineListMain:
/// - Official: https://optifine.net/downloads (HTML 解析)
/// - BMCLAPI: https://bmclapi2.bangbang93.com/optifine/versionList
#[tauri::command]
pub async fn fetch_optifine_versions() -> Result<Vec<serde_json::Value>, String> {
    let client = build_http_client(Duration::from_secs(30))?;

    // 尝试 BMCLAPI
    let bmcl_url = "https://bmclapi2.bangbang93.com/optifine/versionList";
    if let Ok(resp) = client.get(bmcl_url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json.as_array() {
                let versions: Vec<serde_json::Value> = arr.iter().map(|v| {
                    let mc = v["mcversion"].as_str().unwrap_or("");
                    let typ = v["type"].as_str().unwrap_or("");
                    let patch = v["patch"].as_str().unwrap_or("");
                    let filename = v["filename"].as_str().unwrap_or("");
                    let forge = v["forge"].as_str().unwrap_or("");

                    let display_name = format!(
                        "{} {}{}",
                        mc,
                        typ.replace("HD_U", "").replace('_', " "),
                        format!(" {}", patch).trim_end()
                    );
                    let name_version = format!("{}-OptiFine_{}_{}", mc, typ, patch);

                    serde_json::json!({
                        "displayName": display_name,
                        "inherit": mc,
                        "nameVersion": name_version,
                        "nameFile": filename,
                        "requiredForgeVersion": if forge.contains("N/A") { serde_json::Value::Null } else { serde_json::Value::String(forge.to_string()) },
                        "isPreview": patch.to_lowercase().contains("pre"),
                        "releaseTime": "",
                    })
                }).collect();
                return Ok(versions);
            }
        }
    }

    Err("OptiFine 版本列表获取失败".to_string())
}

/// LiteLoader 版本列表
///
/// 参照 PCL-CE DlLiteLoaderListMain:
/// - Official: https://dl.liteloader.com/versions/versions.json
/// - BMCLAPI: https://bmclapi2.bangbang93.com/maven/com/mumfrey/liteloader/versions.json
#[tauri::command]
pub async fn fetch_liteloader_versions() -> Result<Vec<serde_json::Value>, String> {
    let client = build_http_client(Duration::from_secs(15))?;

    let urls = [
        "https://bmclapi2.bangbang93.com/maven/com/mumfrey/liteloader/versions.json",
        "https://dl.liteloader.com/versions/versions.json",
    ];

    for url in &urls {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let versions_obj = json.get("versions").or(json.get("versions"));
                if let Some(obj) = versions_obj.and_then(|v| v.as_object()) {
                    let mut versions = Vec::new();
                    for (mc_ver, data) in obj {
                        if mc_ver.starts_with("1.6") || mc_ver.starts_with("1.5") {
                            continue;
                        }
                        let artifacts = data
                            .get("artefacts")
                            .or(data.get("snapshots"));
                        if let Some(real_entry) = artifacts
                            .and_then(|a| a.get("com.mumfrey:liteloader"))
                            .and_then(|l| l.get("latest"))
                        {
                            let md5 = real_entry["md5"].as_str().unwrap_or("");
                            let timestamp = real_entry["timestamp"]
                                .as_str()
                                .or(real_entry["timestamp"].as_i64().map(|_| ""))
                                .unwrap_or("");

                            let mc_num: f64 = mc_ver
                                .split('.')
                                .nth(1)
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0.0);

                            versions.push(serde_json::json!({
                                "inherit": mc_ver,
                                "isLegacy": mc_num < 8.0,
                                "isPreview": real_entry["stream"].as_str().unwrap_or("") == "snapshot",
                                "fileName": format!("liteloader-installer-{}-00-SNAPSHOT.jar",
                                    if mc_ver == "1.8" || mc_ver == "1.9" {
                                        format!("{}.0", mc_ver)
                                    } else {
                                        mc_ver.to_string()
                                    }),
                                "md5": md5,
                                "releaseTime": timestamp,
                            }));
                        }
                    }
                    return Ok(versions);
                }
            }
        }
    }

    Err("LiteLoader 版本列表获取失败".to_string())
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

/// LegacyFabric 版本列表
///
/// 参照 PCL-CE DlLegacyFabricListMain:
/// - https://meta.legacyfabric.net/v2/versions
#[tauri::command]
pub async fn fetch_legacyfabric_versions() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(15))?;
    let resp = client
        .get("https://meta.legacyfabric.net/v2/versions")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.json().await.map_err(|e| e.to_string())
}

/// LabyMod 版本列表
///
/// 参照 PCL-CE DlLabyModListMain:
/// - Production: https://releases.r2.labymod.net/api/v1/manifest/production/latest.json
/// - Snapshot: https://releases.r2.labymod.net/api/v1/manifest/snapshot/latest.json
#[tauri::command]
pub async fn fetch_labymod_versions() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(15))?;

    let prod_resp = client
        .get("https://releases.r2.labymod.net/api/v1/manifest/production/latest.json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let production: serde_json::Value =
        prod_resp.json().await.map_err(|e| e.to_string())?;

    let snap_resp = client
        .get("https://releases.r2.labymod.net/api/v1/manifest/snapshot/latest.json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let snapshot: serde_json::Value =
        snap_resp.json().await.map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "production": production,
        "snapshot": snapshot
    }))
}
