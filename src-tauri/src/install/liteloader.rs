//! LiteLoader 安装 — 参照 PCL-CE McDownloadLiteLoaderLoader
//!
//! 流程:
//! 1. 下载 installer JAR → 放入 libraries/com/mumfrey/liteloader/
//! 2. 创建 inheritsFrom JSON 指向原版 MC 版本

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::install::helpers::create_inherits_json;
use std::path::Path;

#[allow(dead_code)]
pub async fn install_liteloader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    liteloader_filename: &str,
) -> Result<(), String> {
    let _mc_num: f64 = mc_version
        .split('.')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let jar_url = format!(
        "https://bmclapi2.bangbang93.com/maven/com/mumfrey/liteloader/{}/{}/{}",
        mc_version, mc_version, liteloader_filename
    );

    let lib_path = mc_dir
        .join("libraries")
        .join("com")
        .join("mumfrey")
        .join("liteloader")
        .join(mc_version)
        .join(liteloader_filename);

    let mut dl = DownloadFile::new(vec![jar_url], lib_path, FileChecker::with_min_size(1024));
    download_file(app, &mut dl, false).await?;

    // 创建 inheritsFrom JSON
    create_inherits_json(mc_dir, instance_name, mc_version)?;

    Ok(())
}
