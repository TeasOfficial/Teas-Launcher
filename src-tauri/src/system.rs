//! 系统命令：版本号、系统状态、网络检测、Java 扫描、代理检测

use crate::config::teas_dir;
use crate::http::build_http_client;
use crate::BUILD;
use std::sync::Mutex;
use std::time::Duration;
use sysinfo::System;

#[tauri::command]
pub(crate) fn get_version() -> String {
    format!("Teas Launcher v{}", env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
pub(crate) fn get_full_version() -> String {
    format!("v{}.{:04}", env!("CARGO_PKG_VERSION"), BUILD)
}

#[tauri::command]
pub(crate) fn is_dev() -> bool {
    cfg!(dev)
}

#[tauri::command]
pub(crate) fn get_minecraft_dir() -> String {
    #[cfg(dev)]
    {
        r"D:\Minecraft\[000A]".to_string()
    }
    #[cfg(not(dev))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.display().to_string()))
            .unwrap_or_else(|| ".".to_string())
    }
}

#[tauri::command]
pub(crate) fn get_teas_dir() -> Result<String, String> {
    teas_dir().map(|d| d.display().to_string())
}

#[tauri::command]
pub(crate) fn get_system_stats(sys: tauri::State<'_, Mutex<System>>) -> (f32, f32) {
    let mut s = sys.lock().unwrap();
    s.refresh_cpu_all();
    s.refresh_memory();
    let cpu = s.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / s.cpus().len() as f32;
    let mem = s.used_memory() as f32 / s.total_memory() as f32 * 100.0;
    (cpu, mem)
}

#[tauri::command]
pub(crate) async fn check_network() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(5))?;
    let targets: [(&str, &str); 3] = [
        ("Modrinth", "https://api.modrinth.com/v2/tags/game_version"),
        ("CurseForge", "https://api.curseforge.com/v1/games/432"),
        ("Minecraft", "https://session.minecraft.net/"),
    ];
    let mut failures = Vec::new();
    let mut ok = 0u32;
    for (name, url) in &targets {
        match client.head(*url).send().await {
            Ok(rsp) if rsp.status().is_success() || rsp.status().as_u16() == 404 => {
                ok += 1;
            }
            Ok(rsp) => {
                failures.push(format!("{} (HTTP {})", name, rsp.status().as_u16()));
            }
            Err(e) => {
                failures.push(format!("{} ({})", name, e));
            }
        }
    }
    Ok(serde_json::json!({"ok": ok, "total": targets.len(), "failures": failures}))
}

#[tauri::command]
pub(crate) async fn test_proxy_connectivity() -> Result<serde_json::Value, String> {
    let client = build_http_client(Duration::from_secs(8))?;
    let targets = [
        ("Google", "https://www.google.com"),
        ("YouTube", "https://www.youtube.com"),
        ("Modrinth", "https://api.modrinth.com/v2/tags/game_version"),
    ];
    let mut results = Vec::new();
    for (name, url) in &targets {
        let start = std::time::Instant::now();
        match client.head(*url).send().await {
            Ok(r) => {
                let ms = start.elapsed().as_millis();
                results.push(serde_json::json!({
                    "name": name, "ok": true, "status": r.status().as_u16(), "latency_ms": ms
                }));
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "name": name, "ok": false, "error": e.to_string()
                }));
            }
        }
    }
    Ok(serde_json::json!({"results": results}))
}

/// 检测系统代理设置（Windows 注册表）
#[tauri::command]
pub(crate) fn detect_system_proxy() -> Result<serde_json::Value, String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let settings = hkcu
            .open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                KEY_READ,
            )
            .map_err(|e| format!("读取注册表失败: {}", e))?;

        let enabled: u32 = settings.get_value("ProxyEnable").unwrap_or(0);
        if enabled == 0 {
            return Ok(serde_json::json!({"enabled": false}));
        }

        let server: String = settings.get_value("ProxyServer").unwrap_or_default();
        if server.is_empty() {
            return Ok(serde_json::json!({"enabled": false}));
        }

        let (host, port) = if let Some(pos) = server.rfind(':') {
            let h = &server[..pos];
            let p = server[pos + 1..].parse::<u16>().unwrap_or(8080);
            (h.to_string(), p)
        } else {
            (server.clone(), 8080u16)
        };

        Ok(serde_json::json!({
            "enabled": true,
            "proxy_type": "HTTP",
            "host": host,
            "port": port,
        }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(serde_json::json!({"enabled": false}))
    }
}

/// 扫描系统中的 Java 安装
#[tauri::command]
pub(crate) fn scan_java() -> Vec<serde_json::Value> {
    let mut paths = Vec::new();
    for root in &[
        "C:\\Program Files\\Java",
        "C:\\Program Files\\Eclipse Adoptium",
        "C:\\Program Files\\Microsoft",
        "C:\\Program Files\\Zulu",
    ] {
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let jw = p.join("bin").join("javaw.exe");
                    let j = p.join("bin").join("java.exe");
                    if jw.exists() { paths.push(jw.display().to_string()); }
                    else if j.exists() { paths.push(j.display().to_string()); }
                }
            }
        }
    }
    if let Ok(out) = std::process::Command::new("where").arg("javaw").output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let p = line.trim().to_string();
            if !p.is_empty() && !paths.contains(&p) { paths.push(p); }
        }
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.to_lowercase().replace('\\', "/")));
    let mut result = Vec::new();
    for java_path in &paths {
        let version = std::process::Command::new(java_path)
            .arg("-version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()
            .map(|o| {
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                combined
                    .lines()
                    .find(|l| l.contains("version"))
                    .and_then(|l| l.split('\"').nth(1).map(|v| v.to_string()))
                    .unwrap_or_else(|| "未知".to_string())
            })
            .unwrap_or_else(|| "未知".to_string());
        result.push(serde_json::json!({"path": java_path, "version": version}));
    }
    result
}
