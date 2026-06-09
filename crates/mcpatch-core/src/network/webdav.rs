//! WebDAV 下载协议实现

use std::ops::Range;

use async_trait::async_trait;

use crate::network::{DownloadResult, GlobalConfigPlaceholder, UpdatingSource};

pub struct Webdav {
    #[allow(dead_code)]
    url: String,
    mask_keyword: String,
    #[allow(dead_code)]
    index: u32,
}

impl Webdav {
    pub fn new(url: &str, _config: &GlobalConfigPlaceholder, index: u32) -> Self {
        let mask_keyword = url
            .replacen("webdavs://", "", 1)
            .replacen("webdav://", "", 1)
            .split(':')
            .nth(2)
            .unwrap_or("")
            .to_owned();

        Self {
            url: url.to_owned(),
            mask_keyword,
            index,
        }
    }
}

#[async_trait]
impl UpdatingSource for Webdav {
    async fn request(
        &mut self,
        _path: &str,
        _range: &Range<u64>,
        _desc: &str,
        _config: &GlobalConfigPlaceholder,
    ) -> DownloadResult {
        // TODO: 使用 reqwest_dav 实现完整 WebDAV 下载
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "WebDAV 协议待实现",
        ))
    }

    fn mask_keyword(&self) -> &str {
        &self.mask_keyword
    }
}
