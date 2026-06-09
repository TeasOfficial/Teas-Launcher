//! Forge 安装器 — 参照 PCL-CE McDownloadForgelikeLoader
//!
//! 新版本 Forge (1.13+, version >= 20.x):
//! 1. 下载 installer.jar (BMCLAPI → Official)
//! 2. 解压 install_profile.json + version.json
//! 3. 解析 libraries → 下载
//! 4. 运行 Forge 安装器 Java 进程
//! 5. 拷贝生成的 JSON 到目标
//!
//! 旧版 Forge (1.12-, version < 20.x):
//! - Legacy 方式1: 解压 json + maven 目录
//! - Legacy 方式2: 提取 JAR + versionInfo

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use std::path::Path;
use std::time::Duration;

/// 安装 Forge 加载器（在已有原版 JSON 的基础上）
pub async fn install_forge_loader(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    forge_version: &str,
) -> Result<(), String> {
    let _client = crate::http::build_http_client(Duration::from_secs(600))?;
    let mc_clean = mc_version.replace('-', "_");
    let tmp_dir = mc_dir.join(format!(".tmp_frg_{}", instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let installer_path = tmp_dir.join("forge_installer.jar");

    // 1) 下载 installer.jar
    let installer_urls: Vec<String> = vec![
        format!(
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar",
            mc_clean, forge_version, mc_clean, forge_version
        ),
        format!(
            "https://files.minecraftforge.net/maven/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar",
            mc_clean, forge_version, mc_clean, forge_version
        ),
        format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar",
            mc_clean, forge_version, mc_clean, forge_version
        ),
    ];

    let mut installer_dl = DownloadFile::new(
        installer_urls,
        installer_path.clone(),
        FileChecker::with_min_size(64 * 1024),
    );

    download_file(app, &mut installer_dl, false).await?;
    eprintln!("[forge] installer.jar 已下载");

    // 2) 解压并读取 install_profile.json
    let archive_file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive =
        zip::ZipArchive::new(archive_file).map_err(|e| format!("无法读取 installer: {}", e))?;

    let profile = {
        let entry = archive
            .by_name("install_profile.json")
            .map_err(|_| "installer 中无 install_profile.json".to_string())?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(entry), &mut s)
            .map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&s).map_err(|e| e.to_string())?
    };

    let spec = profile.get("spec").and_then(|s| s.as_u64()).unwrap_or(0);
    let version_major: f64 = forge_version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    let is_new_forge = spec > 0 || version_major >= 20.0;

    if is_new_forge {
        install_forge_new(app, mc_dir, instance_name, mc_version, forge_version, &mut archive, &profile, &tmp_dir).await
    } else {
        install_forge_legacy(mc_dir, instance_name, mc_version, forge_version, &mut archive, &profile, &tmp_dir)
    }
}

/// 新版 Forge 安装 (1.13+)
async fn install_forge_new(
    app: &tauri::AppHandle,
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    forge_version: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    profile: &serde_json::Value,
    tmp_dir: &Path,
) -> Result<(), String> {
    // 1) 提取 version.json 并合并到 install_profile 数据
    let version_entry = archive
        .by_name("version.json")
        .map_err(|_| "installer 中无 version.json".to_string())?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::BufReader::new(version_entry), &mut s)
        .map_err(|e| e.to_string())?;
    let version_json: serde_json::Value =
        serde_json::from_str(&s).map_err(|e| e.to_string())?;

    // PCL-CE json.Merge(json2): System.Text.Json.Nodes.JsonObject.Merge
    // JsonArray 拼接，普通值补充
    let mut merged = profile.clone();
    // libraries: PCL-CE Merge 拼接两个数组
    if let Some(v_libs) = version_json.get("libraries").and_then(|v| v.as_array()).cloned() {
        let mut profile_libs = merged.get("libraries")
            .and_then(|v| v.as_array()).cloned().unwrap_or_default();
        profile_libs.extend(v_libs);
        merged["libraries"] = serde_json::Value::Array(profile_libs);
    }
    // 普通字段
    if merged.get("mainClass").is_none() {
        if let Some(mc) = version_json.get("mainClass") { merged["mainClass"] = mc.clone(); }
    }
    if merged.get("arguments").is_none() {
        if let Some(args) = version_json.get("arguments") { merged["arguments"] = args.clone(); }
    }
    if let Some(id) = version_json.get("id") {
        merged["id"] = id.clone();
    } else {
        merged["id"] = serde_json::Value::String(format!("{}-forge-{}", mc_version, forge_version));
    }
    merged["inheritsFrom"] = serde_json::Value::String(mc_version.to_string());

    // 2) 解析 libraries → 下载
    let libs = crate::version::library::mclib_list_from_json(&merged, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }

    // 3) 提取内嵌 Maven 库
    extract_embedded_libs(archive, mc_dir);

    // 4) 保存合并后的 version JSON
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let json_path = target.join(format!("{}.json", instance_name));
    let _ = std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&merged).unwrap_or_default(),
    );

    // 5) 尝试运行 Forge installer Java 进程
    // PCL-CE 用了 JavaWrapper来处理中文路径，这里简化：尝试直接运行
    if let Err(e) = run_forge_installer(mc_dir, tmp_dir, mc_version, forge_version).await {
        eprintln!("[forge] 运行 Forge installer 失败 (非致命): {}", e);
    }

    // 清理
    let _ = std::fs::remove_dir_all(tmp_dir);
    eprintln!("[forge] 新版 Forge 安装完成: {}-forge-{}", mc_version, forge_version);
    Ok(())
}

