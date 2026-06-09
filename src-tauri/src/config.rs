//! 配置持久化：tc.ini 读写、.teas 目录

/// 获取 .teas 目录（dev 模式指向 D:\Minecraft\[000A]\.teas）
pub(crate) fn teas_dir() -> Result<std::path::PathBuf, String> {
    #[cfg(dev)]
    let base: std::path::PathBuf = r"D:\Minecraft\[000A]".into();
    #[cfg(not(dev))]
    let base = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("无法获取 exe 目录")?
        .to_path_buf();
    let dir = base.join(".teas");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[tauri::command]
pub(crate) fn config_read(scope: String) -> Result<serde_json::Value, String> {
    let path = if scope == "launcher" {
        teas_dir()?.join("tc.ini")
    } else {
        let d = dirs::config_dir().ok_or("?")?.join("TeasLauncher");
        std::fs::create_dir_all(&d).ok();
        d.join("tc.ini")
    };
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let c = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&c).map_err(|e| format!("解析失败: {}", e))
}

#[tauri::command]
pub(crate) fn config_write(
    scope: String,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let path = if scope == "launcher" {
        teas_dir()?.join("tc.ini")
    } else {
        let d = dirs::config_dir().ok_or("?")?.join("TeasLauncher");
        std::fs::create_dir_all(&d).ok();
        d.join("tc.ini")
    };
    let mut cfg: serde_json::Value = if path.exists() {
        serde_json::from_str(
            &std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
        )
        .unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert(key, value);
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
