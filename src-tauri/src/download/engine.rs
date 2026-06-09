//! 下载引擎 — 参照 PCL-CE FileDownloader.cs + LoaderDownload.cs
//!
//! 核心逻辑:
//! - download_file: 单文件多源回退下载 (3 次重试)
//! - download_files_parallel: 并行批量下载 (信号量控制)
//! - 进度通过 Tauri event "download-progress" 发射
//! - 支持全局取消标志
//!
//! 进度事件格式:
//! {
//!   "batch_id": u64,        // 批量任务唯一 ID，用于前端聚合
//!   "batch_total": u64,     // 批量任务总文件数
//!   "batch_done": u64,      // 已完成文件数 (含跳过)
//!   "file_name": str,       // 当前文件名
//!   "file_percent": u32,    // 当前文件进度 (0-100)
//!   "file_speed": u64,      // 当前文件速度 (B/s)
//!   "file_downloaded": u64, // 当前文件已下载字节
//!   "file_total": u64,      // 当前文件总字节
//!   "total_bytes": u64,     // 批量任务总字节 (所有文件)
//!   "total_downloaded": u64,// 批量任务已下载总字节
//!   "step": str,            // 人类可读的描述
//! }

use super::model::{DownloadFile, DownloadState, FileChecker};
use crate::config::config_read;
use crate::http::{build_http_client, is_cancelled};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Emitter;

/// 批量下载结果
#[derive(Debug)]
pub struct DownloadResult {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize, // 文件已存在且通过校验，跳过
    #[allow(dead_code)]
    pub errors: Vec<String>,
}

/// 全局下载速度统计
#[allow(dead_code)]
static GLOBAL_SPEED: AtomicU64 = AtomicU64::new(0);
#[allow(dead_code)]
static GLOBAL_ACTIVE: AtomicU64 = AtomicU64::new(0);

#[allow(dead_code)]
pub fn global_speed() -> u64 {
    GLOBAL_SPEED.load(Ordering::Relaxed)
}

#[allow(dead_code)]
pub fn global_active() -> u64 {
    GLOBAL_ACTIVE.load(Ordering::Relaxed)
}

/// 批量下载进度追踪器 — 跨线程共享
///
/// 前端使用 `batch_id` 将多次文件下载事件聚合到同一个任务卡片上，
/// 避免进度条在多个文件之间来回跳动。
struct BatchProgress {
    batch_id: u64,
    total_files: u64,
    done_files: AtomicU64,
    total_bytes: AtomicU64,
    downloaded_bytes: AtomicU64,
    /// 各个活跃文件的当前速度 (B/s)，用于计算总速度
    active_speeds: std::sync::Mutex<Vec<u64>>,
}

impl BatchProgress {
    fn emit(&self, app: &tauri::AppHandle, file_name: &str, file_percent: u32,
            file_speed: u64, file_downloaded: u64, file_total: u64, step: &str) {
        let done = self.done_files.load(Ordering::Relaxed);
        let total_dl = self.downloaded_bytes.load(Ordering::Relaxed);
        let total_all = self.total_bytes.load(Ordering::Relaxed);

        // 更新活跃速度列表
        {
            let mut speeds = self.active_speeds.lock().unwrap();
            // 清理已完成文件的速度 (speed=0 表示文件已完成)
            speeds.retain(|s| *s > 0);
            if file_speed > 0 && !speeds.contains(&file_speed) {
                // 用文件名哈希做简单去重
                speeds.push(file_speed);
            }
        }

        let aggregated_percent = if total_all > 0 {
            ((total_dl as f64 / total_all as f64) * 100.0).min(99.0) as u32
        } else if self.total_files > 0 {
            // 没有大小信息时按文件数量算
            ((done as f64 / self.total_files as f64) * 100.0).min(99.0) as u32
        } else {
            0
        };

        let _ = app.emit("download-progress", serde_json::json!({
            "batch_id": self.batch_id,
            "batch_total": self.total_files,
            "batch_done": done,
            "file_name": file_name,
            "file_percent": file_percent,
            "file_speed": file_speed,
            "file_downloaded": file_downloaded,
            "file_total": file_total,
            "total_bytes": total_all,
            "total_downloaded": total_dl,
            "aggregated_percent": aggregated_percent,
            "step": step,
        }));
    }
}

