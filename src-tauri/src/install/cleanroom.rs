//! Cleanroom 安装 — 参照 PCL-CE McDownloadForgelikeLoader (Cleanroom 分支)
//!
//! Cleanroom 是 1.12.2 的加载器，安装流程同新版 Forge

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use std::path::Path;

#[allow(dead_code)]
pub async fn install_cleanroom(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    cleanroom_version: &str,
) -> Result<(), String> {
    let tmp_dir = mc_dir.join(format!(".tmp_clr_{}", instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let installer_path = tmp_dir.join("cleanroom_installer.jar");
    let installer_url = format!(
        "https://github.com/CleanroomMC/Cleanroom/releases/download/{}/cleanroom-{}-installer.jar",
        cleanroom_version, cleanroom_version
    );

    let mut dl = DownloadFile::new(
        vec![installer_url],
        installer_path.clone(),
        FileChecker::with_min_size(64 * 1024),
    );
    download_file(app, &mut dl, false).await?;

    // 解压 installer → 提取 version.json
    let archive_file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|e| format!("无法读取 installer: {}", e))?;

    // 读取 version.json
    let version_json = match archive.by_name("version.json") {
        Ok(mut entry) => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s)
                .map_err(|e| e.to_string())?;
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(v) => Some(v),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(_) => None,
    };

    // 如果有 install_profile.json 也读
    let profile = match archive.by_name("install_profile.json") {
        Ok(mut entry) => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s)
                .map_err(|e| e.to_string())?;
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(v) => Some(v),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(_) => None,
    };

    let mut merged = profile.unwrap_or(version_json.clone().unwrap_or(serde_json::json!({})));
    if let Some(vj) = &version_json {
        if let Some(libs) = vj.get("libraries").cloned() { merged["libraries"] = libs; }
        if let Some(mc) = vj.get("mainClass").cloned() { merged["mainClass"] = mc; }
        if let Some(args) = vj.get("arguments").cloned() { merged["arguments"] = args; }
    }
    merged["id"] = serde_json::Value::String(instance_name.to_string());
    merged["inheritsFrom"] = serde_json::Value::String("1.12.2".to_string());

    // 下载 libraries
    let libs = crate::version::library::mclib_list_from_json(&merged, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    // 保存 JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&merged).unwrap_or_default(),
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}
