//! 系统命令：版本号、系统状态、网络检测、Java 扫描、代理检测

use crate::config::teas_dir;
use crate::http::build_http_client;
use crate::BUILD;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::System;

// ── SystemState：CPU 刷新节流 ────────────────────────────────

/// 包装 sysinfo::System，提供 CPU/内存信息 1 秒缓存避免频繁刷新
pub struct SystemState {
    pub sys: System,
    last_cpu_refresh: Instant,
    cpu_cache: (f32, f32),
}

impl SystemState {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            last_cpu_refresh: Instant::now(),
            cpu_cache: (0.0, 0.0),
        }
    }

    /// 获取 CPU 和内存使用率（1 秒内缓存）
    pub fn get_stats(&mut self) -> (f32, f32) {
        let elapsed = self.last_cpu_refresh.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.sys.refresh_cpu_all();
            self.sys.refresh_memory();
            let cpu = self.sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
                / self.sys.cpus().len() as f32;
            let mem = self.sys.used_memory() as f32 / self.sys.total_memory() as f32 * 100.0;
            self.cpu_cache = (cpu, mem);
            self.last_cpu_refresh = Instant::now();
        }
        self.cpu_cache
    }

    /// 获取内存信息（MB）
    pub fn get_memory_info(&mut self) -> (u64, u64) {
        self.sys.refresh_memory();
        let total = self.sys.total_memory() / 1024 / 1024;
        let free = self.sys.free_memory() / 1024 / 1024;
        (total, free)
    }
}

/// Java 扫描结果持久化缓存文件路径
fn java_cache_path() -> Option<std::path::PathBuf> {
    crate::config::teas_dir().ok().map(|d| d.join("java_scan.json"))
}

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
pub(crate) fn get_system_stats(state: tauri::State<'_, Mutex<SystemState>>) -> (f32, f32) {
    let mut s = state.lock().unwrap();
    s.get_stats()
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
///
/// 首次启动扫描后结果持久化到 `.teas/java_scan.json`，
/// 后续启动直接读取缓存。用户可在设置中手动触发重新扫描。
#[tauri::command]
pub(crate) fn scan_java() -> Vec<serde_json::Value> {
    // ── 1. 检查文件缓存 ──
    if let Some(cache_path) = java_cache_path() {
        if cache_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&cache_path) {
                if let Ok(cached) = serde_json::from_str::<Vec<serde_json::Value>>(&data) {
                    if !cached.is_empty() {
                        return cached;
                    }
                }
            }
        }
    }

    // ── 2. 完整扫描（仅首次启动或缓存失效时执行） ──
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

    // ── 3. 写入持久化缓存 ──
    if let Some(cache_path) = java_cache_path() {
        if let Ok(json) = serde_json::to_string(&result) {
            let _ = std::fs::write(&cache_path, &json);
        }
    }

    result
}

/// 获取系统内存信息 (MB)
#[tauri::command]
pub fn get_memory_info(state: tauri::State<'_, Mutex<SystemState>>) -> (u64, u64) {
    let mut s = state.lock().unwrap();
    s.get_memory_info()
}

/// 自动计算推荐内存分配 (MB)
///
/// 整合 HMCL getAllocatedMemory + PCL GetRam:
/// - 基础需求根据实例类型 + Mod 数量确定 (PCL 风格)
/// - 可用内存分段分配 (PCL 风格阶段分配)
/// - 系统保留 512MB (HMCL 风格)
/// - 上限 16GB (HMCL 风格)
/// - 32-bit Java 上限 ~1.3GB
#[tauri::command]
pub fn calc_auto_memory(
    total_mem_mb: u64,
    available_mem_mb: u64,
    is_64bit_java: bool,
    mod_count: u32,
    has_mod_loader: bool,
) -> u32 {
    let total_gb = total_mem_mb as f64 / 1024.0;
    let mut available_gb = available_mem_mb as f64 / 1024.0;

    // HMCL: 系统保留 512MB
    available_gb -= 0.5;
    if available_gb <= 0.0 {
        return 512; // 最低 512MB
    }

    // PCL: 根据实例类型确定基础内存需求目标
    let (ram_min, ram_t1, ram_t2, ram_t3) = if has_mod_loader {
        // 可安装 Mod 的版本 (Forge/Fabric/NeoForge)
        let mc = mod_count as f64;
        (0.5 + mc / 150.0, 1.5 + mc / 90.0, 2.7 + mc / 50.0, 4.5 + mc / 25.0)
    } else {
        // 普通原版
        (0.5, 1.5, 2.5, 4.0)
    };

    let mut ram_give = 0.0;

    // 阶段一: 0 → T1，100% 分配
    let delta = ram_t1;
    ram_give += f64::min(delta, available_gb);
    available_gb -= delta;
    if available_gb < 0.1 { return (ram_give * 1024.0) as u32; }

    // 阶段二: T1 → T2，70% 分配
    let delta = ram_t2 - ram_t1;
    ram_give += f64::min(available_gb * 0.7, delta);
    available_gb -= delta / 0.7;
    if available_gb < 0.1 { return (ram_give * 1024.0) as u32; }

    // 阶段三: T2 → T3，40% 分配
    let delta = ram_t3 - ram_t2;
    ram_give += f64::min(available_gb * 0.4, delta);
    available_gb -= delta / 0.4;
    if available_gb < 0.1 { return (ram_give * 1024.0) as u32; }

    // 阶段四: T3 → T3*2，15% 分配
    let delta = ram_t3;
    ram_give += f64::min(available_gb * 0.15, delta);

    // HMCL: 上限 16GB
    let ram_cap = 16.0_f64;
    let ram_max = f64::min(ram_cap, total_gb * 0.75);

    // 32-bit Java 上限 ~1.3GB
    let ram_arch_cap = if is_64bit_java { ram_max } else { f64::min(1.3, ram_max) };

    let result = if ram_give < 0.5 { 0.5 } else if ram_give > ram_arch_cap { ram_arch_cap } else { ram_give };
    (result * 1024.0) as u32
}
