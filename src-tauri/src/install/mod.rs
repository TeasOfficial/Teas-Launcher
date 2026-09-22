//! 游戏安装引擎 — 参照 PCL-CE ModDownloadLib.cs 安装流程
//!
//! 支持的安装类型:
//! - vanilla:     原版 Minecraft
//! - forge:        Forge (Legacy + New)
//! - neoforge:     NeoForge
//! - fabric:       Fabric Loader
//! - quilt:        Quilt Loader
//! - cleanroom:    Cleanroom
//!
//! 合并安装: merge.rs — 多 loader 同时安装时合并 JSON

pub mod vanilla;
pub mod forge;
pub mod neoforge;
pub mod fabric;
pub mod quilt;
pub mod cleanroom;
pub mod merge;
