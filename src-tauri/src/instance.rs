//! 实例管理：解析版本、列表、删除、修复

use crate::utils::detect_loader;
use tauri::Emitter;

/// HMCL-style: 解析版本 JSON 并合并 inheritsFrom 父版本的数据
///
/// 合并规则（子优先）:
/// - libraries: 父在前子在后
/// - arguments.jvm / arguments.game: 父在前子在后（子 JSON 常为差异式，只含自身参数）
/// - minecraftArguments / mainClass / assets / assetIndex / jar / javaVersion: 子为空时用父的
pub(crate) fn resolve_version(
    dot_minecraft: &std::path::Path,
    instance_name: &str,
) -> Result<serde_json::Value, String> {
    resolve_version_impl(dot_minecraft, instance_name, &mut std::collections::HashSet::new(), 0)
}

fn resolve_version_impl(
    dot_minecraft: &std::path::Path,
    instance_name: &str,
    visited: &mut std::collections::HashSet<String>,
    depth: u32,
) -> Result<serde_json::Value, String> {
    // 环检测 + 深度上限（防止 inheritsFrom 循环导致栈溢出）
    if depth > 10 {
        return Err(format!("inheritsFrom 链过深 (>10): {}", instance_name));
    }
    if !visited.insert(instance_name.to_string()) {
        return Err(format!("检测到循环 inheritsFrom: {}", instance_name));
    }

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
            let parent = resolve_version_impl(dot_minecraft, &parent_name, visited, depth + 1)?;

            // libraries: 父在前子在后
            if let Some(parent_libs) = parent["libraries"].as_array() {
                let cur_libs = version["libraries"].as_array().cloned().unwrap_or_default();
                let mut merged: Vec<serde_json::Value> = parent_libs.iter().cloned().collect();
                merged.extend(cur_libs);
                version["libraries"] = serde_json::Value::Array(merged);
            }

            // mainClass / assets / assetIndex / jar / javaVersion: 子优先，为空则继承父
            for field in ["mainClass", "assets", "assetIndex", "jar", "javaVersion"] {
                if version[field].is_null() && !parent[field].is_null() {
                    version[field] = parent[field].clone();
                }
            }

            // minecraftArguments（旧版格式）: 子为空则继承父
            if version["minecraftArguments"].is_null() && !parent["minecraftArguments"].is_null() {
                version["minecraftArguments"] = parent["minecraftArguments"].clone();
            }

            // arguments: jvm + game 均合并（父在前子在后）
            let parent_args = parent["arguments"].as_object().cloned();
            let cur_args = version["arguments"].as_object_mut();
            match (parent_args, cur_args) {
                (Some(pargs), Some(cargs)) => {
                    for key in ["jvm", "game"] {
                        let cur = cargs.get(key).cloned().unwrap_or(serde_json::Value::Array(vec![]));
                        let par = pargs.get(key).cloned().unwrap_or(serde_json::Value::Array(vec![]));
                        match (cur.as_array(), par.as_array()) {
                            (Some(cur_arr), Some(par_arr)) => {
                                let mut merged = par_arr.clone();
                                merged.extend(cur_arr.clone());
                                cargs.insert(key.to_string(), serde_json::Value::Array(merged));
                            }
                            // 子参数缺失/畸形（非数组）→ 直接继承父的
                            (_, Some(par_arr)) => {
                                cargs.insert(key.to_string(), serde_json::Value::Array(par_arr.clone()));
                            }
                            _ => {}
                        }
                    }
                }
                (Some(pargs), None) => {
                    version["arguments"] = serde_json::Value::Object(pargs);
                }
                _ => {}
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

    // ★ 修复取消粘滞: 之前用户取消过下载会导致 CANCEL_DOWNLOAD 永久为 true
    crate::http::reset_cancel();

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

/// 将 Unix 时间戳转为相对时间字符串
fn format_last_played(mtime_secs: u64) -> String {
    // mtime 读取失败时上游填 0，不能当成 1970 年算出"两万天前"
    if mtime_secs == 0 {
        return "从未".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(mtime_secs);
    if diff < 60 { "刚刚".into() }
    else if diff < 3600 { format!("{} 分钟前", diff / 60) }
    else if diff < 86400 { format!("{} 小时前", diff / 3600) }
    else { format!("{} 天前", diff / 86400) }
}

/// 列出所有实例
///
/// 使用 `.teas/instance_index.json` 持久化缓存扫描结果。
/// 仅对目录 mtime 发生变化的实例重新扫描，其余从缓存读取。
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

    // ── 读取缓存索引 ──
    let index_path = crate::config::teas_dir()
        .ok()
        .map(|d| d.join("instance_index.json"));
    let mut index: serde_json::Map<String, serde_json::Value> = index_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .and_then(|v: serde_json::Value| v.as_object().cloned())
        .unwrap_or_default();

    let mut instances = Vec::new();
    let mut index_changed = false;

    // 用户当前选中的实例（存在 .teas/tc.ini 的 launcher scope）。
    // 索引里不再持久化 active —— 那个值只是"首次扫描时排在第一个"，与用户选择无关。
    let active_instance = crate::config::config_read("launcher".to_string())
        .ok()
        .and_then(|c| {
            c.get("active_instance")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if let Ok(entries) = std::fs::read_dir(&versions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            // versions/ 下的任意子目录都会被扫到，但只有带同名 <name>.json 的才是
            // 可启动的版本实例（launch 的 pre_check 同样要求这个文件）。缺它的目录是
            // 残留的游戏目录或半成品，列出来只会让用户点到必然失败的启动按钮。
            if !path.join(format!("{}.json", name)).exists() {
                log::debug!("[instance] 跳过无版本 JSON 的目录: {}", name);
                continue;
            }

            // 获取目录 mtime 作为缓存键
            let mtime = path.metadata().ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
                .unwrap_or(0);

            // ── 检查缓存 ──
            if let Some(cached) = index.get(&name) {
                if cached.get("_mtime").and_then(|v| v.as_u64()) == Some(mtime)
                    && cached.get("version").is_some()
                {
                    // 缓存命中：派生字段一律即时计算，避免随缓存僵化
                    // - lastPlayed 是相对时间，存下来一天后就永久错误
                    // - active 依赖用户当前选择，选择变了缓存不会跟着变
                    // - loader 由 loaderName 派生（旧版本缓存里存的是反的）
                    let mut entry = cached.clone();
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert(
                            "lastPlayed".into(),
                            serde_json::Value::String(format_last_played(mtime)),
                        );
                        obj.insert("active".into(), serde_json::Value::Bool(name == active_instance));
                        let has_loader = cached
                            .get("loaderName")
                            .and_then(|v| v.as_str())
                            .map(|s| s != "Vanilla")
                            .unwrap_or(false);
                        obj.insert("loader".into(), serde_json::Value::Bool(has_loader));
                    }
                    instances.push(entry);

                    // 旧格式缓存里还留着 loader / lastPlayed / active 三个派生字段，
                    // 它们会永远僵化在首次扫描时的值。这里顺手清掉，让文件收敛到干净 schema。
                    if let Some(obj) = index.get_mut(&name).and_then(|v| v.as_object_mut()) {
                        for k in ["loader", "lastPlayed", "active"] {
                            if obj.remove(k).is_some() {
                                index_changed = true;
                            }
                        }
                    }
                    continue;
                }
            }

            // ── 缓存未命中：完整扫描 ──
            index_changed = true;
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
            let mc_version = detect_mc_version(&path, &name)
                .unwrap_or_else(|| "未知".to_string());

            // 只有需要扫盘才能得到的字段才入库。lastPlayed / active 是派生值，
            // 存下来会随缓存一起僵化（每天变化、随用户选择变化），loader 也能由
            // loaderName 直接推出——这三个一律在返回时即时计算。
            index.insert(
                name.clone(),
                serde_json::json!({
                    "name": name,
                    "version": mc_version,
                    "mods": mods,
                    "loaderName": &loader,
                    "_mtime": mtime,
                }),
            );

            instances.push(serde_json::json!({
                "name": name,
                "version": mc_version,
                "mods": mods,
                "loader": loader != "Vanilla",
                "loaderName": &loader,
                "lastPlayed": format_last_played(mtime),
                "active": name == active_instance,
                "_mtime": mtime,
            }));
        }
    }

    // ── 清理缓存中已删除的实例 ──
    let existing_names: Vec<&str> = instances.iter()
        .filter_map(|i| i["name"].as_str())
        .collect();
    let stale: Vec<String> = index.keys()
        .filter(|k| !existing_names.contains(&k.as_str()))
        .cloned()
        .collect();
    if !stale.is_empty() {
        index_changed = true;
        for k in &stale { index.remove(k); }
    }

    // ── 写入更新后的缓存 ──
    if index_changed {
        if let Some(p) = &index_path {
            if let Ok(json) = serde_json::to_string(&index) {
                let _ = std::fs::write(p, &json);
            }
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