/// 提取 JAR 内嵌的 Maven 库
fn extract_embedded_libs(
    archive: &mut zip::ZipArchive<std::fs::File>,
    mc_dir: &Path,
) {
    let libs_dir = mc_dir.join("libraries");
    let maven_entries: Vec<(String, std::path::PathBuf)> = {
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.starts_with("maven/") && !name.ends_with('/') {
                    let rel = name.trim_start_matches("maven/");
                    let dest = libs_dir.join(rel);
                    if !dest.exists() {
                        entries.push((name, dest));
                    }
                }
            }
        }
        entries
    };

    for (name, dest) in maven_entries {
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

/// 运行 Forge installer 进程
async fn run_forge_installer(
    mc_dir: &Path,
    tmp_dir: &Path,
    _mc_version: &str,
    _forge_version: &str,
) -> Result<(), String> {
    let installer = tmp_dir.join("forge_installer.jar");
    if !installer.exists() {
        return Err("installer.jar 不存在".to_string());
    }

    let mc_str = mc_dir.to_string_lossy().to_string();
    let installer_str = installer.to_string_lossy().to_string();
    let args = vec![
        "-jar",
        installer_str.as_str(),
        "--installClient",
        &mc_str,
    ];

    let output = std::process::Command::new("java")
        .args(&args)
        .output()
        .map_err(|e| format!("运行 Java 失败: {} (请确保 Java 已安装)", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    if !output.status.success() {
        eprintln!("[forge] installer stderr:\n{}", stderr);
        return Err(format!("Forge installer 失败: {}", stderr.lines().last().unwrap_or("")));
    }

    eprintln!("[forge] installer stdout:\n{}", stdout);
    Ok(())
}

/// 旧版 Forge 安装 (1.12-)
fn install_forge_legacy(
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
    forge_version: &str,
    archive: &mut zip::ZipArchive<std::fs::File>,
    profile: &serde_json::Value,
    tmp_dir: &Path,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    if profile.get("install").is_some() && profile.get("versionInfo").is_some() {
        // Legacy 方式2: 提取 JAR + 用 versionInfo 生成 JSON
        let install = &profile["install"];
        let file_path = install["filePath"].as_str().unwrap_or("");

        if !file_path.is_empty() {
            let dest_rel = if let Some(path_str) = install["path"].as_str() {
                let path_parts: Vec<&str> = path_str.split(':').collect();
                if path_parts.len() >= 3 {
                    format!(
                        "{}/{}/{}/{}-{}.jar",
                        path_parts[0].replace('.', "/"),
                        path_parts[1],
                        path_parts[2],
                        path_parts[1],
                        path_parts[2]
                    )
                } else {
                    file_path.trim_start_matches("maven/").to_string()
                }
            } else {
                file_path.trim_start_matches("maven/").to_string()
            };

            let dest = mc_dir.join("libraries").join(&dest_rel);
            if let Some(p) = dest.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if let Ok(mut entry) = archive.by_name(file_path) {
                if let Ok(mut out) = std::fs::File::create(&dest) {
                    let _ = std::io::copy(&mut entry, &mut out);
                }
            }
        }

        let mut vi = profile["versionInfo"].clone();
        vi["id"] = serde_json::Value::String(instance_name.to_string());
        if vi.get("inheritsFrom").is_none() {
            vi["inheritsFrom"] = serde_json::Value::String(mc_version.to_string());
        }
        let _ = std::fs::write(
            target.join(format!("{}.json", instance_name)),
            serde_json::to_string_pretty(&vi).unwrap_or_default(),
        );
    } else if let Some(json_path) = profile["json"].as_str() {
        // Legacy 方式1: 解压 json + maven 目录
        let json_path = json_path.trim_start_matches('/');
        if let Ok(mut entry) = archive.by_name(json_path) {
            let mut s = String::new();
            if std::io::Read::read_to_string(&mut std::io::BufReader::new(&mut entry), &mut s).is_ok() {
                let mut vj: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
                vj["id"] = serde_json::Value::String(instance_name.to_string());
                let _ = std::fs::write(
                    target.join(format!("{}.json", instance_name)),
                    serde_json::to_string_pretty(&vj).unwrap_or_default(),
                );
            }
        }

        // 解压 maven/ 目录
        let _unrar_dir = tmp_dir.join("_unrar");
        extract_maven_dir(archive, mc_dir);
    }

    let _ = std::fs::remove_dir_all(tmp_dir);
    eprintln!("[forge] 旧版 Forge 安装完成: {}-forge-{}", mc_version, forge_version);
    Ok(())
}

fn extract_maven_dir(
    archive: &mut zip::ZipArchive<std::fs::File>,
    mc_dir: &Path,
) {
    let entries: Vec<(String, std::path::PathBuf)> = {
        let mut result = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.starts_with("maven/") && !name.ends_with('/') {
                    let rel = name.trim_start_matches("maven/");
                    let dest = mc_dir.join("libraries").join(rel);
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