static NEXT_BATCH_ID: AtomicU64 = AtomicU64::new(1);

fn next_batch_id() -> u64 {
    NEXT_BATCH_ID.fetch_add(1, Ordering::Relaxed)
}

/// 单文件下载 — 多源回退，3次重试
///
/// 参照 PCL-CE FileDownloader.Download():
/// ```csharp
/// foreach url in Urls:
///     try DownloadSingleAsync(url) → return
///     catch → cleanup, next url
/// throw "所有源失败"
/// ```
///
/// LoaderDownload.ProcessFileAsync():
/// ```csharp
/// if file.Check.canUseExistsFile && Check(file.LocalPath) == null:
///     skip
/// for retry in 0..3:
///     try FileDownloader.Download() → break
///     catch → sleep(300~800ms), retry
/// ```
pub async fn download_file(
    app: &tauri::AppHandle,
    file: &mut DownloadFile,
    enable_parallel_chunks: bool,
) -> Result<(), String> {
    // 1) 如果文件已存在且通过校验，直接跳过
    if file.check.can_use_exists {
        if file.check.check(&file.local_path).is_none() {
            file.state = DownloadState::Finished;
            if let Ok(meta) = std::fs::metadata(&file.local_path) {
                file.total_size = meta.len() as i64;
                file.downloaded = meta.len() as u64;
            }
            file.mark_finished();
            eprintln!("[DL] 跳过 (已存在): {}", file.file_name());
            return Ok(());
        }
    }

    // 2) 确保目标目录存在
    if let Some(parent) = file.local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}: {}", parent.display(), e))?;
    }

    // 3) 清理可能的残留临时文件
    cleanup_temp(&file.local_path);

    // 4) 遍历所有 URL，每个最多重试 3 次
    file.state = DownloadState::Connecting;
    GLOBAL_ACTIVE.fetch_add(1, Ordering::Relaxed);

    // 清除 URL 列表中的空格，过滤空 URL
    let urls: Vec<String> = file
        .urls
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();

    if urls.is_empty() {
        GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        file.mark_failed("未提供可用的下载地址".to_string());
        return Err("未提供可用的下载地址".to_string());
    }

    let mut last_error = String::new();

    for url in &urls {
        eprintln!("[DL] 尝试: {} -> {}", url, file.local_path.display());

        // 每个源最多重试 3 次 (参照 PCL-CE: 4次总共 = 1次初始 + 3次重试)
        for retry in 0u32..4 {
            if is_cancelled() {
                GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                file.state = DownloadState::Interrupted;
                return Err("已取消".to_string());
            }

            let result = download_single(
                app,
                url,
                &file.local_path,
                file.use_browser_ua,
                file.custom_ua.as_deref(),
                enable_parallel_chunks,
            )
            .await;

            match result {
                Ok(size) => {
                    file.downloaded = size;
                    file.total_size = size as i64;
                    file.speed = 0;
                    file.active_threads = 0;
                    file.state = DownloadState::Verifying;

                    // 下载后校验
                    if let Some(err) = file.check.check(&file.local_path) {
                        eprintln!(
                            "[DL] 校验失败: {} ({})",
                            file.file_name(),
                            err
                        );
                        cleanup_temp(&file.local_path);
                        last_error = format!("校验失败: {}", err);
                        continue; // 尝试下一个 URL
                    }

                    file.mark_finished();
                    GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                    eprintln!("[DL] 成功: {} ({} bytes)", file.file_name(), size);
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("[DL] 失败 (重试 {}/3): {}", retry, e);
                    cleanup_temp(&file.local_path);
                    last_error = e;

                    if retry < 3 {
                        // 参照 PCL-CE: Thread.Sleep(300 + retry * 300)
                        let delay = 300 + retry as u64 * 300;
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }
    }

    GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
    file.mark_failed(last_error.clone());
    Err(format!("{}: 所有下载源均不可用: {}", file.file_name(), last_error))
}

/// 单 URL 下载 (无重试)
async fn download_single(
    app: &tauri::AppHandle,
    url: &str,
    local_path: &PathBuf,
    _use_browser_ua: bool,
    _custom_ua: Option<&str>,
    _enable_parallel_chunks: bool,
) -> Result<u64, String> {
    let client = build_http_client(Duration::from_secs(600))?;
    let start = Instant::now();

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    eprintln!("[DL] 响应: total={}, url={}", total, url);

    // 流式下载 + 进度追踪
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    let mut last_bytes: u64 = 0;

    // 写入临时文件
    let temp_path = local_path.with_extension("dlpart");
    let mut file = std::fs::File::create(&temp_path)
        .map_err(|e| format!("无法创建临时文件: {}", e))?;

    use std::io::Write;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取失败: {}", e))?;

        if is_cancelled() {
            let _ = std::fs::remove_file(&temp_path);
            return Err("已取消".to_string());
        }

        file.write_all(&chunk)
            .map_err(|e| format!("写入失败: {}", e))?;
        downloaded += chunk.len() as u64;

        // 每 100ms 发射一次进度 (参照 PCL-CE: 事件驱动)
        let elapsed = last_emit.elapsed();
        if elapsed >= Duration::from_millis(100) || downloaded >= total {
            let speed = if elapsed.as_secs_f64() > 0.0 {
                ((downloaded - last_bytes) as f64 / elapsed.as_secs_f64()) as u64
            } else {
                0
            };
            GLOBAL_SPEED.store(speed, Ordering::Relaxed);

            let percent = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0).min(99.0) as u32
            } else {
                0
            };

            let file_name = local_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let _ = app.emit(
                "download-progress",
                serde_json::json!({
                    "batch_id": 0u64,
                    "batch_total": 1u64,
                    "batch_done": 0u64,
                    "file_name": file_name,
                    "file_percent": percent,
                    "file_speed": speed,
                    "file_downloaded": downloaded,
                    "file_total": total,
                    "total_bytes": total,
                    "total_downloaded": downloaded,
                    "aggregated_percent": percent,
                    "step": format!("下载中 {}%", percent),
                }),
            );

            last_emit = Instant::now();
            last_bytes = downloaded;
        }
    }

    // 确保数据写入磁盘
    file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    drop(file);

    // 原子重命名
    if local_path.exists() {
        std::fs::remove_file(local_path).ok();
    }
    std::fs::rename(&temp_path, local_path)
        .map_err(|e| format!("重命名失败: {}", e))?;

    let elapsed_total = start.elapsed();
    let avg_speed = if elapsed_total.as_secs_f64() > 0.0 {
        (downloaded as f64 / elapsed_total.as_secs_f64()) as u64
    } else {
        0
    };
    eprintln!(
        "[DL] 单文件完成: {} bytes, {}s, {} B/s avg",
        downloaded,
        elapsed_total.as_secs_f64(),
        avg_speed
    );

    Ok(downloaded)
}

