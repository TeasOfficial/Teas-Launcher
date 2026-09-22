//! Quilt Loader 安装 — 参照 PCL-CE McDownloadQuiltLoader + MergeJson
//!
//! 流程同 Fabric: 下载原版 JSON (内存) → 合并 libraries → 消除 inheritsFrom → 写入自包含 JSON

use crate::install::merge::{
    download_libs_and_client_jar, download_vanilla_json_mem, merge_libraries, strip_to_self_contained,
};
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
    log::debug!("[quilt] 下载 profile: {}", profile_url);
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value =
        serde_json::from_str(&profile_str).map_err(|e| format!("Quilt profile 解析失败: {}", e))?;

    let vanilla_json = download_vanilla_json_mem(&client, mc_version).await?;
    merge_libraries(&mut profile, &vanilla_json);

    strip_to_self_contained(&mut profile, instance_name);

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

    download_libs_and_client_jar(app, mc_dir, &profile, instance_name, &vanilla_json).await;

    log::info!("[quilt] 安装完成");
    Ok(())
}
