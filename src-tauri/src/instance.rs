//! 实例管理：解析版本、列表、删除、修复

use crate::utils::{detect_loader, read_json_field};
use tauri::Emitter;

/// HMCL-style: 解析版本 JSON 并合并 inheritsFrom 父版本的数据
pub(crate) fn resolve_version(
    dot_minecraft: &std::path::Path,
    instance_name: &str,
) -> Result<serde_json::Value, String> {
    let json_path = dot_minecraft
        .join("versions")
        .join(instance_name)
        .join(format!("{}.json", instance_name));
    let cache_path = crate::config::teas_dir()
        .ok()
        .map(|d| d.join("vanilla").join(format!("{}.json", instance_name)));
    let read_path = if json_path.exists() {
        &json_path
    } else if cache_path.as_ref().map_or(false, |p| p.exists()) {
        cache_path.as_ref().unwrap()
    } else {
        &json_path
    };
    let content =
        std::fs::read_to_string(read_path).map_err(|e| format!("版本 JSON 不存在: {}", e))?;
    let mut version: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("JSON 解析错误: {}", e))?;

    // 递归解析 inheritsFrom
    if let Some(parent_name) = version["inheritsFrom"].as_str().map(|s| s.to_string()) {
        if parent_name != instance_name {
            let parent = resolve_version(dot_minecraft, &parent_name)?;
            if let Some(parent_libs) = parent["libraries"].as_array() {
                let cur_libs = version["libraries"].as_array().cloned().unwrap_or_default();
                let mut merged: Vec<serde_json::Value> = parent_libs.iter().cloned().collect();
                merged.extend(cur_libs);
                version["libraries"] = serde_json::Value::Array(merged);
            }
            if version["mainClass"].is_null() {
                version["mainClass"] = parent["mainClass"].clone();
            }
            if version["assets"].is_null() {
                version["assets"] = parent["assets"].clone();
            }
            if version["assetIndex"].is_null() && !parent["assetIndex"].is_null() {
                version["assetIndex"] = parent["assetIndex"].clone();
            }
            if let Some(parent_args) = parent["arguments"].as_object() {
                if version["arguments"].is_null() {
                    version["arguments"] = parent["arguments"].clone();
                } else if let Some(cur_args) = version["arguments"].as_object_mut() {
                    let cur_game = cur_args.get("game").cloned().unwrap_or(serde_json::Value::Array(vec![]));
                    let parent_game = parent_args.get("game").cloned().unwrap_or(serde_json::Value::Array(vec![]));
                    if let (Some(cur_arr), Some(parent_arr)) = (cur_game.as_array(), parent_game.as_array()) {
                        let mut merged = parent_arr.clone();
                        merged.extend(cur_arr.clone());
                        cur_args.insert("game".to_string(), serde_json::Value::Array(merged));
                    } else if cur_game.is_null() {
                        cur_args.insert("game".to_string(), parent_game);
                    }
                }
            }
        }
    }
    Ok(version)
}

