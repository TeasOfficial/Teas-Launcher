//! 下载速度采样器

use std::time::{Duration, Instant};

/// 下载速度计算器（滑动窗口采样）
pub struct SpeedCalculator {
    window: Duration,
    samples: Vec<(Instant, usize)>,
}

impl SpeedCalculator {
    /// 创建新的速度采样器
    /// `window_ms`: 采样窗口大小（毫秒）
    pub fn new(window_ms: u64) -> Self {
        Self {
            window: Duration::from_millis(window_ms),
            samples: Vec::new(),
        }
    }

    /// 记录一次下载的字节数
    pub fn feed(&mut self, bytes: usize) {
        let now = Instant::now();
        self.samples.push((now, bytes));

        // 清理超过窗口的旧样本
        let cutoff = now - self.window;
        self.samples.retain(|(t, _)| *t > cutoff);
    }

    /// 获取当前采样速度（字节/秒），返回人类可读字符串
    pub fn sample_speed2(&self) -> String {
        let now = Instant::now();
        let cutoff = now - self.window;
        let total: usize = self
            .samples
            .iter()
            .filter(|(t, _)| *t > cutoff)
            .map(|(_, b)| *b)
            .sum();

        let bytes_per_sec = total as f64 / self.window.as_secs_f64();
        super::utility::convert_bytes(bytes_per_sec as u64) + "/s"
    }
}
