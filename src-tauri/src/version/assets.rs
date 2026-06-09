//! Asset/资源文件管理 — 参照 PCL-CE ModAssets.cs
//!
//! 核心功能:
//! - mcassets_get_index: 获取资源索引 JSON (递归 inheritsFrom, 回退 legacy)
//! - mcassets_list_get: 解析索引 JSON 获取全部资源文件列表
//! - mcassets_fix_list: 查找缺失资源 → 生成 DownloadFile 下载任务

use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_assets;
use std::path::{Path, PathBuf};

/// 资源文件描述 — 参照 PCL-CE McAssetsToken
#[derive(Debug, Clone)]
pub struct McAssetToken {
    pub local_path: PathBuf,
    #[allow(dead_code)]
    pub source_path: String,
    pub hash: String,
    pub size: i64,
}

/// 获取资源索引名称 — 参照 PCL-CE McAssetsGetIndexName
///
/// 递归 inheritsFrom: assetIndex.id → assets 字段 → "legacy"
pub fn mcassets_get_index_name(json: &serde_json::Value) -> String {
    // 当前版本的 assetIndex.id
    if let Some(id) = json
        .get("assetIndex")
        .and_then(|a| a.get("id"))
        .and_then(|i| i.as_str())
    {
        return id.to_string();
    }

    // 回退: assets 字段
    if let Some(assets) = json.get("assets").and_then(|a| a.as_str()) {
        return assets.to_string();
    }

    // 递归 inheritsFrom
    "legacy".to_string()
}

/// 获取资源索引完整信息 (含 URL)
///
/// 参照 PCL-CE McAssetsGetIndex:
/// ```vb
/// While True:
///   If index IsNot Nothing AndAlso index("id") IsNot Nothing: Return index
///   If jsonObject("assets") IsNot Nothing: assetsName = jsonObject("assets")
///   ' 下一个实例 (inheritsFrom)
///   If String.IsNullOrEmpty(instance.InheritInstanceName): Break
///   instance = New McInstance(parentPath)
/// End While
/// ' 无法获取 → 回退 legacy
/// ```
pub fn mcassets_get_index(json: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(index) = json.get("assetIndex") {
        if index.get("id").is_some() {
            return Some(index.clone());
        }
    }

    // 回退到 legacy 硬编码信息 (参照 PCL-CE)
    Some(serde_json::json!({
        "id": "legacy",
        "sha1": "c0fd82e8ce9fbc93119e40d96d5a4e62cfa3f729",
        "size": 134284,
        "url": "https://launchermeta.mojang.com/mc-staging/assets/legacy/c0fd82e8ce9fbc93119e40d96d5a4e62cfa3f729/legacy.json",
        "totalSize": 111220701
    }))
}

/// 资源 URL - 参照 PCL-CE McAssetsUrl
pub fn mcassets_url(hash: &str) -> String {
    let prefix = &hash[..2];
    format!("https://resources.download.minecraft.net/{}/{}", prefix, hash)
}

/// 解析资源索引 JSON → Asset 列表
///
/// 参照 PCL-CE McAssetsListGet:
/// ```vb
/// Dim json = GetJson(ReadFile($"assets\indexes\{indexName}.json"))
/// For Each file In json("objects"):
///   ' 确定 localPath (normal / virtual / map_to_resources)
///   Result.Add(New McAssetsToken{localPath, sourcePath, hash, size})
/// ```
pub fn mcassets_list_get(
    mc_dir: &Path,
    index_name: &str,
) -> Result<Vec<McAssetToken>, String> {
    let index_path = mc_dir
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", index_name));

    let content = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("资源索引不存在: {} ({})", index_path.display(), e))?;

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("索引JSON解析失败: {}", e))?;

    let objects = json
        .get("objects")
        .and_then(|o| o.as_object())
        .ok_or("索引JSON缺少 objects 字段")?;

    let is_map_to_resources = json
        .get("map_to_resources")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_virtual = json
        .get("virtual")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut result = Vec::new();
    let objects_dir = mc_dir.join("assets").join("objects");

    for (path, obj) in objects {
        let hash = obj
            .get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("")
            .to_string();
        let size = obj.get("size").and_then(|s| s.as_i64()).unwrap_or(0);

        let local_path = if is_map_to_resources {
            mc_dir.join("resources").join(path.replace('/', "\\"))
        } else if is_virtual {
            mc_dir
                .join("assets")
                .join("virtual")
                .join("legacy")
                .join(path.replace('/', "\\"))
        } else {
            let prefix = if hash.len() >= 2 { &hash[..2] } else { "00" };
            objects_dir.join(prefix).join(&hash)
        };

        result.push(McAssetToken {
            local_path,
            source_path: path.to_string(),
            hash,
            size,
        });
    }

    Ok(result)
}

/// 查找缺失资源 → 生成下载任务
///
/// 参照 PCL-CE McAssetsFixList:
/// ```vb
/// ' Option 1 (checkHash=True): 全部加入下载列表，由下载引擎校验哈希
/// ' Option 2 (checkHash=False): 立即检查文件，缺失的才加入
/// ```
pub fn mcassets_fix_list(
    mc_dir: &Path,
    index_name: &str,
    check_size: bool,
    prefer_official: bool,
) -> Result<Vec<DownloadFile>, String> {
    let assets = mcassets_list_get(mc_dir, index_name)?;
    let mut result = Vec::new();

    for token in assets {
        // 如果 check_size=True，检查文件大小（快速筛除已存在的）
        if check_size {
            if let Ok(meta) = std::fs::metadata(&token.local_path) {
                if token.size == 0 || token.size == meta.len() as i64 {
                    continue;
                }
            }
        }

        let url = mcassets_url(&token.hash);
        let urls = source_assets(&url, prefer_official);
        let checker = FileChecker::new(
            1,
            if token.size > 0 { token.size } else { -1 },
            Some(token.hash.clone()),
        )
        .with_hash_algo(crate::download::model::HashAlgo::Sha1);

        result.push(DownloadFile::new(urls, token.local_path, checker));
    }

    Ok(result)
}

/// 下载资源索引文件
///
/// 参照 PCL-CE ModDownload.DlClientAssetIndexGet
pub fn download_asset_index(
    json: &serde_json::Value,
    mc_dir: &Path,
    prefer_official: bool,
) -> Option<DownloadFile> {
    let index_info = mcassets_get_index(json)?;
    let index_name = index_info.get("id")?.as_str()?;
    let index_url = index_info.get("url")?.as_str()?;

    let index_path = mc_dir
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", index_name));

    if index_path.exists() {
        return None; // 已存在
    }

    let urls: Vec<String> = if prefer_official {
        vec![
            index_url.to_string(),
            index_url
                .replace("https://launchermeta.mojang.com", "https://bmclapi2.bangbang93.com")
        ]
    } else {
        vec![
            index_url
                .replace("https://launchermeta.mojang.com", "https://bmclapi2.bangbang93.com"),
            index_url.to_string(),
        ]
    };

    // 如果索引 URL 为空，返回 None
    if index_url.is_empty() {
        return None;
    }

    let checker = FileChecker {
        min_size: 100,
        is_json: true,
        ..Default::default()
    };

    Some(DownloadFile::new(urls, index_path, checker))
}