/// PCL-style: 从版本 JSON 中检测 MC 版本号
///
/// 参照 PCL McVersion.VanillaName 的完整检测链:
/// 1. clientVersion
/// 2. inheritsFrom
/// 3. HMCL patches[id=game].version
/// 4. Forge --fml.mcVersion
/// 5. Fabric: fabricmc:intermediary:VERSION
/// 6. Quilt: quiltmc:intermediary:VERSION
/// 7. Forge: net.minecraftforge:forge:VERSION
/// 8. OptiFine: optifine:OptiFine:VERSION
/// 9. jar 字段
fn detect_mc_version(version_dir: &std::path::Path, instance_name: &str) -> Option<String> {
    let json_path = version_dir.join(format!("{}.json", instance_name));
    let content = std::fs::read_to_string(&json_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    // 1) clientVersion (PCL/官方格式)
    if let Some(v) = json["clientVersion"].as_str() { return Some(v.to_string()); }

    // 2) inheritsFrom
    if let Some(v) = json["inheritsFrom"].as_str() {
        if !v.is_empty() { return Some(v.to_string()); }
    }

    // 3) HMCL patches[id=game].version
    if let Some(patches) = json["patches"].as_array() {
        for p in patches {
            if p.get("id").and_then(|i| i.as_str()) == Some("game") {
                if let Some(v) = p["version"].as_str() { return Some(v.to_string()); }
            }
        }
    }

    // 4) Forge --fml.mcVersion
    if let Some(game_args) = json["arguments"]["game"].as_array() {
        let mut next_is_ver = false;
        for arg in game_args {
            if let Some(s) = arg.as_str() {
                if next_is_ver { return Some(s.to_string()); }
                if s == "--fml.mcVersion" { next_is_ver = true; }
            }
        }
    }

    // 5-8) 从 libraries 中提取 (Fabric/Quilt/Forge/OptiFine)
    if let Some(libs) = json["libraries"].as_array() {
        for lib in libs {
            let name = lib["name"].as_str().unwrap_or("");
            // Fabric: fabricmc:intermediary:VERSION
            if name.starts_with("net.fabricmc:intermediary:") || name.starts_with("fabricmc:intermediary:") {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() >= 3 { return Some(parts[2].to_string()); }
            }
            // Quilt: quiltmc:intermediary:VERSION
            if name.starts_with("org.quiltmc:intermediary:") || name.starts_with("quiltmc:intermediary:") {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() >= 3 { return Some(parts[2].to_string()); }
            }
            // Forge
            if name.starts_with("net.minecraftforge:forge:") {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() >= 3 {
                    let v = parts[2];
                    // forge:X.Y.Z-W → X.Y.Z
                    if let Some(dash) = v.find('-') { return Some(v[..dash].to_string()); }
                    return Some(v.to_string());
                }
            }
            // OptiFine
            if name.starts_with("optifine:OptiFine:") {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() >= 3 {
                    let v = parts[2].strip_prefix("HD_U_").unwrap_or(parts[2]);
                    if let Some(underscore) = v.find('_') { return Some(v[..underscore].to_string()); }
                    return Some(v.to_string());
                }
            }
        }
    }

    // 9) jar 字段
    if let Some(v) = json["jar"].as_str() {
        if !v.is_empty() { return Some(v.to_string()); }
    }

    // 10) PCL: 从主 JAR 内嵌的 version.json 读取
    let jar_path = version_dir.join(format!("{}.jar", instance_name));
    if jar_path.exists() {
        if let Ok(file) = std::fs::File::open(&jar_path) {
            if let Ok(mut archive) = zip::ZipArchive::new(file) {
                if let Ok(entry) = archive.by_name("version.json") {
                    use std::io::Read;
                    let mut buf = String::new();
                    if let Ok(_) = std::io::BufReader::new(entry).read_to_string(&mut buf) {
                        if let Ok(vj) = serde_json::from_str::<serde_json::Value>(&buf) {
                            if let Some(id) = vj["id"].as_str() {
                                return Some(id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// 修复实例 — 补全缺失文件
#[tauri::command]
pub(crate) async fn fix_instance(
    app: tauri::AppHandle,
    mc_dir: String,
    instance_name: String,
    source: String,
) -> Result<String, String> {
    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let prefer_official = source == "Mojang";

    // 解析版本 JSON
    let version = resolve_version(&dot_minecraft, &instance_name)?;
    let mut fixed = 0u32;

    // 1) 下载缺失的库文件
    let libs = crate::version::library::mclib_from_instance(&version, &dot_minecraft, prefer_official);
    if !libs.is_empty() {
        app.emit("download-progress", serde_json::json!({
            "filename": &instance_name,
            "downloaded": 0, "total": libs.len(), "percent": 0,
            "step": format!("修复实例: 下载 {} 个库文件...", libs.len())
        })).ok();
        let mut files = libs;
        let result = crate::download::engine::download_files_parallel(&app, &mut files, 8).await;
        fixed += result.success as u32;
    }

    // 2) 下载缺失的资源文件
    let index_name = crate::version::assets::mcassets_get_index_name(&version);
    if let Ok(assets) = crate::version::assets::mcassets_fix_list(
        &dot_minecraft, &index_name, true, prefer_official
    ) {
        if !assets.is_empty() {
            app.emit("download-progress", serde_json::json!({
                "filename": &instance_name,
                "downloaded": 0, "total": assets.len(), "percent": 0,
                "step": format!("修复实例: 下载 {} 个资源文件...", assets.len())
            })).ok();
            let mut files = assets;
            let result = crate::download::engine::download_files_parallel(&app, &mut files, 8).await;
            fixed += result.success as u32;
        }
    }

    Ok(format!("修复完成: {} 个文件已补全", fixed))
}

/// 列出所有实例
#[tauri::command]
pub(crate) fn list_instances(mc_dir: String) -> Result<Vec<serde_json::Value>, String> {
    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let versions_dir = dot_minecraft.join("versions");
    if !versions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut instances = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let loader = detect_loader(&path).to_string();
            let mods = if let Ok(md) = std::fs::read_dir(path.join("mods")) {
                md.filter(|e| {
                    e.as_ref()
                        .ok()
                        .and_then(|e| e.path().extension().map(|x| x == "jar"))
                        .unwrap_or(false)
                })
                .count()
            } else {
                0
            };
            let last_played = path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let diff = std::time::SystemTime::now()
                        .duration_since(t)
                        .unwrap_or_default()
                        .as_secs();
                    if diff < 60 { "刚刚".into() }
                    else if diff < 3600 { format!("{} 分钟前", diff / 60) }
                    else if diff < 86400 { format!("{} 小时前", diff / 3600) }
                    else { format!("{} 天前", diff / 86400) }
                })
                .unwrap_or_else(|| "从未".to_string());
            let mc_version = detect_mc_version(&path, &name)
                .unwrap_or_else(|| name.clone());
            instances.push(serde_json::json!({
                "name": name,
                "version": mc_version,
                "mods": mods,
                "loader": loader == "Vanilla",
                "loaderName": &loader,
                "lastPlayed": last_played,
                "active": instances.is_empty()
            }));
        }
    }
    Ok(instances)
}

/// 删除指定实例
#[tauri::command]
pub(crate) fn delete_instance(
    mc_dir: String,
    instance_name: String,
) -> Result<(), String> {
    let base = std::path::Path::new(&mc_dir);
    let dot_minecraft = if base.join(".minecraft").exists() {
        base.join(".minecraft")
    } else {
        base.to_path_buf()
    };
    let version_dir = dot_minecraft.join("versions").join(&instance_name);
    if !version_dir.exists() {
        return Err(format!("实例目录不存在: {}", version_dir.display()));
    }
    std::fs::remove_dir_all(&version_dir).map_err(|e| format!("删除失败: {}", e))
}
