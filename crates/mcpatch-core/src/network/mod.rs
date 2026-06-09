//! 网络层——支持多种下载协议

pub mod alist;
pub mod http;
pub mod private;
pub mod webdav;

use std::ops::Range;
use std::pin::Pin;

use async_trait::async_trait;
use tokio::io::AsyncRead;

use crate::error::{BusinessError, BusinessResult, ResultToBusinessError};
use crate::log::{log_debug, log_error};

/// 下载结果类型
pub type DownloadResult =
    std::io::Result<BusinessResult<(u64, Pin<Box<dyn AsyncRead + Send>>)>>;

/// 网络管理器，管理多个下载源
pub struct Network<'a> {
    sources: Vec<Box<dyn UpdatingSource + Sync + Send>>,
    skip_sources: usize,
    config: &'a GlobalConfigPlaceholder,
}

/// 全局配置占位（后续替换为真正的 GlobalConfig）
pub struct GlobalConfigPlaceholder {
    pub http_retries: u8,
    pub http_timeout: u32,
    pub http_ignore_certificate: bool,
    pub http_headers: Vec<(String, String)>,
    pub urls: Vec<String>,
}

impl<'a> Network<'a> {
    pub fn new(config: &'a GlobalConfigPlaceholder) -> BusinessResult<Self> {
        let mut sources = Vec::new();
        let mut index = 0u32;

        for url in &config.urls {
            if url.starts_with("http://") || url.starts_with("https://") {
                sources.push(Box::new(http::HttpProtocol::new(url, config, index))
                    as Box<dyn UpdatingSource + Sync + Send>);
            } else if url.starts_with("mcpatch://") {
                sources.push(Box::new(private::PrivateProtocol::new(
                    &url["mcpatch://".len()..],
                    config,
                    index,
                )) as Box<dyn UpdatingSource + Sync + Send>);
            } else if url.starts_with("webdav://") || url.starts_with("webdavs://") {
                sources.push(Box::new(webdav::Webdav::new(url, config, index))
                    as Box<dyn UpdatingSource + Sync + Send>);
            } else if url.starts_with("alist://") {
                sources.push(Box::new(alist::AlistProtocol::new(
                    &url["alist://".len()..],
                    config,
                    index,
                )) as Box<dyn UpdatingSource + Sync + Send>);
            }
            index += 1;
        }

        log_debug(format!("loaded {} network sources", sources.len()));

        if sources.is_empty() {
            return Err(BusinessError::new("没有可用的服务器地址"));
        }

        Ok(Network {
            sources,
            skip_sources: 0,
            config,
        })
    }

    /// 请求文本内容
    pub async fn request_text(
        &mut self,
        path: &str,
        range: Range<u64>,
        desc: impl AsRef<str>,
    ) -> BusinessResult<String> {
        match self.request_file(path, range, desc.as_ref()).await {
            Ok((len, mut data)) => {
                use tokio::io::AsyncReadExt;
                let mut text = String::with_capacity(len as usize);
                data.read_to_string(&mut text)
                    .await
                    .be(|e| format!("无法解码为 UTF-8 字符串({})，原因：{:?}", desc.as_ref(), e))?;
                Ok(text)
            }
            Err(err) => Err(err),
        }
    }

    /// 请求文件数据
    pub async fn request_file(
        &mut self,
        path: &str,
        range: Range<u64>,
        desc: &str,
    ) -> BusinessResult<(u64, Pin<Box<dyn AsyncRead + Send>>)> {
        assert!(range.end >= range.start);

        let mut io_error: Option<(std::io::Error, String)> = None;

        for (idx, source) in self.sources[self.skip_sources..].iter_mut().enumerate() {
            let url_index = self.skip_sources + idx;

            log_debug(format!(
                "request {} {}+{} ({}) url: {}",
                path,
                range.start,
                range.end - range.start,
                desc,
                url_index
            ));

            for i in 0..self.config.http_retries + 1 {
                let r = source.request(path, &range, desc, self.config).await;

                match r {
                    Ok(Ok(ok)) => return Ok(ok),
                    Ok(Err(err)) => {
                        io_error = Some((
                            std::io::Error::new(std::io::ErrorKind::Other, err.reason),
                            source.mask_keyword().to_owned(),
                        ));
                        if i != self.config.http_retries {
                            log_error(format!(
                                "url {} encountered a business error, retrying...",
                                url_index
                            ));
                        }
                    }
                    Err(err) => {
                        io_error = Some((err, source.mask_keyword().to_owned()));
                        if i != self.config.http_retries {
                            log_error(format!(
                                "url {} encountered an io error, retrying...",
                                url_index
                            ));
                        }
                    }
                }
            }
        }

        let (err, kw) = io_error.unwrap();
        Err(BusinessError::new(
            format!("{:?}", err).replace(&kw, "[主机部分]"),
        ))
    }
}

/// 更新源 trait
#[async_trait]
pub trait UpdatingSource {
    async fn request(
        &mut self,
        path: &str,
        range: &Range<u64>,
        desc: &str,
        config: &GlobalConfigPlaceholder,
    ) -> DownloadResult;

    fn mask_keyword(&self) -> &str;
}
