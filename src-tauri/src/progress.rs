//! 下载/安装进度事件 — 全后端唯一的发射入口
//!
//! 历史问题: 同一条 `download-progress` 通道上跑过两套 schema。
//! - download/engine.rs 用 `{batch_id, batch_total, batch_done, file_name,
//!   file_percent, aggregated_percent, step, ...}`
//! - install/、modpack、modrinth、curseforge、instance 各自 emit
//!   `{filename, downloaded, total, percent, step}`
//!
//! 前端任务页按新 schema 解析，收到旧 schema 时 `file_name` 为 undefined，
//! `p.file_name.includes(...)` 直接抛 TypeError，整个监听器中断——表现为
//! 原版安装 / 整合包安装 / 单文件安装在「任务列表」里完全没有进度。
//!
//! 现在统一走这里:
//! - `emit_stage`: 阶段提示（准备 / 解压 / 收尾），没有文件级进度
//! - `emit_file`:  带文件计数的批量进度（整合包逐文件下载）
//! - `next_batch_id`: 批量任务唯一 ID 的唯一分配点，避免不同安装流程撞 ID
//!
//! 事件字段约定见 download/engine.rs 顶部注释；`kind` 字段区分两类事件，
//! 前端据此决定用 `step` 还是用文件进度渲染状态文案。

use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Emitter;

/// 进度事件名 — 前后端共用，改这里等于改协议
pub(crate) const EVENT: &str = "download-progress";

static NEXT_BATCH_ID: AtomicU64 = AtomicU64::new(1);

/// 分配批量任务 ID（进程内单调递增）
pub(crate) fn next_batch_id() -> u64 {
    NEXT_BATCH_ID.fetch_add(1, Ordering::Relaxed)
}

/// 阶段进度 — 只有一句文案和总百分比，没有文件粒度
///
/// `percent` 是整个任务的完成度（0-100），`task` 与前端任务名一致时
/// 会被直接匹配到对应的任务卡片。
pub(crate) fn emit_stage(app: &tauri::AppHandle, task: &str, percent: u32, step: &str) {
    let _ = app.emit(
        EVENT,
        serde_json::json!({
            "kind": "stage",
            "batch_id": 0u64,
            "batch_total": 0u64,
            "batch_done": 0u64,
            "file_name": task,
            "file_percent": percent,
            "file_speed": 0u64,
            "file_downloaded": 0u64,
            "file_total": 0u64,
            "total_bytes": 0u64,
            "total_downloaded": 0u64,
            "aggregated_percent": percent,
            "step": step,
        }),
    );
}

/// 批量任务的逐文件进度
///
/// `aggregated_percent` 由调用方按任务的进度区间换算，保证进度条只向前走。
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_file(
    app: &tauri::AppHandle,
    batch_id: u64,
    batch_total: u64,
    batch_done: u64,
    file_name: &str,
    file_percent: u32,
    aggregated_percent: u32,
    step: &str,
) {
    let _ = app.emit(
        EVENT,
        serde_json::json!({
            "kind": "file",
            "batch_id": batch_id,
            "batch_total": batch_total,
            "batch_done": batch_done,
            "file_name": file_name,
            "file_percent": file_percent,
            "file_speed": 0u64,
            "file_downloaded": 0u64,
            "file_total": 0u64,
            "total_bytes": 0u64,
            "total_downloaded": 0u64,
            "aggregated_percent": aggregated_percent,
            "step": step,
        }),
    );
}
