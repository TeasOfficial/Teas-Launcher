//! 版本元数据（描述单个版本内的文件变动列表）

use std::time::SystemTime;

/// 单个文件变动操作
#[derive(Debug, Clone)]
pub enum FileChange {
    /// 创建空目录
    CreateFolder { path: String },
    /// 更新/创建文件
    UpdateFile {
        path: String,
        hash: String,
        len: u64,
        modified: SystemTime,
        offset: u64,
    },
    /// 删除目录
    DeleteFolder { path: String },
    /// 删除文件
    DeleteFile { path: String },
    /// 移动/重命名文件
    MoveFile { from: String, to: String },
}

/// 版本元数据
#[derive(Debug, Clone)]
pub struct VersionMeta {
    /// 版本标签
    pub label: String,
    /// 更新日志
    pub logs: String,
    /// 文件变动列表
    pub changes: Vec<FileChange>,
}

impl VersionMeta {
    /// 从 JSON 对象加载版本元数据
    pub fn load(json: &json::JsonValue) -> Self {
        let label = json["label"].as_str().unwrap_or("unknown").to_owned();
        let logs = json["logs"].as_str().unwrap_or("").to_owned();
        let mut changes = Vec::new();

        for change in json["changes"].members() {
            let op = change["op"].as_str().unwrap_or("");

            match op {
                "createFolder" => {
                    changes.push(FileChange::CreateFolder {
                        path: change["path"].as_str().unwrap_or("").to_owned(),
                    });
                }
                "updateFile" => {
                    changes.push(FileChange::UpdateFile {
                        path: change["path"].as_str().unwrap_or("").to_owned(),
                        hash: change["hash"].as_str().unwrap_or("").to_owned(),
                        len: change["len"].as_u64().unwrap_or(0),
                        modified: SystemTime::UNIX_EPOCH
                            + std::time::Duration::from_secs(
                                change["modified"].as_u64().unwrap_or(0),
                            ),
                        offset: change["offset"].as_u64().unwrap_or(0),
                    });
                }
                "deleteFolder" => {
                    changes.push(FileChange::DeleteFolder {
                        path: change["path"].as_str().unwrap_or("").to_owned(),
                    });
                }
                "deleteFile" => {
                    changes.push(FileChange::DeleteFile {
                        path: change["path"].as_str().unwrap_or("").to_owned(),
                    });
                }
                "moveFile" => {
                    changes.push(FileChange::MoveFile {
                        from: change["from"].as_str().unwrap_or("").to_owned(),
                        to: change["to"].as_str().unwrap_or("").to_owned(),
                    });
                }
                _ => {}
            }
        }

        VersionMeta { label, logs, changes }
    }
}
