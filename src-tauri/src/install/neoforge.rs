//! NeoForge 安装器 — 参照 PCL-CE McDownloadForgelikeLoader (NeoForge 分支)
//!
//! NeoForge 安装流程:
//! 1. 下载 installer.jar (BMCLAPI maven → maven.neoforged.net)
//! 2. 解压 install_profile.json + version.json → 合并
//! 3. 解析 libraries → 下载
//! 4. 提取内嵌 Maven 库
//! 5. 保存合并后的 version JSON

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use std::path::Path;

pub async fn install_neoforge_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    neoforge_version: &str,
    prefer_official: bool,
) -> Result<(), String> {
    let tmp_dir = mc_dir.join(format!(".tmp_nfg_{}", instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let installer_path = tmp_dir.join("neoforge_installer.jar");

    // 确定 api_name (如 "20.4.30-beta" 或 "1.20.1-47.1.99")
    let api_name = neoforge_version;

    // PCL-CE: 根据 IsLegacy 决定使用 "forge" 还是 "neoforge" 包名
    let package_name = if api_name.contains("1.20.1") { "forge" } else { "neoforge" };

    let official = format!(
        "https://maven.neoforged.net/releases/net/neoforged/{}/{}/{}-{}-installer.jar",
        package_name, api_name, package_name, api_name
    );
    let mirror = format!(
        "https://bmclapi2.bangbang93.com/maven/net/neoforged/{}/{}/{}-{}-installer.jar",
        package_name, api_name, package_name, api_name
    );
    let installer_urls = if prefer_official {
        vec![official, mirror]
    } else {
        vec![mirror, official]
    };

    let mut installer_dl = DownloadFile::new(
        installer_urls,
        installer_path.clone(),
        FileChecker::with_min_size(64 * 1024),
    );

    download_file(app, &mut installer_dl, false).await?;
    log::debug!("[neoforge] installer.jar 已下载");

    // 解压
    let archive_file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|e| format!("无法读取 installer: {}", e))?;

    // 读取 install_profile.json + version.json
    let profile =
        crate::install::merge::read_required_json_entry(&mut archive, "install_profile.json")?;
    let version_json =
        crate::install::merge::read_required_json_entry(&mut archive, "version.json")?;

    // 合并
    let mut merged = profile;
    if let Some(libs) = version_json.get("libraries").cloned() {
        merged["libraries"] = libs;
    }
    if let Some(mc) = version_json.get("mainClass").cloned() {
        merged["mainClass"] = mc;
    }
    if let Some(args) = version_json.get("arguments").cloned() {
        merged["arguments"] = args;
    }
    merged["id"] = serde_json::Value::String(instance_name.to_string());

    // NeoForge 1.21+ 版本可能直接指定 MC 版本而不通过 inheritsFrom
    if merged.get("inheritsFrom").is_none() {
        merged["inheritsFrom"] = serde_json::Value::String(mc_version.to_string());
    }

    // 解析 libraries → 下载
    crate::install::merge::download_json_libraries(app, mc_dir, &merged).await;

    // 提取内嵌 Maven 库
    crate::install::merge::extract_embedded_maven(&mut archive, mc_dir);

    // 保存 JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&merged).unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
    log::info!("[neoforge] 安装完成: {}", instance_name);
    Ok(())
}

