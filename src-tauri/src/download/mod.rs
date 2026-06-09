//! 下载引擎 — 参照 PCL-CE IO/Download + Modules/Network 架构
//!
//! 模块:
//! - model:   DownloadFile, FileChecker, 下载状态
//! - source:  4 种 URL 镜像源转换 (参照 DlSource*)
//! - engine:  多线程下载 + 多源回退 + 进度追踪
//! - tracker: 全局下载任务管理

pub mod model;
pub mod source;
pub mod engine;
pub mod tracker;

