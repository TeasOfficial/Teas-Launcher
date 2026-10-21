//! 下载引擎 — 参照 PCL-CE FileDownloader.cs + LoaderDownload.cs
//!
//! 核心逻辑:
//! - download_file: 单文件多源回退下载 (3 次重试)
//! - download_files_parallel: 并行批量下载 (信号量控制)
//! - 进度通过 Tauri event "download-progress" 发射
//! - 支持全局取消标志
//!
//! 进度事件格式（发射统一走 crate::progress，不要在别处直接 emit）:
//! {
//!   "kind": "file"|"stage", // file=逐文件进度, stage=阶段文案（见 progress.rs）
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

use super::model::{DownloadFile, DownloadState};
use crate::config::config_read;
use crate::http::{build_http_client, is_cancelled};
use crate::progress::next_batch_id;
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
    /// 活跃文件 → 当前速度 (B/s)。按文件名记账，文件完成即移除，
    /// 这样总速度才是"此刻真正在传输的那些文件"之和。
    active_speeds: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}

impl BatchProgress {
    fn emit(&self, app: &tauri::AppHandle, file_name: &str, file_percent: u32,
            file_speed: u64, file_downloaded: u64, file_total: u64, step: &str) {
        let done = self.done_files.load(Ordering::Relaxed);
        let total_dl = self.downloaded_bytes.load(Ordering::Relaxed);
        let total_all = self.total_bytes.load(Ordering::Relaxed);

        // 维护"活跃文件速度表"：完成或停传的文件移出，其余更新为最新速度
        let (total_speed, running) = {
            let mut speeds = self.active_speeds.lock().unwrap();
            if file_percent >= 100 || file_speed == 0 {
                speeds.remove(file_name);
            } else {
                speeds.insert(file_name.to_string(), file_speed);
            }
            let sum: u64 = speeds.values().sum();
            (sum, speeds.len())
        };

        let aggregated_percent = if total_all > 0 {
            ((total_dl as f64 / total_all as f64) * 100.0).min(99.0) as u32
        } else if self.total_files > 0 {
            // 没有大小信息时按文件数量算
            ((done as f64 / self.total_files as f64) * 100.0).min(99.0) as u32
        } else {
            0
        };

        let _ = app.emit(crate::progress::EVENT, serde_json::json!({
            "kind": "file",
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
            "total_speed": total_speed,
            "batch_running": running,
            "step": step,
        }));
    }
}

/// 连接长时间没有新数据 → 判定停滞并换源
///
/// 之前只有 600 秒总超时：一条 7 KB/s 但一直不断的连接会一直挂着，
/// 用户看到的就是"进度卡在 7%、剩余时间 1 小时"。
const STALL_TIMEOUT: Duration = Duration::from_secs(10);
/// 观察窗口：超过这个时长还低于阈值速度，就换源
const SLOW_WINDOW: Duration = Duration::from_secs(15);
/// 低于这个速度（B/s）视为"慢到不值得等"。定在 32 KB/s：
/// 实测官方 Forge maven 在国内经常只有 9 KB/s，而 BMCLAPI 是 MB/s 级，
/// 阈值太低就永远不换源（曾卡在 9 KB/s 不动）。
const SLOW_MIN_BPS: u64 = 32 * 1024;
/// 换源信号：调用方看到这些前缀就不再重试同一个 URL，直接试下一个源
const ERR_STALL: &str = "__STALL__:";
const ERR_SLOW: &str = "__SLOW__:";
/// 限流信号：BMCLAPI 并发一多就回 429（有时是 403），要退避后重试同一个源，
/// 当成永久失败会让整批文件同时失败。
const ERR_THROTTLE: &str = "__THROTTLE__:";

/// BMCLAPI 是社区镜像，并发打太高会被限流（实测 8 并发里 3 个直接 429/403）。
/// 给它单独限流：同一时刻最多 3 个请求走镜像，官方源不受影响。
static MIRROR_SEM: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);

