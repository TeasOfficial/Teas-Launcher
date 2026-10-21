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
use std::collections::HashMap;
use std::path::Path;

/// 合并 libraries 列表到 target
///
/// 去重键是 **group:artifact[:classifier]**（不含版本），同一坐标只留一个版本：
/// 整合包/加载器 profile 经常同时带 asm 9.6 与 9.8，全部留下会让 Fabric Loader
/// 在启动时报 `duplicate ASM classes found on classpath`。
pub(crate) fn merge_libraries(target: &mut serde_json::Value, source: &serde_json::Value) {
    use crate::version::library::{maven_key, version_gt};

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

    // key -> 在 cur_libs 里的下标
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, lib) in cur_libs.iter().enumerate() {
        if let Some(name) = lib.get("name").and_then(|n| n.as_str()) {
            seen.insert(maven_key(name), i);
        }
    }

    for lib in source_libs {
        let Some(name) = lib.get("name").and_then(|n| n.as_str()) else { continue };
        let key = maven_key(name);
        match seen.get(&key).copied() {
            None => {
                seen.insert(key, cur_libs.len());
                cur_libs.push(lib);
            }
            Some(idx) => {
                // 同一坐标已有 → 只在版本更高时替换
                let old_ver = cur_libs[idx].get("name").and_then(|n| n.as_str())
                    .map(|n| n.split(':').nth(2).unwrap_or("").to_string()).unwrap_or_default();
                let new_ver = name.split(':').nth(2).unwrap_or("").to_string();
                if !new_ver.is_empty() && version_gt(&new_ver, &old_ver) {
                    log::debug!("[merge] 同坐标多版本，合并保留高版本: {} (替换 {})", name, old_ver);
                    cur_libs[idx] = lib;
                }
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
    let ver_url = crate::version::manifest::resolve_version_url(client, mc_ver).await?;

    for url in &source_launcher_or_meta(&ver_url, false) {
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

/// 从 installer.jar 读取 JSON 条目（不存在返回 None）
///
/// Forge / NeoForge / Cleanroom / 整合包都要从 installer 里掏 version.json、
/// install_profile.json，此前四份各自手写 by_name + 读字符串 + from_str。
pub(crate) fn read_json_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<Option<serde_json::Value>, String> {
    let entry = match archive.by_name(name) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::BufReader::new(entry), &mut s)
        .map_err(|e| format!("读取 {} 失败: {}", name, e))?;
    serde_json::from_str(&s)
        .map(Some)
        .map_err(|e| format!("解析 {} 失败: {}", name, e))
}

/// 同上，但条目必须存在
pub(crate) fn read_required_json_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Result<serde_json::Value, String> {
    read_json_entry(archive, name)?.ok_or_else(|| format!("installer 中无 {}", name))
}

/// 下载版本 JSON 中声明的全部 libraries
pub(crate) async fn download_json_libraries(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    version_json: &serde_json::Value,
) {
    let libs = mclib_list_from_json(version_json, mc_dir);
    let mut dl_files = mclib_to_download_files(&libs, false);
    if !dl_files.is_empty() {
        download_files_parallel(app, &mut dl_files, 8).await;
    }
}

/// 把 installer.jar 内嵌的 `maven/` 目录解压到 libraries/
///
/// installer 会把部分库直接打包进 JAR，这些文件不在 version JSON 的 libraries
/// 列表里，必须单独解出来，否则启动时找不到类。
pub(crate) fn extract_embedded_maven(
    archive: &mut zip::ZipArchive<std::fs::File>,
    mc_dir: &Path,
) {
    let libs_dir = mc_dir.join("libraries");
    // 先收集再解压：borrow checker 不允许在遍历 archive 时同时 by_name 取内容
    let entries: Vec<(String, std::path::PathBuf)> = {
        let mut result = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.starts_with("maven/") && !name.ends_with('/') {
                    let dest = libs_dir.join(name.trim_start_matches("maven/"));
                    if !dest.exists() {
                        result.push((name, dest));
                    }
                }
            }
        }
        result
    };

    for (name, dest) in entries {
        if let Some(p) = dest.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Ok(mut e) = archive.by_name(&name) {
            if let Ok(mut out) = std::fs::File::create(&dest) {
                let _ = std::io::copy(&mut e, &mut out);
            }
        }
    }
}

/// Fabric / Quilt 共用：loader profile → 合入原版 → 自包含 JSON → 下载文件
///
/// 两个加载器的 meta 接口返回同构的 profile，安装步骤完全一致，
/// 只有 URL 与日志标签不同（此前 fabric.rs / quilt.rs 各写了一遍）。
pub(crate) async fn install_from_profile(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    profile_url: &str,
    label: &str,
) -> Result<(), String> {
    let client = crate::http::build_http_client(std::time::Duration::from_secs(60))?;

    log::debug!("[{}] 下载 profile: {}", label, profile_url);
    let profile_str = client
        .get(profile_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value = serde_json::from_str(&profile_str)
        .map_err(|e| format!("{} profile 解析失败: {}", label, e))?;

    let vanilla_json = download_vanilla_json_mem(&client, mc_version).await?;
    merge_libraries(&mut profile, &vanilla_json);
    strip_to_self_contained(&mut profile, instance_name);

    // profile 未声明的字段从原版补齐
    if profile.get("mainClass").is_none() {
        if let Some(mc) = vanilla_json.get("mainClass").cloned() {
            profile["mainClass"] = mc;
        }
    }
    if profile.get("assets").is_none() {
        if let Some(a) = vanilla_json.get("assets").cloned() {
            profile["assets"] = a;
        }
    }
    if profile.get("assetIndex").is_none() && !vanilla_json["assetIndex"].is_null() {
        profile["assetIndex"] = vanilla_json["assetIndex"].clone();
    }

    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&profile).unwrap_or(profile_str),
    )
    .map_err(|e| e.to_string())?;

    download_libs_and_client_jar(app, mc_dir, &profile, instance_name, &vanilla_json).await;

    log::info!("[{}] 安装完成: {}", label, instance_name);
    Ok(())
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
