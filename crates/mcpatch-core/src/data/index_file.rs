//! 索引文件（服务端版本号列表）

/// 索引文件条目
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// 版本标签
    pub label: String,
    /// 更新包文件名
    pub filename: String,
    /// 元数据在更新包中的偏移量
    pub offset: u64,
    /// 元数据长度
    pub len: u32,
}

/// 索引文件（版本号列表）
#[derive(Debug, Clone)]
pub struct IndexFile {
    pub versions: Vec<IndexEntry>,
}

impl IndexFile {
    /// 从 JSON 文本加载索引文件
    pub fn load_from_json(text: &str) -> Vec<IndexEntry> {
        let parsed = match json::parse(text) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

        let mut entries = Vec::new();
        for item in parsed.members() {
            entries.push(IndexEntry {
                label: item["label"].as_str().unwrap_or("").to_owned(),
                filename: item["filename"].as_str().unwrap_or("").to_owned(),
                offset: item["offset"].as_u64().unwrap_or(0),
                len: item["len"].as_u32().unwrap_or(0),
            });
        }
        entries
    }

    /// 返回版本数量
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// 检查是否包含指定标签的版本
    pub fn contains(&self, label: &str) -> bool {
        self.versions.iter().any(|v| v.label == label)
    }
}
