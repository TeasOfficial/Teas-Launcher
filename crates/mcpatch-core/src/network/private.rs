//! 私有协议（mcpatch://）实现

use std::ops::Range;

use async_trait::async_trait;
use tokio::net::TcpStream;

use crate::network::{DownloadResult, GlobalConfigPlaceholder, UpdatingSource};

pub struct PrivateProtocol {
    host: String,
    mask_keyword: String,
    #[allow(dead_code)]
    index: u32,
}

impl PrivateProtocol {
    pub fn new(addr: &str, _config: &GlobalConfigPlaceholder, index: u32) -> Self {
        let (host, mask) = if let Some((h, _)) = addr.rsplit_once(':') {
            (addr.to_owned(), h.to_owned())
        } else {
            (addr.to_owned(), addr.to_owned())
        };

        Self {
            host,
            mask_keyword: mask,
            index,
        }
    }
}

#[async_trait]
impl UpdatingSource for PrivateProtocol {
    async fn request(
        &mut self,
        _path: &str,
        _range: &Range<u64>,
        _desc: &str,
        _config: &GlobalConfigPlaceholder,
    ) -> DownloadResult {
        // 私有协议：简单的 TCP 连接
        let stream = TcpStream::connect(&self.host).await.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e)
        })?;

        // TODO: 实现完整的私有协议握手和数据传输
        Ok(Ok((0, Box::pin(tokio::io::BufReader::new(stream)))))
    }

    fn mask_keyword(&self) -> &str {
        &self.mask_keyword
    }
}
