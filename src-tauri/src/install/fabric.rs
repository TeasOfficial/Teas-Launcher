//! Fabric Loader 安装 — 参照 PCL-CE McDownloadFabricLoader + MergeJson
//!
//! PCL-CE 流程:
//! 1. 下载原版 JSON (内存) + Fabric profile JSON
//! 2. 合并原版 libraries 到 Fabric profile
//! 3. 消除 inheritsFrom — 转为自包含 JSON
//! 4. 下载全部 libraries + 原版 JAR

use crate::install::merge::{
    download_libs_and_client_jar, download_vanilla_json_mem, merge_libraries, strip_to_self_contained,
};
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
    log::debug!("[fabric] 下载 profile: {}", profile_url);
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value =
        serde_json::from_str(&profile_str).map_err(|e| format!("Fabric profile 解析失败: {}", e))?;

    // 2) 下载原版 JSON 到内存 (不写 versions/{mc_ver}/)
    let vanilla_json = download_vanilla_json_mem(&client, mc_version).await?;

    // 3) PCL-CE MergeJson: 合并原版 libraries 到 Fabric profile
    merge_libraries(&mut profile, &vanilla_json);

    // 4) 消除 inheritsFrom
    strip_to_self_contained(&mut profile, instance_name);

    // 5) Fabric profile 未声明的字段从原版补齐
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

    // 7) 下载所有 libraries 与原版 client JAR
    download_libs_and_client_jar(app, mc_dir, &profile, instance_name, &vanilla_json).await;

    log::info!("[fabric] 安装完成: fabric-loader-{}-{}", fabric_version, mc_version);
    Ok(())
}
