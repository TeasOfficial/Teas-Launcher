//! Library 管理 — 参照 PCL-CE ModLibrary.cs
//!
//! 核心功能:
//! - McLibToken: 库文件描述 (name, url, local_path, sha1, is_natives)
//! - mclib_get: Maven 坐标 → 本地文件路径
//! - json_rule_check: OS/arch/feature 规则检查
//! - mclib_list_get: 递归 inheritsFrom 获取完整库列表
//! - mclib_netfiles: 将 McLibToken 列表转为 DownloadFile 下载任务
//! - mclib_from_instance: 便捷函数，从实例获取需下载的 DownloadFile 列表

use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::{source_library, source_maven_bmclapi};
use std::path::{Path, PathBuf};

/// 库文件描述 — 参照 PCL-CE McLibToken
#[derive(Debug, Clone)]
pub struct McLibToken {
    /// PCL-CE OriginalName: group:artifact:version[:classifier]
    pub original_name: String,
    /// PCL-CE Name: group:artifact (不含版本号)
    pub name: String,
    /// 本地完整路径
    pub local_path: PathBuf,
    /// JSON 中提供的 URL (可能为 None)
    pub url: Option<String>,
    /// 文件大小 (0 表示未知)
    pub size: i64,
    /// SHA1 哈希
    pub sha1: Option<String>,
    /// 是否为 Natives 文件
    pub is_natives: bool,
    /// 是否为纯本地文件 (hint=local)
    pub is_local: bool,
}

/// Maven 坐标 → 本地路径
///
/// 参照 PCL-CE McLibGet(original, withHead, ignoreLiteLoader, customMcFolder):
/// ```vb
/// Dim splited = original.Split(":")
/// path = customMcFolder\libraries\{splited[0].Replace(".", "\")}\{splited[1]}\{splited[2]}\{splited[1]}-{splited[2]}.jar
/// ```
pub fn mclib_get(original: &str, mc_dir: &Path) -> PathBuf {
    let parts: Vec<&str> = original.split(':').collect();
    if parts.len() < 3 {
        return mc_dir.join("libraries").join(original);
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];

    mc_dir
        .join("libraries")
        .join(&group)
        .join(artifact)
        .join(version)
        .join(format!("{}-{}.jar", artifact, version))
}

/// 带 classifier 的 Maven 坐标 → 本地路径
///
/// 参照 PCL-CE: natives 库替换 .jar → -{natives}.jar → ${arch} 替换
pub fn mclib_get_with_classifier(original: &str, classifier: &str, mc_dir: &Path) -> PathBuf {
    let parts: Vec<&str> = original.split(':').collect();
    if parts.len() < 3 {
        return mclib_get(original, mc_dir);
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];

    let jar_name = if classifier.is_empty() {
        format!("{}-{}.jar", artifact, version)
    } else {
        // ${arch} 替换 — 参照 PCL-CE
        let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
        let classifier = classifier.replace("${arch}", arch);
        format!("{}-{}-{}.jar", artifact, version, classifier)
    };

    mc_dir
        .join("libraries")
        .join(&group)
        .join(artifact)
        .join(version)
        .join(&jar_name)
}

/// 检查 JSON 中的 Rules 规则 — 参照 PCL-CE McJsonRuleCheck
///
/// ```vb
/// For Each Rule In RuleToken:
///   ' os check
///   ' features check
///   ' action: allow → required=True / disallow → required=False
/// Return required
/// ```
pub fn json_rule_check(rules: Option<&serde_json::Value>) -> bool {
    let rules = match rules {
        Some(r) => r,
        None => return true, // 无 rules → 默认允许
    };

    let arr = match rules.as_array() {
        Some(a) => a,
        None => return true,
    };

    let mut required = false;

    for rule in arr {
        let mut is_right = true;

        // OS 检查
        if let Some(os) = rule.get("os") {
            if let Some(os_name) = os.get("name").and_then(|v| v.as_str()) {
                match os_name {
                    "windows" => {
                        // Windows 平台匹配，检查版本
                        if os.get("version").is_some() {
                            // PCL-CE: 用正则匹配 Windows 版本
                            // 简化：始终匹配
                        }
                    }
                    "osx" | "macos" | "linux" => is_right = false,
                    _ => {}
                }
            }
            // arch 检查
            if let Some(arch) = os.get("arch").and_then(|v| v.as_str()) {
                let is_32bit = cfg!(target_arch = "x86");
                // PCL-CE: arch == "x86" == Is32BitSystem
                is_right = is_right && (arch == "x86") == is_32bit;
            }
        }

        // features 检查
        if let Some(features) = rule.get("features") {
            // 反选: is_demo_user 必须不存在
            if features.get("is_demo_user").is_some() {
                is_right = false;
            }
            // 反选: quick_play 必须不存在 (参照 PCL-CE)
            if let Some(obj) = features.as_object() {
                for key in obj.keys() {
                    if key.contains("quick_play") {
                        is_right = false;
                    }
                }
            }
        }

        // 动作处理
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("allow");
        match action {
            "allow" => {
                if is_right {
                    required = true;
                }
            }
            "disallow" => {
                if is_right {
                    required = false;
                }
            }
            _ => {}
        }
    }

    required
}

