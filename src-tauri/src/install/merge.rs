//! Loader 安装共享原语 — 参照 PCL-CE MergeJson()
//!
//! Fabric / Quilt / Forge / NeoForge / 整合包 的安装流程共用以下四步，
//! 全部集中在此处，避免各 loader 各自维护一份实现：
//! 1. `download_vanilla_json_mem`  — 取原版版本 JSON（不落盘）
//! 2. `merge_libraries`            — 合并 libraries（按 name 去重）
//! 3. `strip_to_self_contained`    — 消除 inheritsFrom，转为自包含 JSON
//! 4. `download_libs_and_client_jar` — 下载 libraries + 原版 client JAR
//!
//! 另有 `parse_modpack_deps` 解析整合包依赖中的 loader 类型与版本。

use crate::download::engine::{download_file, download_files_parallel};
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_launcher_or_meta;
use crate::version::library::{mclib_list_from_json, mclib_to_download_files};
use std::collections::HashSet;
use std::path::Path;

/// 合并 libraries 列表到 target（去重 key: name 字段）
pub(crate) fn merge_libraries(target: &mut serde_json::Value, source: &serde_json::Value) {
    let source_libs = source
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut cur_libs = target
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut seen: HashSet<String> = cur_libs
        .iter()
        .filter_map(|l| l.get("name").and_then(|n| n.as_str().map(|s| s.to_string())))
        .collect();

    for lib in source_libs {
        if let Some(name) = lib.get("name").and_then(|n| n.as_str()) {
            if !seen.contains(name) {
                seen.insert(name.to_string());
                cur_libs.push(lib);
            }
        }
    }

    target["libraries"] = serde_json::Value::Array(cur_libs);
}

/// 转为自包含 JSON：消除 inheritsFrom / _comment_ / jar，写入实例 id
///
/// 参照 PCL-CE MergeJson 的收尾步骤。显式移除 `inheritsFrom` 是启动正确的前提——
/// 生成的 JSON 已含合并后的完整 libraries 与 arguments，不能再让启动器回查父版本。
pub(crate) fn strip_to_self_contained(json: &mut serde_json::Value, instance_name: &str) {
    if let Some(o) = json.as_object_mut() {
        o.remove("inheritsFrom");
        o.remove("_comment_");
        o.remove("jar");
    }
    json["id"] = serde_json::Value::String(instance_name.to_string());
}

/// 从 Mojang 版本清单解析并下载原版版本 JSON（不落盘）
pub(crate) async fn download_vanilla_json_mem(
    client: &reqwest::Client,
    mc_ver: &str,
) -> Result<serde_json::Value, String> {
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let ver_url = manifest["versions"]
        .as_array()
        .ok_or("版本清单为空")?
        .iter()
        .find(|v| v["id"].as_str() == Some(mc_ver))
        .and_then(|v| v["url"].as_str())
        .ok_or(format!("找不到版本 {}", mc_ver))?;

    for url in &source_launcher_or_meta(ver_url, false) {
        if let Ok(resp) = client.get(url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json.get("libraries").is_some() {
                    return Ok(json);
                }
            }
        }
    }
    Err(format!("无法下载原版 JSON: {}", mc_ver))
}

/// 下载 JSON 中声明的全部 libraries，以及原版 client JAR
///
/// 原版 JAR 与 libraries 一样是启动必需文件；有 sha1/size 时优先按哈希校验，
/// 避免残缺文件被当作"已存在"而跳过。
pub(crate) async fn download_libs_and_client_jar(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_json: &serde_json::Value,
    instance_name: &str,
    vanilla_json: &serde_json::Value,
) {
    let libs = mclib_list_from_json(instance_json, mc_dir);
    let mut dl_files = mclib_to_download_files(&libs, false);

    if let Some(jar_url) = vanilla_json["downloads"]["client"]["url"].as_str() {
        let target = mc_dir.join("versions").join(instance_name);
        let jar_path = target.join(format!("{}.jar", instance_name));
        let sha1 = vanilla_json["downloads"]["client"]["sha1"]
            .as_str()
            .map(|s| s.to_string());
        let size = vanilla_json["downloads"]["client"]["size"]
            .as_i64()
            .unwrap_or(-1);
        let checker = FileChecker::new(1024, size, sha1);
        if checker.check(&jar_path).is_some() {
            let urls = source_launcher_or_meta(jar_url, false);
            dl_files.push(DownloadFile::new(urls, jar_path, checker));
        }
    }

    if !dl_files.is_empty() {
        download_files_parallel(app, &mut dl_files, 8).await;
    }
}

/// 解析整合包索引中的 loader 类型和版本
pub fn parse_modpack_deps(index: &serde_json::Value) -> (String, String, String) {
    let deps = index.get("dependencies");
    let mc_ver = deps
        .and_then(|d| d.get("minecraft"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if let Some(ver) = deps.and_then(|d| d.get("forge")).and_then(|v| v.as_str()) {
        ("forge".to_string(), ver.to_string(), mc_ver.to_string())
    } else if let Some(ver) = deps.and_then(|d| d.get("neoforge")).and_then(|v| v.as_str()) {
        ("neoforge".to_string(), ver.to_string(), mc_ver.to_string())
    } else if let Some(ver) = deps.and_then(|d| d.get("fabric-loader")).and_then(|v| v.as_str()) {
        ("fabric".to_string(), ver.to_string(), mc_ver.to_string())
    } else if let Some(ver) = deps.and_then(|d| d.get("quilt-loader")).and_then(|v| v.as_str()) {
        ("quilt".to_string(), ver.to_string(), mc_ver.to_string())
    } else {
        ("vanilla".to_string(), "".to_string(), mc_ver.to_string())
    }
}

/// 下载单个文件到指定路径（instaleler JAR 等）
pub(crate) async fn fetch_single(
    app: &tauri::AppHandle,
    urls: Vec<String>,
    dest: std::path::PathBuf,
    min_size: i64,
) -> Result<(), String> {
    let mut dl = DownloadFile::new(urls, dest, FileChecker::with_min_size(min_size));
    download_file(app, &mut dl, false).await
}
