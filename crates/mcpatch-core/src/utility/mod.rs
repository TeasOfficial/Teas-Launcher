//! 工具函数

pub mod filename_ext;
pub mod partial_read;
pub mod vec_ext;

/// 将字节数转换为人类可读字符串
pub fn convert_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1}{}", size, UNITS[unit_idx])
}

/// 检查是否在 cargo run 下运行
pub fn is_running_under_cargo() -> bool {
    std::env::var("CARGO").is_ok()
}
