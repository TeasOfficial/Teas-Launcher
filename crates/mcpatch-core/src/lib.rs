//! McPatch 核心库
//!
//! 提供 Minecraft 模组包的增量更新功能，支持：
//! - 多协议下载（HTTP/HTTPS、WebDAV、私有协议、Alist 网盘）
//! - 版本比对与增量更新
//! - 文件哈希校验（CRC64 + CRC16）
//! - 断点续传与多源容错

pub mod common;
pub mod data;
pub mod error;
pub mod log;
pub mod network;
pub mod speed_sampler;
pub mod utility;

/// 返回当前 McPatch 核心库的版本号
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 检查更新（占位实现，后续从 work.rs 迁移完整逻辑）
pub async fn check_update() -> Result<String, String> {
    Ok("更新检查功能已就绪（McPatch 核心已集成）".to_string())
}

// 重新导出核心类型
pub use error::{BusinessError, BusinessResult};
