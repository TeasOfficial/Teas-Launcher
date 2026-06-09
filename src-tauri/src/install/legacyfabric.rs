//! Legacy Fabric Loader 安装 — 参照 PCL-CE McDownloadLegacyFabricLoader
//!
//! 流程同 Fabric: 下载 profile JSON → 保存 → 下载 libraries

use std::path::Path;

#[allow(dead_code)]
pub async fn install_legacyfabric_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    loader_version: &str,
) -> Result<(), String> {
    let client = crate::http::build_http_client(std::time::Duration::from_secs(60))?;

    let profile_url = format!(
        "https://meta.legacyfabric.net/v2/versions/loader/{}/{}/profile/json",
        mc_version, loader_version
    );
    eprintln!("[legacyfabric] 下载 profile: {}", profile_url);
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value =
        serde_json::from_str(&profile_str).map_err(|e| format!("profile 解析失败: {}", e))?;

    profile["id"] = serde_json::Value::String(instance_name.to_string());

    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&profile).unwrap_or(profile_str),
    )
    .map_err(|e| e.to_string())?;

    let libs = crate::version::library::mclib_list_from_json(&profile, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    Ok(())
}
