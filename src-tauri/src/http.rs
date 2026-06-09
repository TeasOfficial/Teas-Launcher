//! HTTP 客户端、代理、下载源镜像
//! 下载引擎已移至 download/ 模块

use crate::config::config_read;
use std::time::Duration;

static CANCEL_DOWNLOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub(crate) fn cancel_download() {
    CANCEL_DOWNLOAD.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn reset_cancel() {
    CANCEL_DOWNLOAD.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn is_cancelled() -> bool {
    CANCEL_DOWNLOAD.load(std::sync::atomic::Ordering::SeqCst)
}

/// 根据用户设置转换下载 URL（镜像源）
/// 保留向后兼容，新代码应使用 download::source 模块
pub(crate) fn apply_source(url: &str, source: &str) -> String {
    match source {
        "BMCLAPI" => url
            .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com")
            .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com")
            .replace("https://launcher.mojang.com", "https://bmclapi2.bangbang93.com")
            .replace("https://launchermeta.mojang.com", "https://bmclapi2.bangbang93.com")
            .replace("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/maven")
            .replace("https://resources.download.minecraft.net", "https://bmclapi2.bangbang93.com/assets")
            .replace("https://maven.minecraftforge.net", "https://bmclapi2.bangbang93.com/maven")
            .replace("https://maven.neoforged.net/releases", "https://bmclapi2.bangbang93.com/maven"),
        "MCIMirror" => url
            .replace("https://api.modrinth.com/v2/", "https://mod.mcimirror.top/modrinth/v2/")
            .replace("https://cdn.modrinth.com/data/", "https://mod.mcimirror.top/data/")
            .replace("https://api.curseforge.com/v1/", "https://mod.mcimirror.top/curseforge/v1/"),
        _ => url.to_string(),
    }
}

/// 从用户配置读取代理设置，创建 reqwest 客户端
pub(crate) fn build_http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    let cfg: serde_json::Value =
        config_read("user".to_string()).unwrap_or(serde_json::json!({}));
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout);

    if let Some(proxy_type) = cfg.get("proxy_type").and_then(|v| v.as_str()) {
        if proxy_type != "None" && !proxy_type.is_empty() {
            if let (Some(host), Some(port)) = (
                cfg.get("proxy_host").and_then(|v| v.as_str()),
                cfg.get("proxy_port")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u16>().ok()),
            ) {
                let scheme = match proxy_type {
                    "SOCKS5" => "socks5",
                    _ => "http",
                };
                let proxy_url = format!("{}://{}:{}", scheme, host, port);
                let mut proxy =
                    reqwest::Proxy::all(&proxy_url).map_err(|e| format!("代理配置失败: {}", e))?;
                if let (Some(user), Some(pass)) = (
                    cfg.get("proxy_user")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty()),
                    cfg.get("proxy_pass")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty()),
                ) {
                    proxy = proxy.basic_auth(user, pass);
                }
                builder = builder.proxy(proxy);
            }
        }
    }

    builder.build().map_err(|e| e.to_string())
}
