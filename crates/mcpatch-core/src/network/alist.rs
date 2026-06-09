//! Alist 网盘协议实现

use std::ops::Range;

use async_trait::async_trait;

use crate::network::{DownloadResult, GlobalConfigPlaceholder, UpdatingSource};

pub struct AlistProtocol {
    #[allow(dead_code)]
    url: String,
    mask_keyword: String,
    #[allow(dead_code)]
    index: u32,
}

impl AlistProtocol {
    pub fn new(url: &str, _config: &GlobalConfigPlaceholder, index: u32) -> Self {
        let mask_keyword = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_owned()))
            .unwrap_or_default();

        Self {
            url: url.to_owned(),
            mask_keyword,
            index,
        }
    }
}

#[async_trait]
impl UpdatingSource for AlistProtocol {
    async fn request(
        &mut self,
        _path: &str,
        _range: &Range<u64>,
        _desc: &str,
        _config: &GlobalConfigPlaceholder,
    ) -> DownloadResult {
        // TODO: 实现 Alist 网盘的 API 调用
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Alist 协议待实现",
        ))
    }

    fn mask_keyword(&self) -> &str {
        &self.mask_keyword
    }
}
