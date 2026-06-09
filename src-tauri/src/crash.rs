//! 崩溃检测：分析 stderr / crash-reports / latest.log

use std::time::Duration;

#[tauri::command]
pub(crate) fn check_crash(
    mc_dir: String,
    instance_name: String,
) -> Option<serde_json::Value> {
    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let teas = base.join(".teas");
    let stderr_log = teas.join("last_stderr.log");
    let crash_dir = dot_minecraft
        .join("versions")
        .join(&instance_name)
        .join("crash-reports");
    let now = std::time::SystemTime::now();
    let window = Duration::from_secs(180);
    let in_win = |p: &std::path::Path| -> bool {
        p.metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| now.duration_since(t).unwrap_or_default() < window)
            .unwrap_or(false)
    };
    let mut reasons = Vec::new();
    let mut log_paths = Vec::new();
    let mut log_all = String::new();

    // 检查 stderr 日志
    if stderr_log.exists() && in_win(&stderr_log) {
        if let Ok(c) = std::fs::read_to_string(&stderr_log) {
            let t = c.trim();
            if !t.is_empty() {
                // 正常退出标记：日志中含 "Stopping!" 或进程正常关闭
                if t.contains("Stopping!") {
                    return None;
                }
                log_all.push_str(&t[..t.len().min(8000)]);
                log_paths.push(stderr_log.display().to_string());
            }
        }
    }

    // 检查 crash-reports 目录
    if crash_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&crash_dir) {
            let mut latest_time = std::time::UNIX_EPOCH;
            let mut latest_path = None;
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map_or(false, |x| x == "txt") {
                    if let Ok(m) = p.metadata() {
                        if let Ok(t) = m.modified() {
                            if t > latest_time {
                                latest_time = t;
                                latest_path = Some(p);
                            }
                        }
                    }
                }
            }
            if let Some(p) = latest_path {
                if in_win(&p) {
                    if let Ok(c) = std::fs::read_to_string(&p) {
                        let desc = c
                            .lines()
                            .skip_while(|l| !l.starts_with("Description:"))
                            .take(5)
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !desc.is_empty() {
                            reasons.push(format!("崩溃报告: {}", desc));
                        }
                        log_paths.push(p.display().to_string());
                    }
                }
            }
        }
    }

    // 检查 latest.log
    let latest_log = dot_minecraft
        .join("versions")
        .join(&instance_name)
        .join("logs")
        .join("latest.log");
    if latest_log.exists() && in_win(&latest_log) {
        log_paths.push(latest_log.display().to_string());
        if let Ok(c) = std::fs::read_to_string(&latest_log) {
            let ls: Vec<&str> = c.lines().collect();
            let tl = if ls.len() > 500 { &ls[ls.len() - 500..] } else { &ls };
            log_all.push_str(&tl.join("\n"));
        }
    }

    // 正常退出检测：latest.log 含 "Stopping!" 或进程正常退出关键词
    if log_all.contains("Stopping!")
        || log_all.contains("Stopping the server")
        || log_all.contains("Stopping worker")
    {
        return None;
    }

    // 仅在找到明确崩溃关键词时才报告崩溃
    for (pat, reason) in &[
        ("OutOfMemoryError", "内存溢出"),
        ("NoClassDefFoundError", "类缺失"),
        ("ClassNotFoundException", "类未找到"),
        ("Could not find or load main class", "主类未找到"),
        ("Failed to locate library", "Natives库缺失"),
        ("EXCEPTION_ACCESS_VIOLATION", "显卡驱动不兼容"),
        ("Unsupported class file major version", "Java版本不兼容"),
        ("MixinBootstrap", "MixinBootstrap缺失"),
    ] {
        if log_all.contains(pat) {
            reasons.push(reason.to_string());
        }
    }
    reasons.sort();
    reasons.dedup();

    // 无崩溃原因 + 无 crash-report → 正常退出
    if reasons.is_empty() {
        return None;
    }

    Some(serde_json::json!({
        "crashed": true,
        "reason": reasons.join("\n"),
        "log_path": log_paths.join("\n"),
        "crash_dir": crash_dir.display().to_string()
    }))
}
