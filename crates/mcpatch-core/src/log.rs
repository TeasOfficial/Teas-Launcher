//! 日志系统

use chrono::Local;

/// 日志消息级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// 日志处理器 trait
pub trait LogHandler: Send + Sync {
    fn handle(&self, level: MessageLevel, message: &str);
}

/// 控制台日志处理器
pub struct ConsoleHandler {
    min_level: MessageLevel,
}

impl ConsoleHandler {
    pub fn new(min_level: MessageLevel) -> Self {
        Self { min_level }
    }
}

impl LogHandler for ConsoleHandler {
    fn handle(&self, level: MessageLevel, message: &str) {
        if level < self.min_level {
            return;
        }
        let level_str = match level {
            MessageLevel::Debug => "DEBUG",
            MessageLevel::Info => " INFO",
            MessageLevel::Warning => " WARN",
            MessageLevel::Error => "ERROR",
        };
        let now = Local::now().format("%Y-%m-%d %H:%M:%S");
        println!("[{} {}] {}", now, level_str, message);
    }
}

/// 文件日志处理器
pub struct FileHandler {
    path: std::path::PathBuf,
}

impl FileHandler {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl LogHandler for FileHandler {
    fn handle(&self, _level: MessageLevel, message: &str) {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = writeln!(file, "{}", message);
        }
    }
}

// 全局日志处理器（简化版，仅用控制台输出）
static mut LOG_HANDLERS: once_cell::sync::Lazy<
    Vec<Box<dyn LogHandler>>,
> = once_cell::sync::Lazy::new(|| Vec::new());

static mut LOG_PREFIX: String = String::new();

pub fn add_log_handler(handler: Box<dyn LogHandler>) {
    unsafe {
        LOG_HANDLERS.push(handler);
    }
}

pub fn set_log_prefix(prefix: &str) {
    unsafe {
        LOG_PREFIX = prefix.to_owned();
    }
}

pub fn log_debug(message: impl AsRef<str>) {
    log(MessageLevel::Debug, message.as_ref());
}

pub fn log_info(message: impl AsRef<str>) {
    log(MessageLevel::Info, message.as_ref());
}

pub fn log_error(message: impl AsRef<str>) {
    log(MessageLevel::Error, message.as_ref());
}

fn log(level: MessageLevel, message: &str) {
    unsafe {
        let formatted = if LOG_PREFIX.is_empty() {
            message.to_owned()
        } else {
            format!("[{}] {}", LOG_PREFIX, message)
        };
        for handler in LOG_HANDLERS.iter() {
            handler.handle(level, &formatted);
        }
    }
}
