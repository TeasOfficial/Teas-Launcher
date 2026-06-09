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
) -> Result<(), String> {
    let tmp_dir = mc_dir.join(format!(".tmp_nfg_{}", instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let installer_path = tmp_dir.join("neoforge_installer.jar");

    // 确定 api_name (如 "20.4.30-beta" 或 "1.20.1-47.1.99")
    let api_name = neoforge_version;

    // PCL-CE: 根据 IsLegacy 决定使用 "forge" 还是 "neoforge" 包名
    let package_name = if api_name.contains("1.20.1") { "forge" } else { "neoforge" };

    let installer_urls = vec![
        format!(
            "https://bmclapi2.bangbang93.com/maven/net/neoforged/{}/{}/{}-{}-installer.jar",
            package_name, api_name, package_name, api_name
        ),
        format!(
            "https://maven.neoforged.net/releases/net/neoforged/{}/{}/{}-{}-installer.jar",
            package_name, api_name, package_name, api_name
        ),
    ];

    let mut installer_dl = DownloadFile::new(
        installer_urls,
        installer_path.clone(),
        FileChecker::with_min_size(64 * 1024),
    );

    download_file(app, &mut installer_dl, false).await?;
    eprintln!("[neoforge] installer.jar 已下载");

    // 解压
    let archive_file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|e| format!("无法读取 installer: {}", e))?;

    // 读取 install_profile.json + version.json
    let profile = read_json_from_archive(&mut archive, "install_profile.json")?;
    let version_json = read_json_from_archive(&mut archive, "version.json")?;

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
    let libs = crate::version::library::mclib_list_from_json(&merged, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    // 提取内嵌 Maven 库
    extract_neo_embedded_libs(&mut archive, mc_dir);

    // 保存 JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&merged).unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
    eprintln!("[neoforge] 安装完成: {}", instance_name);
    Ok(())
}

fn read_json_from_archive(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<serde_json::Value, String> {
    let entry = archive
        .by_name(name)
        .map_err(|_| format!("installer 中无 {}", name))?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::BufReader::new(entry), &mut s)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

fn extract_neo_embedded_libs(
    archive: &mut zip::ZipArchive<std::fs::File>,
    mc_dir: &Path,
) {
    let libs_dir = mc_dir.join("libraries");
    // 收集所有需要提取的条目
    let entries: Vec<(String, std::path::PathBuf)> = {
        let mut result = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.starts_with("maven/") && !name.ends_with('/') {
                    let rel = name.trim_start_matches("maven/");
                    let dest = libs_dir.join(rel);
                    if !dest.exists() {
                        result.push((name, dest));
                    }
                }
            }
        }
        result
    };

    for (name, dest) in entries {
        if let Some(p) = dest.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Ok(mut e) = archive.by_name(&name) {
            if let Ok(mut out) = std::fs::File::create(&dest) {
                let _ = std::io::copy(&mut e, &mut out);
            }
        }
    }
}