fn is_mirror_url(url: &str) -> bool {
    url.contains("bmclapi2.bangbang93.com")
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
            log::debug!("[DL] 跳过 (已存在): {}", file.file_name());
            return Ok(());
        }
    }

    // 2) 确保目标目录存在
    if let Some(parent) = file.local_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建目录 {}: {}", parent.display(), e))?;
    }

    // 3) 清理可能的残留临时文件
    cleanup_temp(&file.local_path).await;

    // 4) 遍历所有 URL，每个最多重试 3 次
    file.state = DownloadState::Connecting;

    // 清除 URL 列表中的空格，过滤空 URL
    let urls: Vec<String> = file
        .urls
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();

    if urls.is_empty() {
        file.mark_failed("未提供可用的下载地址".to_string());
        return Err("未提供可用的下载地址".to_string());
    }

    let mut last_error = String::new();

    for (url_idx, url) in urls.iter().enumerate() {
        log::debug!("[DL] 尝试: {} -> {}", url, file.local_path.display());
        let allow_slow_abort = url_idx + 1 < urls.len();

        // 每个源最多重试 3 次 (参照 PCL-CE: 4次总共 = 1次初始 + 3次重试)
        for retry in 0u32..4 {
            if is_cancelled() {
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
                allow_slow_abort,
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
                        log::error!(
                            "[DL] 校验失败: {} ({})",
                            file.file_name(),
                            err
                        );
                        cleanup_temp(&file.local_path).await;
                        last_error = format!("校验失败: {}", err);
                        continue; // 尝试下一个 URL
                    }

                    file.mark_finished();
                    log::debug!("[DL] 成功: {} ({} bytes)", file.file_name(), size);
                    return Ok(());
                }
                Err(e) => {
                    // 限流：等一会儿再来，别把整批文件一起判死
                    if e.starts_with(ERR_THROTTLE) {
                        // 指数退避：1s / 2s / 4s / 8s
                        let wait = 1000u64 << retry.min(3);
                        log::warn!("[DL] 被限流，退避 {} ms 后重试同一个源: {}", wait, e);
                        tokio::time::sleep(Duration::from_millis(wait)).await;
                        last_error = e;
                        continue;
                    }
                    // 停滞/过慢是"这个源不行"：换下一个源，但**不删**已下载的部分（续传用）
                    if e.starts_with(ERR_STALL) || e.starts_with(ERR_SLOW) {
                        log::warn!("[DL] 换源: {} ({})", url, e);
                        last_error = e;
                        break;
                    }
                    cleanup_temp(&file.local_path).await;
                    log::warn!("[DL] 失败 (重试 {}/3): {}", retry, e);
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
    // 只剩最后一个源时不要再"过慢中断"，否则大文件会永远下不完
    allow_slow_abort: bool,
) -> Result<u64, String> {
    let client = build_http_client(Duration::from_secs(600))?;
    let start = Instant::now();

    // 断点续传：上一轮（可能是换了源）留下的 .dlpart 已有多少字节就从哪继续，
    // 这样"换源"不再意味着从头再下 5 MB。
    let temp_path = local_path.with_extension("dlpart");
    let resume_from = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);

    // 走镜像时先排队，避免把社区镜像打成限流
    let _mirror_permit = if is_mirror_url(url) {
        Some(MIRROR_SEM.acquire().await.map_err(|e| format!("排队失败: {}", e))?)
    } else {
        None
    };

    let mut req = client.get(url).timeout(Duration::from_secs(600));
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={}-", resume_from));
    }
    // 等响应头也要有超时：BMCLAPI 走 Cloudflare，实测会出现连接建立后
    // 一个字都不回的情况（600 秒总超时意味着界面卡十分钟）。
    let resp = tokio::time::timeout(Duration::from_secs(20), req.send())
        .await
        .map_err(|_| format!("{} 20 秒未收到响应头，换源", ERR_STALL))?
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        if code == 429 || code == 403 || code == 503 {
            return Err(format!("{} HTTP {}", ERR_THROTTLE, status));
        }
        return Err(format!("HTTP {}", status));
    }

    // 206 = 服务器支持续传；200 = 不支持，只能从头来
    let resumed = status.as_u16() == 206 && resume_from > 0;
    let (mut downloaded, total, mut file) = if resumed {
        let body_len = resp.content_length().unwrap_or(0);
        let f = std::fs::OpenOptions::new()
            .append(true)
            .open(&temp_path)
            .map_err(|e| format!("无法打开临时文件: {}", e))?;
        (resume_from, resume_from + body_len, f)
    } else {
        let f = std::fs::File::create(&temp_path)
            .map_err(|e| format!("无法创建临时文件: {}", e))?;
        (0u64, resp.content_length().unwrap_or(0), f)
    };
    log::debug!("[DL] 响应: total={} resume_from={} url={}", total, resume_from, url);

    // 流式下载 + 进度追踪
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut last_emit = Instant::now();
    let mut last_bytes: u64 = downloaded;

    use std::io::Write;
    let mut slow_checked = false;
    loop {
        // 停滞检测：超过 STALL_TIMEOUT 没收到任何字节就判定这条连接废了，换源
        let next = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(v) => v,
            Err(_) => {
                // 保留 .dlpart：换源后要接着下
                return Err(format!(
                    "{} {} 秒无数据，换源",
                    ERR_STALL,
                    STALL_TIMEOUT.as_secs()
                ));
            }
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| format!("读取失败: {}", e))?;

        // 过慢检测：跑了 SLOW_WINDOW 还低于 SLOW_MIN_BPS，且离完成还远 → 换源
        if allow_slow_abort && !slow_checked && start.elapsed() >= SLOW_WINDOW && total > 0 && downloaded < total * 95 / 100 {
            slow_checked = true;
            let secs = start.elapsed().as_secs().max(1);
            // 只算**本轮**下载的字节：续传时 downloaded 里含着上次的进度，
            // 直接除会把"续传了 2 MB 然后卡住"误判成高速。
            let bps = downloaded.saturating_sub(resume_from) / secs;
            if bps < SLOW_MIN_BPS {
                // 保留 .dlpart：换源后接着下，不白费这几 MB
                return Err(format!(
                    "{} 平均 {} KB/s 过慢（已下载 {} KB），换源",
                    ERR_SLOW,
                    bps / 1024,
                    downloaded / 1024
                ));
            }
        }

        if is_cancelled() {
            let _ = std::fs::remove_file(&temp_path);
            return Err("已取消".to_string());
        }

        file.write_all(&chunk)
            .map_err(|e| format!("写入失败: {}", e))?;
        downloaded += chunk.len() as u64;

        // 每 100ms 发射一次进度 (参照 PCL-CE: 事件驱动)
        let elapsed = last_emit.elapsed();
        if elapsed >= Duration::from_millis(50) || downloaded >= total {
            let speed = if elapsed.as_secs_f64() > 0.0 {
                ((downloaded - last_bytes) as f64 / elapsed.as_secs_f64()) as u64
            } else {
                0
            };
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
                crate::progress::EVENT,
                serde_json::json!({
                    "kind": "file",
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
    log::info!(
        "[DL] 单文件完成: {} bytes, {}s, {} B/s avg",
        downloaded,
        elapsed_total.as_secs_f64(),
        avg_speed
    );

    Ok(downloaded)
}

/// 清理临时文件
async fn cleanup_temp(path: &PathBuf) {
    let temp_path = path.with_extension("dlpart");
    for _ in 0..5 {
        match std::fs::remove_file(&temp_path) {
            Ok(_) => break,
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
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
        };
    }

    let dl_threads = config_read("user".to_string())
        .ok()
        .and_then(|c| c["dl_threads"].as_str().and_then(|s| s.parse::<usize>().ok()))
        .unwrap_or(max_threads)
        .max(1)
        .min(64);

    log::info!("[DL] 批量下载 {} 个文件 ({} 线程)", total, dl_threads);

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
        active_speeds: std::sync::Mutex::new(std::collections::HashMap::new()),
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

            // 先报一条 0%：并发文件马上出现在任务详情里，
            // 而不是各自等第一个数据块（大文件首字节常要等几秒）
            batch.emit(&a, &file_name, 0, 0, 0, file_size, "开始下载");

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

            let _done = batch.done_files.fetch_add(1, Ordering::Relaxed) + 1;
            match result {
                Ok(bytes) => {
                    sc.fetch_add(1, Ordering::Relaxed);
                    batch.downloaded_bytes.fetch_add(bytes, Ordering::Relaxed);
                    batch.emit(&a, &file_name, 100, 0, file_size, file_size,
                        &format!("完成 ({}/{})", _done, total));
                }
                Err(e) => {
                    fc.fetch_add(1, Ordering::Relaxed);
                    log::warn!("[DL] 批量-失败: {} ({})", file_name, e);
                    // 失败也要计入完成数，否则进度条会停在最后一个成功文件上；
                    // 但必须用独立文案，避免前端把失败当成功显示
                    batch.emit(&a, &file_name, 100, 0, file_size, file_size,
                        &format!("失败 ({}/{}): {}", _done, total, e));
                }
            }
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
    };

    log::info!(
        "[DL] 批量完成: {}/{} 成功, {} 跳过, {} 失败",
        result.success, result.total, result.skipped, result.failed
    );
    result
}

/// 批量模式下的单文件下载 — 在下载过程中实时向 BatchProgress 报告进度
async fn download_single_with_batch(
    app: &tauri::AppHandle,
    file: &mut DownloadFile,
    _enable_parallel_chunks: bool,
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

    cleanup_temp(&file.local_path).await;

    file.state = DownloadState::Connecting;

    let urls: Vec<String> = file.urls.iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();

    if urls.is_empty() {
        file.mark_failed("未提供可用的下载地址".to_string());
        return Err("未提供可用的下载地址".to_string());
    }

    let mut last_error = String::new();

    for (url_idx, url) in urls.iter().enumerate() {
        let allow_slow_abort = url_idx + 1 < urls.len();
        for retry in 0u32..4 {
            if is_cancelled() {
                file.state = DownloadState::Interrupted;
                return Err("已取消".to_string());
            }

            let result = download_single_with_batch_progress(
                app, url, &file.local_path, batch, display_name,
                file.check.actual_size.max(0) as u64, allow_slow_abort,
            ).await;

            match result {
                Ok(size) => {
                    file.downloaded = size;
                    file.total_size = size as i64;
                    file.state = DownloadState::Verifying;

                    if let Some(err) = file.check.check(&file.local_path) {
                        log::error!("[DL] 校验失败: {} ({})", file.file_name(), err);
                        cleanup_temp(&file.local_path).await;
                        last_error = format!("校验失败: {}", err);
                        continue;
                    }

                    file.mark_finished();
                    return Ok(size);
                }
                Err(e) => {
                    // 限流：退避后重试同一个源
                    if e.starts_with(ERR_THROTTLE) {
                        // 指数退避：1s / 2s / 4s / 8s
                        let wait = 1000u64 << retry.min(3);
                        log::warn!("[DL] 被限流，退避 {} ms 后重试同一个源: {}", wait, e);
                        tokio::time::sleep(Duration::from_millis(wait)).await;
                        last_error = e;
                        continue;
                    }
                    // 停滞/过慢：换下一个源，保留已下载部分
                    if e.starts_with(ERR_STALL) || e.starts_with(ERR_SLOW) {
                        log::warn!("[DL] 换源: {} ({})", url, e);
                        last_error = e;
                        break;
                    }
                    cleanup_temp(&file.local_path).await;
                    log::warn!("[DL] 失败 (重试 {}/3): {}", retry, e);
                    last_error = e;
                    if retry < 3 {
                        let delay = 300 + retry as u64 * 300;
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }
    }

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
    allow_slow_abort: bool,
) -> Result<u64, String> {
    let client = build_http_client(Duration::from_secs(600))?;

    // 断点续传（同 download_single）：换源时接着下，不从头再来
    let temp_path = local_path.with_extension("dlpart");
    let resume_from = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);

    // 走镜像时先排队，避免把社区镜像打成限流
    let _mirror_permit = if is_mirror_url(url) {
        Some(MIRROR_SEM.acquire().await.map_err(|e| format!("排队失败: {}", e))?)
    } else {
        None
    };

    let mut req = client.get(url).timeout(Duration::from_secs(600));
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={}-", resume_from));
    }
    // 等响应头也要有超时：BMCLAPI 走 Cloudflare，实测会出现连接建立后
    // 一个字都不回的情况（600 秒总超时意味着界面卡十分钟）。
    let resp = tokio::time::timeout(Duration::from_secs(20), req.send())
        .await
        .map_err(|_| format!("{} 20 秒未收到响应头，换源", ERR_STALL))?
        .map_err(|e| format!("请求失败: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        if code == 429 || code == 403 || code == 503 {
            return Err(format!("{} HTTP {}", ERR_THROTTLE, status));
        }
        return Err(format!("HTTP {}", status));
    }

    let resumed = status.as_u16() == 206 && resume_from > 0;
    let (mut downloaded, total, mut file) = if resumed {
        let body_len = resp.content_length().unwrap_or(0);
        let f = std::fs::OpenOptions::new()
            .append(true)
            .open(&temp_path)
            .map_err(|e| format!("无法打开临时文件: {}", e))?;
        (resume_from, resume_from + body_len, f)
    } else {
        let f = std::fs::File::create(&temp_path)
            .map_err(|e| format!("无法创建临时文件: {}", e))?;
        (0u64, resp.content_length().unwrap_or(file_total), f)
    };
    log::debug!("[DL] 响应: total={} resume_from={} url={}", total, resume_from, url);
    let start = Instant::now();

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut last_emit = Instant::now();
    let mut last_bytes: u64 = downloaded;

    use std::io::Write;
    let mut slow_checked = false;
    loop {
        // 停滞检测：超过 STALL_TIMEOUT 没收到任何字节就判定这条连接废了，换源
        let next = match tokio::time::timeout(STALL_TIMEOUT, stream.next()).await {
            Ok(v) => v,
            Err(_) => {
                // 保留 .dlpart：换源后要接着下
                return Err(format!(
                    "{} {} 秒无数据，换源",
                    ERR_STALL,
                    STALL_TIMEOUT.as_secs()
                ));
            }
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| format!("读取失败: {}", e))?;

        // 过慢检测：跑了 SLOW_WINDOW 还低于 SLOW_MIN_BPS，且离完成还远 → 换源
        if allow_slow_abort && !slow_checked && start.elapsed() >= SLOW_WINDOW && total > 0 && downloaded < total * 95 / 100 {
            slow_checked = true;
            let secs = start.elapsed().as_secs().max(1);
            // 只算**本轮**下载的字节：续传时 downloaded 里含着上次的进度，
            // 直接除会把"续传了 2 MB 然后卡住"误判成高速。
            let bps = downloaded.saturating_sub(resume_from) / secs;
            if bps < SLOW_MIN_BPS {
                // 保留 .dlpart：换源后接着下，不白费这几 MB
                return Err(format!(
                    "{} 平均 {} KB/s 过慢（已下载 {} KB），换源",
                    ERR_SLOW,
                    bps / 1024,
                    downloaded / 1024
                ));
            }
        }

        if is_cancelled() {
            let _ = std::fs::remove_file(&temp_path);
            return Err("已取消".to_string());
        }

        file.write_all(&chunk).map_err(|e| format!("写入失败: {}", e))?;
        downloaded += chunk.len() as u64;

        let elapsed = last_emit.elapsed();
        if elapsed >= Duration::from_millis(50) || (total > 0 && downloaded >= total) {
            let speed = if elapsed.as_secs_f64() > 0.0 {
                ((downloaded - last_bytes) as f64 / elapsed.as_secs_f64()) as u64
            } else { 0 };

            let file_percent = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0).min(99.0) as u32
            } else { 0 };

            // 发射批量进度事件
            batch.emit(app, display_name, file_percent, speed,
                downloaded, total,
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
