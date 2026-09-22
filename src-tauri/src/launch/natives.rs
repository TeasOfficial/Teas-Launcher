use super::LaunchContext;
use crate::version::library::json_rule_check;
use super::args::apply_rules;
use std::path::{Path, PathBuf};

// ── Natives 准备 ────────────────────────────────────────────

/// Natives 解压结果: base_dir 用于 ${natives_directory} 占位符
/// （实际解压目录可能是 base/java 子目录，Forge 1.17+ 重定向）
pub(super) struct NativesResult {
    pub(super) base_dir: PathBuf,
}

/// PCL-style: 清空 natives 目录后重新解压所有 native DLL
///
/// 关键: 扫描版本 JSON jvm 参数中的 -Djava.library.path=${natives_directory}/<sub>
/// （Forge 1.17+ 使用 ${natives_directory}/java），把解压目录重定向到子目录，
/// 否则 LWJGL 在 java.library.path 指向的子目录中找不到 DLL
pub(super) fn prepare_natives(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<NativesResult, String> {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let mut base_dir = version_dir.join(format!("{}-natives", ctx.instance_name));

    // PCL GetNativesFolder: 非 ASCII 路径回退
    if !is_ascii_path(&base_dir) {
        log::warn!("[launch] Natives 路径含非 ASCII 字符，使用回退路径");
        let appdata = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        let fallback1 = appdata.join(".minecraft").join("bin").join("natives");
        if is_ascii_path(&fallback1) {
            base_dir = fallback1;
        } else {
            let fallback2 = PathBuf::from("C:\\ProgramData\\TeasLauncher\\natives");
            std::fs::create_dir_all(&fallback2).ok();
            if is_ascii_path(&fallback2) {
                base_dir = fallback2;
            }
        }
    }

    // ★ natives 子目录重定向（Forge 1.17+）
    let actual_dir = match natives_subdir(version) {
        Some(sub) => base_dir.join(sub),
        None => base_dir.clone(),
    };

    std::fs::create_dir_all(&actual_dir)
        .map_err(|e| format!("创建 natives 目录失败: {}", e))?;

    // ★ 每次启动都清空并重新解压 Natives
    log::debug!("[launch] 开始解压 Natives -> {}", actual_dir.display());
    clean_directory(&actual_dir)?;

    let libs_dir = ctx.dot_minecraft.join("libraries");
    let existing_files = extract_all_natives(version, &libs_dir, &actual_dir)?;

    cleanup_orphaned_files(&actual_dir, &existing_files)?;

    log::debug!("[launch] Natives 解压完成: {} 个文件", existing_files.len());

    Ok(NativesResult { base_dir })
}

/// 扫描版本 JSON jvm 参数（rules 过滤后），提取**第一条** -Djava.library.path 的子目录
///
/// 语义与 deduplicate_jvm_args 的受保护 key 完全一致（保留最先出现的 libpath），
/// 保证"解压位置"与"JVM 实际生效参数"不脱节:
/// - 第一条 libpath 为 ${natives_directory}（无子目录）→ None → 解压到 base
/// - 第一条 libpath 为 ${natives_directory}/java → Some("java") → 解压到 base/java
pub(super) fn natives_subdir(version: &serde_json::Value) -> Option<String> {
    /// 检查单条参数是否为 libpath；返回 Some(子目录或空) = 是 libpath，None = 不是
    fn libpath_subdir(s: &str) -> Option<String> {
        let rest = s.strip_prefix("-Djava.library.path=")?;
        if let Some(sub) = rest.strip_prefix("${natives_directory}/") {
            if !sub.is_empty() && !sub.contains('/') && !sub.contains('\\') && !sub.contains("..") {
                return Some(sub.to_string());
            }
        }
        Some(String::new()) // libpath 存在但无子目录
    }
    fn scan_value(v: &serde_json::Value) -> Option<String> {
        if let Some(s) = v.as_str() {
            return libpath_subdir(s);
        }
        if let Some(arr) = v.as_array() {
            for item in arr {
                if let Some(r) = scan_value(item) {
                    return Some(r);
                }
            }
        }
        None
    }

    if let Some(jvm) = version["arguments"]["jvm"].as_array() {
        for arg in jvm {
            let found = if let Some(s) = arg.as_str() {
                libpath_subdir(s)
            } else if let Some(obj) = arg.as_object() {
                // ★ rules 过滤（与 collect_json_args 一致，否则解压位置与生效参数错位）
                if !apply_rules(obj) {
                    continue;
                }
                obj.get("value").and_then(scan_value)
            } else {
                None
            };
            if let Some(r) = found {
                // 第一条 libpath 定案: 空串 = 无子目录
                return if r.is_empty() { None } else { Some(r) };
            }
        }
    }
    None
}

/// 判断 library 是否为当前平台（Windows）需要解压的 native 库
pub(super) fn is_native_lib(lib: &serde_json::Value) -> bool {
    if let Some(natives) = lib.get("natives") {
        // natives map 存在但无 windows 键 → 非 Windows 平台库
        return natives.get("windows").is_some();
    }
    if let Some(name) = lib["name"].as_str() {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 4 {
            let c = parts[3].replace("${arch}", "64");
            return c.starts_with("natives-windows");
        }
    }
    false
}

/// 从所有 native library JAR 中提取 DLL 文件（含 rules 过滤）
pub(super) fn extract_all_natives(
    version: &serde_json::Value,
    libs_dir: &Path,
    natives_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut existing_files: Vec<PathBuf> = Vec::new();
    let libs = match version["libraries"].as_array() {
        Some(l) => l,
        None => return Ok(existing_files),
    };

    // 收集所有 native library（rules 过滤 + Windows classifier 判断）
    let native_libs: Vec<&serde_json::Value> = libs.iter().filter(|lib| {
        json_rule_check(lib.get("rules")) && is_native_lib(lib)
    }).collect();

    for lib in &native_libs {
        let name = lib["name"].as_str().unwrap_or("");

        let lib_path = get_library_path(name, lib, libs_dir);
        if lib_path.is_none() {
            continue;
        }
        let jar_path = lib_path.unwrap();

        if !jar_path.exists() {
            log::warn!("[launch] Natives JAR 不存在: {}", jar_path.display());
            continue;
        }

        // 获取 extract 规则
        let extract_rules = lib.get("extract").cloned();

        match std::fs::File::open(&jar_path) {
            Ok(file) => {
                match zip::ZipArchive::new(file) {
                    Ok(mut archive) => {
                        for i in 0..archive.len() {
                            if let Ok(mut entry) = archive.by_index(i) {
                                let entry_name = entry.name().to_string();

                                // 跳过目录
                                if entry.is_dir() {
                                    continue;
                                }

                                // 获取文件名
                                let dest_name = Path::new(&entry_name)
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string());

                                let dest_name = match dest_name {
                                    Some(n) => n,
                                    None => continue,
                                };

                                // 检查 extract rules
                                if let Some(ref rules) = extract_rules {
                                    if let Some(exclude) = rules.get("exclude").and_then(|e| e.as_array()) {
                                        if exclude.iter().any(|s| s.as_str() == Some(&entry_name)) {
                                            continue;
                                        }
                                    }
                                }

                                // 只提取 native 库文件
                                let is_native_file = dest_name.ends_with(".dll")
                                    || dest_name.ends_with(".so")
                                    || dest_name.ends_with(".dylib")
                                    || dest_name.ends_with(".jnilib");
                                if !is_native_file {
                                    continue;
                                }

                                // 跳过 .sha1 和 .git 文件
                                if dest_name.ends_with(".sha1") || dest_name.ends_with(".git") {
                                    continue;
                                }

                                let dest_path = natives_dir.join(&dest_name);

                                // 如果已存在且大小相同则跳过
                                if dest_path.exists() {
                                    if let Ok(meta) = std::fs::metadata(&dest_path) {
                                        if meta.len() == entry.size() {
                                            existing_files.push(dest_path);
                                            continue;
                                        }
                                    }
                                    let _ = std::fs::remove_file(&dest_path);
                                }

                                let mut out = std::fs::File::create(&dest_path)
                                    .map_err(|e| format!(
                                        "创建文件失败 {}: {}",
                                        dest_path.display(), e
                                    ))?;
                                std::io::copy(&mut entry, &mut out)
                                    .map_err(|e| format!("解压失败: {}", e))?;
                                existing_files.push(dest_path);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "[launch] 无法打开 Natives JAR (可能已损坏): {} ({})",
                            jar_path.display(), e
                        );
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[launch] 无法读取 Natives JAR: {} ({})",
                    jar_path.display(), e
                );
            }
        }
    }

    Ok(existing_files)
}

/// 根据 Maven 命名获取 library 的实际文件路径
pub(super) fn get_library_path(name: &str, lib: &serde_json::Value, libs_dir: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }

    let (group, artifact, version_str) = (parts[0], parts[1], parts[2]);

    // 确定 classifier
    let classifier = if parts.len() >= 4 {
        let raw = parts[3];
        // ${arch} 替换
        let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
        raw.replace("${arch}", arch)
    } else {
        // 从 natives 字段获取
        if let Some(natives_obj) = lib.get("natives") {
            let os_key = if cfg!(target_os = "windows") { "windows" }
                else if cfg!(target_os = "macos") { "osx" }
                else { "linux" };
            natives_obj.get(os_key)
                .and_then(|v| v.as_str())
                .map(|c| {
                    let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
                    c.replace("${arch}", arch)
                })?
                .to_string()
        } else {
            return None;
        }
    };

    let jar_name = if classifier.is_empty() {
        format!("{}-{}.jar", artifact, version_str)
    } else {
        format!("{}-{}-{}.jar", artifact, version_str, classifier)
    };

    Some(
        libs_dir
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version_str)
            .join(jar_name)
    )
}

/// PCL-style: 删除 natives 目录中的多余文件
pub(super) fn cleanup_orphaned_files(
    natives_dir: &Path,
    keep_files: &[PathBuf],
) -> Result<(), String> {
    if !natives_dir.exists() {
        return Ok(());
    }

    if let Ok(entries) = std::fs::read_dir(natives_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && !keep_files.contains(&path) {
                log::debug!("[launch] 删除孤立 natives 文件: {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

/// HMCL-style: cleanDirectoryQuietly
pub(super) fn clean_directory(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

pub(super) fn is_ascii_path(p: &Path) -> bool {
    p.to_string_lossy().chars().all(|c| c.is_ascii())
}