/// 解析单个版本的 libraries 列表 (不含 inheritsFrom)
///
/// 参照 PCL-CE McLibListGetWithJson:
/// ```vb
/// For Each LibraryNode In jsonObject("libraries"):
///   ' 清理 null 项
///   ' Rules 检查
///   ' 获取 root url
///   ' 处理 natives/non-natives
///   ' 构建 McLibToken
/// ```
pub fn mclib_list_from_json(
    json: &serde_json::Value,
    mc_dir: &Path,
) -> Vec<McLibToken> {
    let mut result = Vec::new();
    let libs = match json.get("libraries").and_then(|v| v.as_array()) {
        Some(l) => l,
        None => return result,
    };

    for lib in libs {
        let lib = match lib.as_object() {
            Some(o) => o.clone(),
            None => continue,
        };

        // Rules 检查
        if !json_rule_check(lib.get("rules")) {
            continue;
        }

        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            continue;
        }

        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            continue;
        }

        let name_key = format!("{}:{}", parts[0], parts[1]); // group:artifact

        // ★ 判断 classifier: 第4部分 (如 natives-windows, natives-windows-x86)
        let has_classifier = parts.len() >= 4;
        let classifier = if has_classifier { parts[3] } else { "" };

        // 根节点 URL — 参照 PCL-CE: rootUrl + McLibGet(name, False)
        // McLibGet with withHead=False 返回不含 libraries/ 前缀的 Maven 路径
        let root_url = lib.get("url").and_then(|v| v.as_str()).map(|u| {
            let jar_name = if has_classifier {
                format!("{}-{}-{}.jar", parts[1], parts[2], classifier)
            } else {
                format!("{}-{}.jar", parts[1], parts[2])
            };
            let maven_rel = format!(
                "{}/{}/{}/{}",
                parts[0].replace('.', "/"),
                parts[1],
                parts[2],
                jar_name
            );
            format!("{}/{}", u.trim_end_matches('/'), maven_rel.trim_start_matches('/'))
        });

        // 纯本地文件
        let is_local = lib
            .get("hint")
            .and_then(|v| v.as_str())
            .map(|h| h == "local")
            .unwrap_or(false);

        // PCL-CE 精确对照: ModLibrary.cs 第 252-330 行
        // Natives vs non-natives
        if lib.get("natives").is_none() {
            // 没有 Natives — 普通库
            // ★ 有 classifier 的名称使用 mclib_get_with_classifier 获取正确路径
            let local_path = if has_classifier {
                mclib_get_with_classifier(name, classifier, mc_dir)
            } else {
                mclib_get(name, mc_dir)
            };
            let (url, sha1, size) = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .map(|art| {
                    let url = art.get("url").and_then(|u| u.as_str())
                        .map(|u| u.to_string())
                        .or_else(|| root_url.clone());
                    let sha1 = art.get("sha1").and_then(|s| s.as_str()).map(|s| s.to_string());
                    let size = art.get("size").and_then(|s| s.as_i64()).unwrap_or(0);
                    (url, sha1, size)
                })
                .unwrap_or((root_url, None, 0));
            result.push(McLibToken {
                original_name: name.to_string(), name: name_key, local_path,
                url, size, sha1, is_natives: false, is_local,
            });
        } else if lib["natives"]["windows"].is_null() {
            // 有 natives 但无 windows → 跳过（非 Windows 平台）
        } else {
            // PCL-CE: 有 Windows Natives (ModLibrary.cs 第 296 行)
            let natives_classifier = lib["natives"]["windows"].as_str().unwrap_or("natives-windows");
            let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };

            // PCL-CE 第 300-318 行: 优先用 downloads.classifiers["natives-windows"] 的 path
            let has_classifier_dl = lib.get("downloads")
                .and_then(|d| d.get("classifiers"))
                .and_then(|c| c.get("natives-windows"))
                .is_some();

            let (url, local_path, sha1, size) = if has_classifier_dl {
                let nw = &lib["downloads"]["classifiers"]["natives-windows"];
                let url = nw["url"].as_str().map(|u| u.to_string())
                    .unwrap_or_else(|| root_url.clone().unwrap_or_default());
                let sha1 = nw["sha1"].as_str().map(|s| s.to_string());
                let size = nw["size"].as_i64().unwrap_or(0);
                // PCL-CE 第 306-312 行
                let local_path = if nw.get("path").is_none() {
                    // 无 path → McLibGet 再 .Replace(".jar", "-" + classifier + ".jar")
                    let base = mclib_get(name, mc_dir);
                    let base_str = base.to_string_lossy();
                    std::path::PathBuf::from(
                        base_str.replace(".jar",
                            &format!("-{}.jar", natives_classifier))
                            .replace("${arch}", arch)
                    )
                } else {
                    // 有 path → 直接用
                    mc_dir.join("libraries").join(
                        nw["path"].as_str().unwrap().replace('/', "\\")
                    )
                };
                (Some(url), local_path, sha1, size)
            } else {
                // PCL-CE 第 320-327 行: 无 downloads.classifiers → fallback
                let base = mclib_get(name, mc_dir);
                let base_str = base.to_string_lossy();
                let local_path = std::path::PathBuf::from(
                    base_str.replace(".jar",
                        &format!("-{}.jar", natives_classifier))
                        .replace("${arch}", arch)
                );
                (root_url, local_path, None, 0)
            };

            result.push(McLibToken {
                original_name: name.to_string(),
                name: name_key,
                local_path,
                url,
                size,
                sha1,
                is_natives: true,
                is_local,
            });
        }
    }

    // 去重 — 参照 PCL-CE: 用完整 original_name 做键（含 classifier）
    // ★ BUILD=72 修复: 之前用 name+is_natives 做键，导致不同 classifier 被错误合并
    // 如 lwjgl-glfw:3.4.1:natives-windows 和 lwjgl-glfw:3.4.1:natives-windows-x86 被当成同一个
    let mut deduped: Vec<McLibToken> = Vec::new();
    for token in result {
        // ★ 键 = original_name（含 classifier，如 :natives-windows-x86）
        let key = token.original_name.clone();
        if let Some(existing) = deduped.iter().position(|t| {
            t.original_name == key
        }) {
            // 同名库，保留高版本
            let existing_ver = extract_version(&deduped[existing].local_path);
            let current_ver = extract_version(&token.local_path);
            if current_ver >= existing_ver {
                deduped[existing] = token;
            }
        } else {
            deduped.push(token);
        }
    }

    deduped
}

