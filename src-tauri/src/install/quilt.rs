//! Quilt Loader 安装 — 参照 PCL-CE McDownloadQuiltLoader + MergeJson
#![allow(dead_code)]
//!
//! 流程同 Fabric: 下载原版 JSON (内存) → 合并 libraries → 消除 inheritsFrom → 写入自包含 JSON

use std::collections::HashSet;
use std::path::Path;

pub async fn install_quilt_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    quilt_version: &str,
) -> Result<(), String> {
    let client = crate::http::build_http_client(std::time::Duration::from_secs(60))?;

    let profile_url = format!(
        "https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json",
        mc_version, quilt_version
    );
    eprintln!("[quilt] 下载 profile: {}", profile_url);
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value =
        serde_json::from_str(&profile_str).map_err(|e| format!("Quilt profile 解析失败: {}", e))?;

    // 下载原版 JSON → 合并 libraries
    let vanilla_json = download_vanilla_json_mem(&client, mc_version).await?;
    merge_vanilla_libs(&mut profile, &vanilla_json);

    // PCL-CE: 消除 inheritsFrom
    profile.as_object_mut().map(|o| o.remove("inheritsFrom"));
    profile.as_object_mut().map(|o| o.remove("_comment_"));
    profile.as_object_mut().map(|o| o.remove("jar"));
    profile["id"] = serde_json::Value::String(instance_name.to_string());

    if profile.get("mainClass").is_none() {
        if let Some(mc) = vanilla_json.get("mainClass").cloned() { profile["mainClass"] = mc; }
    }
    if profile.get("assets").is_none() {
        if let Some(a) = vanilla_json.get("assets").cloned() { profile["assets"] = a; }
    }
    if profile.get("assetIndex").is_none() && !vanilla_json["assetIndex"].is_null() {
        profile["assetIndex"] = vanilla_json["assetIndex"].clone();
    }

    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&profile).unwrap_or(profile_str),
    ).map_err(|e| e.to_string())?;

    let libs = crate::version::library::mclib_list_from_json(&profile, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if let Some(jar_url) = vanilla_json["downloads"]["client"]["url"].as_str() {
        let jar_path = target.join(format!("{}.jar", instance_name));
        let urls = crate::download::source::source_launcher_or_meta(jar_url, false);
        let checker = crate::download::model::FileChecker::with_min_size(1024);
        if checker.check(&jar_path).is_some() {
            dl_files.push(crate::download::model::DownloadFile::new(urls, jar_path, checker));
        }
    }
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    eprintln!("[quilt] 安装完成");
    Ok(())
}

fn merge_vanilla_libs(target: &mut serde_json::Value, vanilla: &serde_json::Value) {
    let v_libs = vanilla["libraries"].as_array().cloned().unwrap_or_default();
    let mut cur_libs = target["libraries"].as_array().cloned().unwrap_or_default();
    let mut seen: HashSet<String> = cur_libs.iter()
        .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
        .collect();
    for lib in v_libs {
        if let Some(name) = lib["name"].as_str() {
            if !seen.contains(name) { seen.insert(name.to_string()); cur_libs.push(lib); }
        }
    }
    target["libraries"] = serde_json::Value::Array(cur_libs);
}

async fn download_vanilla_json_mem(client: &reqwest::Client, mc_ver: &str) -> Result<serde_json::Value, String> {
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;
    let ver_url = manifest["versions"].as_array()
        .ok_or("版本清单为空")?
        .iter().find(|v| v["id"].as_str() == Some(mc_ver))
        .and_then(|v| v["url"].as_str())
        .ok_or(format!("找不到版本 {}", mc_ver))?;
    let urls = crate::download::source::source_launcher_or_meta(ver_url, false);
    for url in &urls {
        if let Ok(resp) = client.get(url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json.get("libraries").is_some() { return Ok(json); }
            }
        }
    }
    Err(format!("无法下载原版 JSON: {}", mc_ver))
}