/// 清理临时文件
fn cleanup_temp(path: &PathBuf) {
    let temp_path = path.with_extension("dlpart");
    for _ in 0..5 {
        match std::fs::remove_file(&temp_path) {
            Ok(_) => break,
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// 并行批量下载 — 信号量控制并发数
///
/// 使用 BatchProgress 发射聚合进度事件，前端按 batch_id 聚合，
/// 进度条平滑递增不再反复横跳。
pub async fn download_files_parallel(
    app: &tauri::AppHandle,
    files: &mut [DownloadFile],
    max_threads: usize,
) -> DownloadResult {
    let total = files.len();
    if total == 0 {
        return DownloadResult {
            total: 0, success: 0, failed: 0, skipped: 0,
            errors: Vec::new(),
        };
    }

    let dl_threads = config_read("user".to_string())
        .ok()
        .and_then(|c| c["dl_threads"].as_str().and_then(|s| s.parse::<usize>().ok()))
        .unwrap_or(max_threads)
        .max(1)
        .min(64);

    eprintln!("[DL] 批量下载 {} 个文件 ({} 线程)", total, dl_threads);

    // ── 计算总大小 ──
    let total_bytes: u64 = files.iter()
        .map(|f| f.check.actual_size.max(0) as u64)
        .sum();

    // ── 共享进度结构 ──
    let batch = Arc::new(BatchProgress {
        batch_id: next_batch_id(),
        total_files: total as u64,
        done_files: AtomicU64::new(0),
        total_bytes: AtomicU64::new(total_bytes),
        downloaded_bytes: AtomicU64::new(0),
        active_speeds: std::sync::Mutex::new(Vec::new()),
    });

    let sem = Arc::new(tokio::sync::Semaphore::new(dl_threads));
    let success_count = Arc::new(AtomicU64::new(0));
    let failed_count = Arc::new(AtomicU64::new(0));
    let skipped_count = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();
    let file_count = files.len();

    for idx in 0..file_count {
        if is_cancelled() {
            break;
        }

        let a = app.clone();
        let s = sem.clone();
        let batch = batch.clone();
        let sc = success_count.clone();
        let fc = failed_count.clone();
        let sk = skipped_count.clone();

        let urls = files[idx].urls.clone();
        let local_path = files[idx].local_path.clone();
        let check = files[idx].check.clone();
        let use_browser_ua = files[idx].use_browser_ua;
        let custom_ua = files[idx].custom_ua.clone();
        let can_use_exists = files[idx].check.can_use_exists;
        let file_size = check.actual_size.max(0) as u64;

        handles.push(tokio::spawn(async move {
            let _permit = s.acquire().await.unwrap();
            if is_cancelled() {
                return;
            }

            let file_name = local_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // ── 跳过已存在且有效的文件 ──
            if can_use_exists && check.check(&local_path).is_none() {
                sk.fetch_add(1, Ordering::Relaxed);
                batch.downloaded_bytes.fetch_add(file_size, Ordering::Relaxed);
                let _done = batch.done_files.fetch_add(1, Ordering::Relaxed) + 1;
                batch.emit(&a, &file_name, 100, 0, file_size, file_size,
                    &format!("跳过 (已存在)"));
                return;
            }

            // ── 下载单个文件 ──
            let mut df = DownloadFile::new(urls, local_path.clone(), check);
            df.use_browser_ua = use_browser_ua;
            df.custom_ua = custom_ua;
            df.check.can_use_exists = can_use_exists;

            let result = download_single_with_batch(
                &a, &mut df, false, &batch, &file_name,
            ).await;

            match result {
                Ok(bytes) => {
                    sc.fetch_add(1, Ordering::Relaxed);
                    batch.downloaded_bytes.fetch_add(bytes, Ordering::Relaxed);
                }
                Err(e) => {
                    fc.fetch_add(1, Ordering::Relaxed);
                    eprintln!("[DL] 批量-失败: {} ({})", file_name, e);
                }
            }

            let _done = batch.done_files.fetch_add(1, Ordering::Relaxed) + 1;
            batch.emit(&a, &file_name, 100, 0, file_size, file_size,
                &format!("完成 ({}/{})", _done, total));
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let result = DownloadResult {
        total,
        success: success_count.load(Ordering::Relaxed) as usize,
        failed: failed_count.load(Ordering::Relaxed) as usize,
        skipped: skipped_count.load(Ordering::Relaxed) as usize,
        errors: Vec::new(),
    };

    eprintln!(
        "[DL] 批量完成: {}/{} 成功, {} 跳过, {} 失败",
        result.success, result.total, result.skipped, result.failed
    );
    result
}

/// 批量模式下的单文件下载 — 在下载过程中实时向 BatchProgress 报告进度
async fn download_single_with_batch(
    app: &tauri::AppHandle,
    file: &mut DownloadFile,
    enable_parallel_chunks: bool,
    batch: &BatchProgress,
    display_name: &str,
) -> Result<u64, String> {
    // 跳过已存在的文件
    if file.check.can_use_exists {
        if file.check.check(&file.local_path).is_none() {
            file.state = DownloadState::Finished;
            file.mark_finished();
            return Ok(file.total_size as u64);
        }
    }

    if let Some(parent) = file.local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}: {}", parent.display(), e))?;
    }

    cleanup_temp(&file.local_path);

    file.state = DownloadState::Connecting;
    GLOBAL_ACTIVE.fetch_add(1, Ordering::Relaxed);

    let urls: Vec<String> = file.urls.iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();

    if urls.is_empty() {
        GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
        file.mark_failed("未提供可用的下载地址".to_string());
        return Err("未提供可用的下载地址".to_string());
    }

    let mut last_error = String::new();

    for url in &urls {
        for retry in 0u32..4 {
            if is_cancelled() {
                GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                file.state = DownloadState::Interrupted;
                return Err("已取消".to_string());
            }

            let result = download_single_with_batch_progress(
                app, url, &file.local_path, batch, display_name, file.check.actual_size.max(0) as u64,
            ).await;

            match result {
                Ok(size) => {
                    file.downloaded = size;
                    file.total_size = size as i64;
                    file.state = DownloadState::Verifying;

                    if let Some(err) = file.check.check(&file.local_path) {
                        eprintln!("[DL] 校验失败: {} ({})", file.file_name(), err);
                        cleanup_temp(&file.local_path);
                        last_error = format!("校验失败: {}", err);
                        continue;
                    }

                    file.mark_finished();
                    GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                    return Ok(size);
                }
                Err(e) => {
                    eprintln!("[DL] 失败 (重试 {}/3): {}", retry, e);
                    cleanup_temp(&file.local_path);
                    last_error = e;
                    if retry < 3 {
                        let delay = 300 + retry as u64 * 300;
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }
    }

    GLOBAL_ACTIVE.fetch_sub(1, Ordering::Relaxed);
    file.mark_failed(last_error.clone());
    Err(format!("{}: 所有下载源均不可用: {}", file.file_name(), last_error))
}

/// 带批量进度报告的流式下载 (对应 download_single)
async fn download_single_with_batch_progress(
    app: &tauri::AppHandle,
    url: &str,
    local_path: &PathBuf,
    batch: &BatchProgress,
    display_name: &str,
    file_total: u64,
) -> Result<u64, String> {
    let client = build_http_client(Duration::from_secs(600))?;

    let resp = client.get(url).send().await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let content_length = resp.content_length().unwrap_or(file_total);
    eprintln!("[DL] 响应: total={}, url={}", content_length, url);

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    let mut last_bytes: u64 = 0;

    let temp_path = local_path.with_extension("dlpart");
    let mut file = std::fs::File::create(&temp_path)
        .map_err(|e| format!("无法创建临时文件: {}", e))?;

    use std::io::Write;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取失败: {}", e))?;

        if is_cancelled() {
            let _ = std::fs::remove_file(&temp_path);
            return Err("已取消".to_string());
        }

        file.write_all(&chunk).map_err(|e| format!("写入失败: {}", e))?;
        downloaded += chunk.len() as u64;

        let elapsed = last_emit.elapsed();
        if elapsed >= Duration::from_millis(100) || downloaded >= content_length {
            let speed = if elapsed.as_secs_f64() > 0.0 {
                ((downloaded - last_bytes) as f64 / elapsed.as_secs_f64()) as u64
            } else { 0 };

            let file_percent = if content_length > 0 {
                ((downloaded as f64 / content_length as f64) * 100.0).min(99.0) as u32
            } else { 0 };

            // 发射批量进度事件
            batch.emit(app, display_name, file_percent, speed,
                downloaded, content_length,
                &format!("下载中 {}%", file_percent));

            last_emit = Instant::now();
            last_bytes = downloaded;
        }
    }

    file.flush().map_err(|e| format!("刷新失败: {}", e))?;
    drop(file);

    if local_path.exists() {
        std::fs::remove_file(local_path).ok();
    }
    std::fs::rename(&temp_path, local_path)
        .map_err(|e| format!("重命名失败: {}", e))?;

    Ok(downloaded)
}

/// 简化的批量下载 — 从 URL 列表直接下载
#[allow(dead_code)]
pub async fn download_urls_parallel(
    app: &tauri::AppHandle,
    tasks: Vec<(Vec<String>, PathBuf, FileChecker)>,
    max_threads: usize,
    step_label: &str,
) -> DownloadResult {
    let files: Vec<DownloadFile> = tasks
        .into_iter()
        .map(|(urls, path, check)| DownloadFile::new(urls, path, check))
        .collect();

    let total = files.len();
    if total == 0 {
        return DownloadResult {
            total: 0, success: 0, failed: 0, skipped: 0, errors: Vec::new(),
        };
    }

    let dl_threads = config_read("user".to_string())
        .ok()
        .and_then(|c| c["dl_threads"].as_str().and_then(|s| s.parse::<usize>().ok()))
        .unwrap_or(max_threads).max(1).min(64);

    let total_bytes: u64 = files.iter().map(|f| f.check.actual_size.max(0) as u64).sum();
    let label = step_label.to_string();

    let batch = Arc::new(BatchProgress {
        batch_id: next_batch_id(),
        total_files: total as u64,
        done_files: AtomicU64::new(0),
        total_bytes: AtomicU64::new(total_bytes),
        downloaded_bytes: AtomicU64::new(0),
        active_speeds: std::sync::Mutex::new(Vec::new()),
    });

    let sem = Arc::new(tokio::sync::Semaphore::new(dl_threads));
    let success_count = Arc::new(AtomicU64::new(0));
    let failed_count = Arc::new(AtomicU64::new(0));
    let skipped_count = Arc::new(AtomicU64::new(0));

    let mut handles = Vec::new();
    for idx in 0..files.len() {
        if is_cancelled() { break; }
        let a = app.clone();
        let s = sem.clone();
        let batch = batch.clone();
        let sc = success_count.clone();
        let fc = failed_count.clone();
        let sk = skipped_count.clone();

        let urls = files[idx].urls.clone();
        let local_path = files[idx].local_path.clone();
        let check = files[idx].check.clone();
        let can_use = check.can_use_exists;
        let file_size = check.actual_size.max(0) as u64;
        let lbl_clone = label.clone();

        handles.push(tokio::spawn(async move {
            let _permit = s.acquire().await.unwrap();
            if is_cancelled() { return; }

            let fname = local_path.file_name()
                .map(|n| n.to_string_lossy().to_string()).unwrap_or_default();

            if can_use && check.check(&local_path).is_none() {
                sk.fetch_add(1, Ordering::Relaxed);
                batch.downloaded_bytes.fetch_add(file_size, Ordering::Relaxed);
                let _done = batch.done_files.fetch_add(1, Ordering::Relaxed) + 1;
                batch.emit(&a, &fname, 100, 0, file_size, file_size,
                    &format!("{}: 跳过", lbl_clone));
                return;
            }

            let mut df = DownloadFile::new(urls, local_path, check);
            match download_single_with_batch(&a, &mut df, false, &batch, &fname).await {
                Ok(bytes) => {
                    sc.fetch_add(1, Ordering::Relaxed);
                    batch.downloaded_bytes.fetch_add(bytes, Ordering::Relaxed);
                }
                Err(_) => { fc.fetch_add(1, Ordering::Relaxed); }
            }

            let _done = batch.done_files.fetch_add(1, Ordering::Relaxed) + 1;
            batch.emit(&a, &fname, 100, 0, file_size, file_size,
                &format!("{}: 完成 ({}/{})", lbl_clone, _done, total));
        }));
    }

    for h in handles { let _ = h.await; }

    DownloadResult {
        total,
        success: success_count.load(Ordering::Relaxed) as usize,
        failed: failed_count.load(Ordering::Relaxed) as usize,
        skipped: skipped_count.load(Ordering::Relaxed) as usize,
        errors: Vec::new(),
    }
}