/// 递归获取完整 libraries 列表 (含 inheritsFrom)
///
/// 参照 PCL-CE McLibListGet:
/// ```vb
/// ' 当前版本 libraries
/// result = McLibListGetWithJson(json)
/// ' 递归 inheritsFrom (需排除 Forge 1.17+)
/// While Not String.IsNullOrEmpty(instance.InheritInstanceName)
///   parent = New McInstance(parentPath)
///   result.AddRange(McLibListGetWithJson(parent.JsonObject))
/// End While
/// ' 添加主 JAR
/// result.Add(clientJar token)
/// ```
pub fn mclib_list_get(
    json: &serde_json::Value,
    mc_dir: &Path,
) -> Vec<McLibToken> {
    let mut result = mclib_list_from_json(json, mc_dir);

    // 递归 inheritsFrom
    if let Some(parent) = json.get("inheritsFrom").and_then(|v| v.as_str()) {
        if !parent.is_empty() {
            let parent_json_path = mc_dir
                .join("versions")
                .join(parent)
                .join(format!("{}.json", parent));
            if let Ok(content) = std::fs::read_to_string(&parent_json_path) {
                if let Ok(parent_json) = serde_json::from_str::<serde_json::Value>(&content) {
                    let parent_libs = mclib_list_from_json(&parent_json, mc_dir);
                    // 简单合并（去重由调用方处理）
                    for pl in parent_libs {
                        let key = format!("{}{}", pl.name, pl.is_natives);
                        if !result.iter().any(|r| {
                            format!("{}{}", r.name, r.is_natives) == key
                        }) {
                            result.push(pl);
                        }
                    }
                }
            }
        }
    }

    result
}

