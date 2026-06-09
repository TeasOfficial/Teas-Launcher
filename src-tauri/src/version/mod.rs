//! 版本与文件管理 — 参照 PCL-CE Minecraft/ 下的版本管理逻辑
//!
//! 模块:
//! - library:  Library 管理 (McLibToken, Maven坐标, 规则检查)
//! - assets:   Asset/资源文件管理
//! - manifest: 版本清单获取 (Minecraft/Forge/NeoForge/Fabric/Quilt/OptiFine...)

pub mod library;
pub mod assets;
pub mod manifest;
