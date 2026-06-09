//! Fabric Loader 安装 — 参照 PCL-CE McDownloadFabricLoader + MergeJson
//!
//! PCL-CE 流程:
//! 1. 下载原版 JSON (内存) + Fabric profile JSON
//! 2. 合并 vanilla libraries + Fabric libraries
//! 3. outputJson.Remove("inheritsFrom") — 消除 inheritsFrom
//! 4. 写入自包含 JSON + 下载全部库文件

use std::collections::HashSet;
use std::path::Path;

pub async fn install_fabric_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    fabric_version: &str,
) -> Result<(), String> {
    let client = crate::http::build_http_client(std::time::Duration::from_secs(60))?;

    // 1) 下载 Fabric profile
    let profile_url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
        mc_version, fabric_version
    );
    eprintln!("[fabric] 下载 profile: {}", profile_url);
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value =
        serde_json::from_str(&profile_str).map_err(|e| format!("Fabric profile 解析失败: {}", e))?;

    // 2) 下载原版 JSON 到内存 (不写 versions/{mc_ver}/)
    let vanilla_json = download_vanilla_json_mem(&client, mc_version).await?;

    // 3) PCL-CE MergeJson: 合并原版 libraries + Fabric libraries
    merge_vanilla_libs(&mut profile, &vanilla_json);

    // 4) PCL-CE 关键: 消除 inheritsFrom
    profile.as_object_mut().map(|o| o.remove("inheritsFrom"));
    profile.as_object_mut().map(|o| o.remove("_comment_"));
    profile.as_object_mut().map(|o| o.remove("jar"));
    profile["id"] = serde_json::Value::String(instance_name.to_string());

    // 5) 确保 mainClass / assets / assetIndex 存在
    if profile.get("mainClass").is_none() {
        if let Some(mc) = vanilla_json.get("mainClass").cloned() {
            profile["mainClass"] = mc;
        }
    }
    if profile.get("assets").is_none() {
        if let Some(a) = vanilla_json.get("assets").cloned() {
            profile["assets"] = a;
        }
    }
    if profile.get("assetIndex").is_none() && !vanilla_json["assetIndex"].is_null() {
        profile["assetIndex"] = vanilla_json["assetIndex"].clone();
    }

    // 6) 保存 JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&profile).unwrap_or(profile_str),
    ).map_err(|e| e.to_string())?;

    // 7) 下载所有 libraries (含原版 JAR)
    let libs = crate::version::library::mclib_list_from_json(&profile, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);

    // 添加原版 client JAR
    if let Some(jar_url) = vanilla_json["downloads"]["client"]["url"].as_str() {
        let sha1 = vanilla_json["downloads"]["client"]["sha1"].as_str().map(|s| s.to_string());
        let size = vanilla_json["downloads"]["client"]["size"].as_i64().unwrap_or(-1);
        let jar_path = target.join(format!("{}.jar", instance_name));
        let urls = crate::download::source::source_launcher_or_meta(jar_url, false);
        let checker = crate::download::model::FileChecker::new(1024, size, sha1);
        if checker.check(&jar_path).is_some() {
            dl_files.push(crate::download::model::DownloadFile::new(urls, jar_path, checker));
        }
    }

    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    eprintln!("[fabric] 安装完成: fabric-loader-{}-{}", fabric_version, mc_version);
    Ok(())
}

/// PCL-CE MergeJson 风格: 合并原版 libraries 到目标 JSON, 去重
fn merge_vanilla_libs(target: &mut serde_json::Value, vanilla: &serde_json::Value) {
    let v_libs = vanilla["libraries"].as_array().cloned().unwrap_or_default();
    let mut cur_libs = target["libraries"].as_array().cloned().unwrap_or_default();

    let mut seen: HashSet<String> = cur_libs
        .iter()
        .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
        .collect();

    for lib in v_libs {
        if let Some(name) = lib["name"].as_str() {
            if !seen.contains(name) {
                seen.insert(name.to_string());
                cur_libs.push(lib);
            }
        }
    }

    target["libraries"] = serde_json::Value::Array(cur_libs);
}

/// 从 Mojang API 下载版本 JSON (不落盘)
async fn download_vanilla_json_mem(
    client: &reqwest::Client,
    mc_ver: &str,
) -> Result<serde_json::Value, String> {
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let ver_url = manifest["versions"].as_array()
        .ok_or("版本清单为空")?
        .iter()
        .find(|v| v["id"].as_str() == Some(mc_ver))
        .and_then(|v| v["url"].as_str())
        .ok_or(format!("找不到版本 {}", mc_ver))?;

    let urls = crate::download::source::source_launcher_or_meta(ver_url, false);
    for url in &urls {
        if let Ok(resp) = client.get(url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json.get("libraries").is_some() {
                    return Ok(json);
                }
            }
        }
    }
    Err(format!("无法下载原版 JSON: {}", mc_ver))
}