/// 提取版本号用于比较（用本地路径中的目录名）
fn extract_version(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 将 McLibToken 列表转换为 DownloadFile 任务列表
///
/// 参照 PCL-CE McLibNetFilesFromTokens:
/// ```vb
/// For Each Token In Libs:
///   ' 文件校验 (checker.Check)
///   ' 跳过本地文件
///   ' 构建 URL 列表 (官方 + BMCLAPI maven + BMCLAPI libraries)
///   ' 特殊处理: Forge universal, OptiFine, LabyMod
///   Result.Add(New DownloadFile(urls, localPath, checker))
/// ```
pub fn mclib_to_download_files(
    tokens: &[McLibToken],
    prefer_official: bool,
) -> Vec<DownloadFile> {
    let mut result = Vec::new();

    for token in tokens {
        // 文件校验: 如果已存在且通过，跳过
        let checker = FileChecker::new(token.size, token.size, token.sha1.clone());
        if checker.check(&token.local_path).is_none() {
            continue;
        }

        // 跳过本地文件
        if token.is_local {
            eprintln!("[lib] 跳过本地文件: {}", token.original_name);
            continue;
        }

        // 构建 URL 列表
        let mut urls: Vec<String> = Vec::new();

        // 如果 token 有 URL，用它
        if let Some(ref url) = token.url {
            urls.push(url.clone());

            // 添加 BMCLAPI maven 版本
            let bmclapi = source_maven_bmclapi(url);
            if bmclapi != *url && !urls.contains(&bmclapi) {
                if prefer_official {
                    urls.push(bmclapi);
                } else {
                    urls.insert(0, bmclapi);
                }
            }
        }

        // 使用标准 libraries.minecraft.net URL 生成回退源
        let lib_rel = token
            .local_path
            .to_string_lossy()
            .replace('\\', "/");
        let std_url = if let Some(lib_idx) = lib_rel.find("/libraries/") {
            format!(
                "https://libraries.minecraft.net{}",
                &lib_rel[lib_idx + "/libraries".len()..]
            )
        } else {
            format!("https://libraries.minecraft.net/{}", lib_rel)
        };

        let fallback_urls = source_library(&std_url, prefer_official);
        for u in fallback_urls {
            if !urls.contains(&u) {
                urls.push(u);
            }
        }

        result.push(DownloadFile::new(urls, token.local_path.clone(), checker));
    }

    result
}

/// 从实例获取需要下载的库文件列表（便捷函数）
///
/// 参照 PCL-CE McLibNetFilesFromInstance:
/// ```vb
/// ' 1) 主 JAR (DlClientJarGet)
/// ' 2) Library files (McLibNetFilesFromTokens)
/// ' 3) Authlib-Injector
/// ' 4) LabyMod assets
/// ```
pub fn mclib_from_instance(
    json: &serde_json::Value,
    mc_dir: &Path,
    prefer_official: bool,
) -> Vec<DownloadFile> {
    let mut result = Vec::new();

    // 1) 主 JAR
    if let Some(url) = json
        .get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|c| c.get("url"))
        .and_then(|u| u.as_str())
    {
        let sha1 = json
            .get("downloads")
            .and_then(|d| d.get("client"))
            .and_then(|c| c.get("sha1"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());

        let size = json
            .get("downloads")
            .and_then(|d| d.get("client"))
            .and_then(|c| c.get("size"))
            .and_then(|s| s.as_i64())
            .unwrap_or(-1);

        let instance_name = json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let jar_path = mc_dir
            .join("versions")
            .join(instance_name)
            .join(format!("{}.jar", instance_name));

        let checker = FileChecker::new(1024, size, sha1);
        if checker.check(&jar_path).is_some() {
            let jar_urls: Vec<String> = if prefer_official {
                vec![
                    url.to_string(),
                    url.replace(
                        "https://piston-data.mojang.com",
                        "https://bmclapi2.bangbang93.com",
                    ),
                ]
            } else {
                vec![
                    url.replace(
                        "https://piston-data.mojang.com",
                        "https://bmclapi2.bangbang93.com",
                    ),
                    url.to_string(),
                ]
            };
            result.push(DownloadFile::new(jar_urls, jar_path, checker));
        }
    }

    // 2) Library
    let libs = mclib_list_get(json, mc_dir);
    result.append(&mut mclib_to_download_files(&libs, prefer_official));

    result
}
