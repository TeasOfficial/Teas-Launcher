//! OptiFine 安装 — 参照 PCL-CE McDownloadOptiFineLoader
//!
//! 安装方式:
//! 1. 作为 Mod: 下载 JAR → 放入 mods/ 目录
//! 2. 作为独立实例: 下载 JAR → libraries/optifine/ + 创建 inheritsFrom JSON

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use std::path::Path;

#[allow(dead_code)]
pub async fn install_optifine_as_mod(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    optifine_name: &str, // e.g., "OptiFine_1.21.1_HD_U_J3"
) -> Result<(), String> {
    let mods_dir = mc_dir.join("versions").join(instance_name).join("mods");
    std::fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;

    let file_name = format!("{}.jar", optifine_name);
    let dest = mods_dir.join(&file_name);

    // BMCLAPI OptiFine URL
    let bmcl_url = format!(
        "https://bmclapi2.bangbang93.com/optifine/{}/{}/{}",
        mc_version, optifine_name, file_name
    );

    let mut dl = DownloadFile::new(
        vec![bmcl_url],
        dest,
        FileChecker::with_min_size(1024),
    );

    download_file(app, &mut dl, false).await?;
    eprintln!("[optifine] Mod 已安装: {}", file_name);
    Ok(())
}
