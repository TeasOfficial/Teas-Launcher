//! 下载模型 — 参照 PCL-CE DownloadFile + FileChecker
//!
//! FileChecker 提供 4 层校验:
//! 1. 最小文件大小 (min_size, -1 不做检查)
//! 2. 期望文件大小 (actual_size, -1 不做检查)
//! 3. SHA1/MD5 哈希校验
//! 4. JSON 格式校验 (is_json)
//!
//! 参照 PCL-CE:
//! - ModBase.FileChecker(canUseExistsFile, minSize, actualSize, hash, isJson)
//! - DownloadFile(urls, localPath, checker)
//! 完整移植 PCL-CE 模型，部分变体/方法前端暂未使用

use std::path::PathBuf;
use std::time::Instant;

/// 下载文件状态 — 参照 PCL-CE NetState
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    /// 等待开始
    Waiting,
    /// 正在连接
    Connecting,
    /// 读取响应头
    #[allow(dead_code)]
    Reading,
    /// 正在下载
    #[allow(dead_code)]
    Downloading,
    /// 下载完成，等待校验
    Verifying,
    /// 已完成
    Finished,
    /// 已中断（失败）
    Interrupted,
}

/// 哈希算法类型
#[derive(Debug, Clone, PartialEq)]
pub enum HashAlgo {
    Sha1,
    #[allow(dead_code)]
    Md5,
}

/// 文件校验检查器 — 参照 PCL-CE ModBase.FileChecker
///
/// PCL-CE 签名:
/// ```vb
/// Public Class FileChecker
///     Public canUseExistsFile As Boolean = True
///     Public minSize          As Long = 1
///     Public actualSize       As Long = -1
///     Public hash             As String = Nothing
///     Public isJson           As Boolean = False
///     Public isZip            As Boolean = False
/// ```
#[derive(Debug, Clone)]
pub struct FileChecker {
    /// 如果文件已存在且通过校验，可直接使用不重新下载
    pub can_use_exists: bool,
    /// 最小文件大小（bytes），-1 不做检查
    pub min_size: i64,
    /// 期望文件大小（bytes），-1 不做检查
    pub actual_size: i64,
    /// SHA1 或 MD5 哈希值
    pub hash: Option<String>,
    /// 哈希算法类型
    pub hash_algo: HashAlgo,
    /// 校验 JSON 格式（解析 JSON 是否成功）
    pub is_json: bool,
}

impl Default for FileChecker {
    fn default() -> Self {
        Self {
            can_use_exists: true,
            min_size: 1,
            actual_size: -1,
            hash: None,
            hash_algo: HashAlgo::Sha1,
            is_json: false,
        }
    }
}

impl FileChecker {
    /// PCL-CE 风格: 只接受最小大小
    pub fn with_min_size(min_size: i64) -> Self {
        Self { min_size, ..Default::default() }
    }

    /// PCL-CE 风格: 完整参数（不含 JSON 标记）
    pub fn new(min_size: i64, actual_size: i64, hash: Option<String>) -> Self {
        Self { min_size, actual_size, hash, ..Default::default() }
    }

    /// PCL-CE 风格: 带 JSON 标记
    #[allow(dead_code)]
    pub fn with_json(mut self, is_json: bool) -> Self {
        self.is_json = is_json;
        self
    }

    /// PCL-CE 风格: 指定哈希算法
    pub fn with_hash_algo(mut self, algo: HashAlgo) -> Self {
        self.hash_algo = algo;
        self
    }

