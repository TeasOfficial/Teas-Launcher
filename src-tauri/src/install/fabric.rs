//! Fabric Loader 安装
//!
//! 安装流程与 Quilt 完全一致（同一套 merge 原语），实现在
//! `install/merge.rs::install_from_profile`，这里只负责拼 Fabric 的 meta 地址。

use std::path::Path;

pub async fn install_fabric_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    fabric_version: &str,
) -> Result<(), String> {
    let url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
        mc_version, fabric_version
    );
    crate::install::merge::install_from_profile(
        app, mc_dir, instance_name, mc_version, &url, "fabric",
    )
    .await
}