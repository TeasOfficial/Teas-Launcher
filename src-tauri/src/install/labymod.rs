//! LabyMod 安装 — 参照 PCL-CE McDownloadLabyModLoader
//!
//! 流程:
//! 1. 获取 LabyMod manifest
//! 2. 下载 assets
//! 3. 创建 LabyMod JSON

use std::path::Path;

#[allow(dead_code)]
pub async fn install_labymod(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    labymod_channel: &str, // "production" or "snapshot"
    labymod_commit_ref: &str,
) -> Result<(), String> {
    let client = crate::http::build_http_client(std::time::Duration::from_secs(60))?;

    // 获取 manifest
    let manifest_url = format!(
        "https://releases.r2.labymod.net/api/v1/manifest/{}/latest.json",
        labymod_channel
    );
    let resp = client.get(&manifest_url).send().await.map_err(|e| e.to_string())?;
    let manifest: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    // 下载 assets
    let assets = manifest.get("assets").and_then(|a| a.as_object());
    if let Some(assets_obj) = assets {
        let laby_dir = mc_dir.join("labymod-neo");
        std::fs::create_dir_all(laby_dir.join("assets")).map_err(|e| e.to_string())?;

        for (asset_name, sha1_val) in assets_obj {
            let sha1 = sha1_val.as_str().unwrap_or("");
            let asset_path = laby_dir
                .join("assets")
                .join(format!("{}.jar", asset_name));
            let asset_url = format!(
                "https://releases.r2.labymod.net/api/v1/download/assets/labymod4/{}/{}/{}/{}.jar",
                labymod_channel, labymod_commit_ref, asset_name, sha1
            );

            let checker = crate::download::model::FileChecker::new(-1, -1, Some(sha1.to_string()));
            if checker.check(&asset_path).is_some() {
                let mut dl = crate::download::model::DownloadFile::new(
                    vec![asset_url],
                    asset_path,
                    checker,
                );
                let _ = crate::download::engine::download_file(app, &mut dl, false).await;
            }
        }
    }

    // 创建实例 JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    let ver_json = serde_json::json!({
        "id": instance_name,
        "inheritsFrom": mc_version,
        "mainClass": "net.minecraft.launchwrapper.Launch",
        "type": "release",
        "labymod_data": {
            "channelType": labymod_channel,
            "commitReference": labymod_commit_ref
        }
    });

    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&ver_json).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