    /// 检查文件是否通过所有校验
    /// 返回 None 表示通过，返回 Some(String) 表示失败原因
    ///
    /// 参照 PCL-CE:
    /// ```vb
    /// Public Function Check(Path As String) As String
    ///     If Path Is Nothing OrElse Not File.Exists(Path) Then
    ///         Return "文件不存在"
    ///     // minSize check
    ///     // actualSize check
    ///     // hash check
    ///     // isJson check
    ///     Return Nothing ' 通过
    /// ```
    pub fn check(&self, path: &PathBuf) -> Option<String> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => return Some(format!("文件不存在: {}", e)),
        };

        let file_size = meta.len() as i64;

        // 最小大小检查
        if self.min_size > 0 && file_size < self.min_size {
            return Some(format!(
                "文件过小 ({} bytes < {} bytes)",
                file_size, self.min_size
            ));
        }

        // 期望大小检查
        if self.actual_size > 0 && file_size != self.actual_size {
            return Some(format!(
                "文件大小不匹配 ({} bytes != {} bytes)",
                file_size, self.actual_size
            ));
        }

        // 哈希检查
        if let Some(ref expected_hash) = self.hash {
            match self.calc_hash(path) {
                Ok(actual_hash) => {
                    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                        return Some(format!(
                            "哈希不匹配 ({} != {})",
                            actual_hash.to_lowercase(),
                            expected_hash.to_lowercase()
                        ));
                    }
                }
                Err(e) => return Some(format!("哈希计算失败: {}", e)),
            }
        }

        // JSON 格式检查
        if self.is_json {
            if let Ok(content) = std::fs::read_to_string(path) {
                if serde_json::from_str::<serde_json::Value>(&content).is_err() {
                    return Some("JSON 格式无效".to_string());
                }
            }
        }

        None
    }

    fn calc_hash(&self, path: &PathBuf) -> Result<String, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let digest: String = match self.hash_algo {
            HashAlgo::Sha1 => {
                use sha1::{Digest, Sha1};
                let mut hasher = Sha1::new();
                hasher.update(&bytes);
                format!("{:x}", hasher.finalize())
            }
            HashAlgo::Md5 => {
                let digest = md5::compute(&bytes);
                format!("{:x}", digest)
            }
        };
        Ok(digest)
    }
}

/// 下载文件描述 — 参照 PCL-CE DownloadFile / NetFile
///
/// PCL-CE 核心字段:
/// - Urls: 多个下载源（优先级从高到低）
/// - LocalPath: 本地目标路径
/// - Check: FileChecker 校验器
/// - State, TotalSize, DownloadedBytes, Speed, ActiveThreads: 运行时状态
#[derive(Debug, Clone)]
pub struct DownloadFile {
    /// 多个下载源 URL（按优先级从高到低排列）
    pub urls: Vec<String>,
    /// 本地目标路径
    pub local_path: PathBuf,
    /// 文件名校验器
    pub check: FileChecker,
    /// 是否使用浏览器 User-Agent
    pub use_browser_ua: bool,
    /// 自定义 User-Agent
    pub custom_ua: Option<String>,

    // ─── 运行时状态（由下载引擎更新）───
    /// 当前状态
    pub state: DownloadState,
    /// 总大小（bytes），-1 表示未知
    pub total_size: i64,
    /// 已下载字节数
    pub downloaded: u64,
    /// 下载速度（bytes/s）
    pub speed: u64,
    /// 活跃线程数
    pub active_threads: u32,
    /// 发生错误时记录
    #[allow(dead_code)]
    pub errors: Vec<String>,
    /// 完成时间
    pub finish_time: Option<Instant>,
}

impl DownloadFile {
    /// 创建新下载文件任务 — PCL-CE 风格
    pub fn new(urls: Vec<String>, local_path: PathBuf, check: FileChecker) -> Self {
        Self {
            urls,
            local_path,
            check,
            use_browser_ua: false,
            custom_ua: None,
            state: DownloadState::Waiting,
            total_size: -1,
            downloaded: 0,
            speed: 0,
            active_threads: 0,
            errors: Vec::new(),
            finish_time: None,
        }
    }

    /// 从单个 URL 创建
    #[allow(dead_code)]
    pub fn from_url(url: String, local_path: PathBuf, check: FileChecker) -> Self {
        Self::new(vec![url], local_path, check)
    }

    /// 设置浏览器 UA
    #[allow(dead_code)]
    pub fn with_browser_ua(mut self) -> Self {
        self.use_browser_ua = true;
        self
    }

    /// 设置自定义 UA
    #[allow(dead_code)]
    pub fn with_custom_ua(mut self, ua: String) -> Self {
        self.custom_ua = Some(ua);
        self
    }

    /// 下载进度 (0.0 ~ 1.0)
    #[allow(dead_code)]
    pub fn progress(&self) -> f64 {
        if self.total_size > 0 {
            (self.downloaded as f64 / self.total_size as f64).min(1.0)
        } else {
            0.0
        }
    }

    /// 文件名
    pub fn file_name(&self) -> String {
        self.local_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// 标记完成
    pub fn mark_finished(&mut self) {
        self.state = DownloadState::Finished;
        self.speed = 0;
        self.active_threads = 0;
        self.finish_time = Some(Instant::now());
    }

    /// 标记失败
    pub fn mark_failed(&mut self, error: String) {
        self.state = DownloadState::Interrupted;
        self.speed = 0;
        self.active_threads = 0;
        self.errors.push(error);
        self.finish_time = Some(Instant::now());
    }
}
