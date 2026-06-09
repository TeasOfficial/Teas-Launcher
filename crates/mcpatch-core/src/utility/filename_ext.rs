//! 文件名提取扩展方法

use std::path::Path;

/// 获取路径的文件名部分（不含扩展名逻辑，就是取最后一个组件）
pub trait GetFileNamePart {
    fn filename(&self) -> &str;
}

impl GetFileNamePart for Path {
    fn filename(&self) -> &str {
        self.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
    }
}
