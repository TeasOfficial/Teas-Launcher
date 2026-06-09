//! 版本元数据组（用于合并多个版本的变动列表）

use super::version_meta::VersionMeta;

/// 完整版本元数据（包含所属更新包文件名）
pub struct FullVersionMeta {
    pub filename: String,
    pub metadata: VersionMeta,
}
