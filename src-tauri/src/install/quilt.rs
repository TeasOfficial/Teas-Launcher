//! Quilt Loader 安装
//!
//! 安装流程与 Fabric 完全一致（同一套 merge 原语），实现在
//! `install/merge.rs::install_from_profile`，这里只负责拼 Quilt 的 meta 地址。

use std::path::Path;

pub async fn install_quilt_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    quilt_version: &str,
) -> Result<(), String> {
    let url = format!(
        "https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json",
        mc_version, quilt_version
    );
    crate::install::merge::install_from_profile(
        app, mc_dir, instance_name, mc_version, &url, "quilt",
    )
    .await
}