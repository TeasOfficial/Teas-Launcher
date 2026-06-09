//! 整合包安装 — 参照 PCL-CE ModDownloadLib + ModModpack
//!
//! 支持格式:
//! - Modrinth (.mrpack) — modrinth.index.json
//! - CurseForge (.zip)   — manifest.json

use crate::download::engine::download_file;
use crate::download::model::{DownloadFile, FileChecker};
use crate::download::source::source_mod_download;
use crate::http::{apply_source, build_http_client, is_cancelled, reset_cancel};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tauri::Emitter;

#[tauri::command]
pub async fn install_modpack(
    app: tauri::AppHandle,
    mc_dir: String,
    url: String,
    filename: String,
    instance_name: String,
    source: String,
    dl_threads: u32,
) -> Result<String, String> {
    reset_cancel();
    let base = Path::new(&mc_dir);
    let mc_path = if base.join(".minecraft").exists() { base.join(".minecraft") } else { base.to_path_buf() };
    let target = mc_path.join("versions").join(&instance_name);
    if target.exists() {
        return Err(format!("实例 {} 已存在", instance_name));
    }

    // Phase 1: 下载整合包文件
    let mirror_url = apply_source(&url, &source);
    let urls: Vec<String> = if mirror_url != url {
        vec![mirror_url, url.clone()]
    } else {
        vec![url.clone()]
    };
    let tmp_file = mc_path.join(format!(".tmp_{}.zip", instance_name));
    let mut dl = DownloadFile::new(urls, tmp_file.clone(), FileChecker::with_min_size(1024));
    download_file(&app, &mut dl, false).await?;

    // Phase 2: 解压 + 格式检测
    let tmp_dir = mc_path.join(format!(".tmp_{}", instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let file = std::fs::File::open(&tmp_file).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("无法读取: {}", e))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if name.ends_with('/') { continue; }
        let dest = tmp_dir.join(&name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&tmp_file);

    let is_curseforge = tmp_dir.join("manifest.json").exists();

    // Phase 3: 复制 overrides
    if is_curseforge {
        let cf_path = tmp_dir.join("manifest.json");
        let content = std::fs::read_to_string(&cf_path).map_err(|e| e.to_string())?;
        let manifest: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        let overrides_dir = manifest["overrides"].as_str().unwrap_or("overrides");
        let overrides_path = if overrides_dir == "." || overrides_dir == "./" {
            tmp_dir.clone()
        } else {
            tmp_dir.join(overrides_dir)
        };
        if overrides_path.exists() {
            crate::install::helpers::move_dir_contents(&overrides_path, &target)?;
        }
    } else {
        if tmp_dir.join("overrides").exists() {
            crate::install::helpers::move_dir_contents(&tmp_dir.join("overrides"), &target)?;
        }
    }

    let client = build_http_client(Duration::from_secs(600))?;

    // Phase 4: 按格式下载 Mod
    if is_curseforge {
        // CurseForge 格式
        let cf_path = tmp_dir.join("manifest.json");
        let content = std::fs::read_to_string(&cf_path).map_err(|e| e.to_string())?;
        let manifest: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        download_curseforge_mods(&app, &client, &manifest, &target, &instance_name, dl_threads).await?;
    } else {
        // Modrinth 格式
        let mr_path = tmp_dir.join("modrinth.index.json");
        if !mr_path.exists() {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err("未找到 modrinth.index.json 或 manifest.json".to_string());
        }
        let content = std::fs::read_to_string(&mr_path).map_err(|e| e.to_string())?;
        let index: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if let Some(files) = index["files"].as_array() {
            let total = files.len();
            if total > 0 {
                download_modrinth_mods(&app, &client, files, &target, &instance_name, total, dl_threads).await;
            }
        }
    }

    // Phase 5: 解析依赖 + 安装 loader
    let (loader_type, loader_ver, mc_ver) = if is_curseforge {
        let cf_path = tmp_dir.join("manifest.json");
        let content = std::fs::read_to_string(&cf_path).map_err(|e| e.to_string())?;
        let manifest: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        parse_curseforge_deps(&manifest)
    } else {
        let mr_path = tmp_dir.join("modrinth.index.json");
        let content = std::fs::read_to_string(&mr_path).map_err(|e| e.to_string())?;
        let index: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        crate::install::merge::parse_modpack_deps(&index)
    };

    app.emit("download-progress", serde_json::json!({
        "filename": &instance_name, "downloaded": 75, "total": 100, "percent": 75,
        "step": "正在安装加载器..."
    })).ok();

    if !mc_ver.is_empty() {
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

        match loader_type.as_str() {
            "fabric" | "quilt" => {
                install_loader_with_vanilla_merge(
                    &app, &mc_path, &instance_name, &mc_ver, &loader_ver, &loader_type, &client
                ).await?;
            }
            "forge" => {
                install_forgelike_with_vanilla_merge(
                    &app, &mc_path, &instance_name, &mc_ver, &loader_ver, "forge", &client
                ).await?;
            }
            "neoforge" => {
                install_forgelike_with_vanilla_merge(
                    &app, &mc_path, &instance_name, &mc_ver, &loader_ver, "neoforge", &client
                ).await?;
            }
            _ => {
                install_vanilla_direct(&app, &mc_path, &instance_name, &mc_ver, &client).await?;
            }
        }
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
    app.emit("download-progress", serde_json::json!({
        "filename": &instance_name, "downloaded": 100, "total": 100, "percent": 100,
        "step": "安装完成!"
    })).ok();

    Ok(format!("整合包 {} 安装完成", instance_name))
}

// ═══ Modrinth Mod 下载 ═══
async fn download_modrinth_mods(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    files: &[serde_json::Value],
    target: &Path,
    instance_name: &str,
    total: usize,
    dl_threads: u32,
) {
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(dl_threads.max(1) as usize));
    let counter = std::sync::Arc::new(AtomicU32::new(0));
    let mut handles = Vec::new();

    for f in files.iter() {
        let path_str = f["path"].as_str().unwrap_or("").to_string();
        let dl = f["downloads"].as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        if path_str.is_empty() || dl.is_empty() { continue; }

        let c = client.clone(); let a = app.clone(); let s = sem.clone();
        let cnt = counter.clone(); let tgt = target.to_path_buf();
        let iname = instance_name.to_string();
        let fname = Path::new(&path_str).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path_str.clone());
        let total_u = total as u64;

        handles.push(tokio::spawn(async move {
            let _p = s.acquire().await.unwrap();
            if is_cancelled() { return; }
            let durls = source_mod_download(&dl, 1);
            let dest = tgt.join(&path_str);
            if dest.exists() { return; }
            if let Some(p) = dest.parent() { let _ = std::fs::create_dir_all(p); }
            for u in &durls {
                if is_cancelled() { return; }
                if let Ok(resp) = c.get(u).send().await {
                    if let Ok(bytes) = resp.bytes().await {
                        let _ = std::fs::write(&dest, &bytes);
                        break;
                    }
                }
            }
            let n = cnt.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = a.emit("download-progress", serde_json::json!({
                "filename": iname, "downloaded": n as u64, "total": total_u,
                "percent": 25 + ((n as f64 / total_u as f64) * 50.0) as u32,
                "step": format!("下载模组 ({}/{})：{}", n, total_u, fname)
            }));
        }));
    }
    for h in handles { let _ = h.await; }
}

// ═══ CurseForge Mod 下载 (PCL ModModpack.cs 第 444-530 行) ═══
async fn download_curseforge_mods(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    manifest: &serde_json::Value,
    target: &Path,
    instance_name: &str,
    dl_threads: u32,
) -> Result<(), String> {
    let files = manifest["files"].as_array().ok_or("manifest.json 缺少 files")?;
    let total = files.len();
    if total == 0 { return Ok(()); }

    // 收集 fileID 列表
    let file_ids: Vec<i64> = files.iter()
        .filter_map(|f| f["fileID"].as_i64())
        .collect();
    if file_ids.is_empty() { return Ok(()); }

    eprintln!("[modpack] CurseForge: 请求 {} 个 Mod 的下载信息", file_ids.len());

    // PCL: POST https://api.curseforge.com/v1/mods/files
    // 先尝试镜像，再直连
    let request_body = serde_json::json!({"fileIds": &file_ids});
    let mut response_json: Option<serde_json::Value> = None;

    for (url, timeout) in &[
        ("https://mod.mcimirror.top/curseforge/v1/mods/files", 15u64),
        ("https://api.curseforge.com/v1/mods/files", 10u64),
    ] {
        match client.post(*url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&request_body)
            .timeout(Duration::from_secs(*timeout))
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let data_count = json["data"].as_array().map(|a| a.len()).unwrap_or(0);
                    if data_count >= 1 {
                        eprintln!("[modpack] CurseForge API 响应: {} 个 Mod, 来自 {}", data_count, url);
                        response_json = Some(json);
                        break;
                    }
                }
            }
            Err(e) => eprintln!("[modpack] CurseForge API {} 失败: {}", url, e),
        }
    }

    let Some(response) = response_json else {
        return Err("CurseForge API 请求失败".to_string());
    };

    // 构建 fileID → downloadUrl 映射
    let data = response["data"].as_array().ok_or("CurseForge API 响应无 data")?;
    use std::collections::HashMap;
    let mut url_map: HashMap<i64, String> = HashMap::new();
    let mut name_map: HashMap<i64, String> = HashMap::new();
    for mod_info in data {
        let fid = mod_info["id"].as_i64().unwrap_or(0);
        let dl_url = mod_info["downloadUrl"].as_str().unwrap_or("").to_string();
        let name = mod_info["fileName"].as_str().unwrap_or("unknown.jar").to_string();
        if !dl_url.is_empty() {
            url_map.insert(fid, dl_url);
            name_map.insert(fid, name);
        }
    }

    eprintln!("[modpack] CurseForge: 解析到 {} 个下载链接", url_map.len());

    // 多线程下载
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(dl_threads.max(1) as usize));
    let counter = std::sync::Arc::new(AtomicU32::new(0));
    let mut handles = Vec::new();

    for f in files.iter() {
        let fid = f["fileID"].as_i64().unwrap_or(0);
        let Some(dl_url) = url_map.get(&fid).cloned() else { continue; };
        let fname = name_map.get(&fid).cloned().unwrap_or_else(|| format!("{}.jar", fid));
        let required = f["required"].as_bool().unwrap_or(true);

        let c = client.clone(); let a = app.clone(); let s = sem.clone();
        let cnt = counter.clone(); let tgt = target.to_path_buf();
        let iname = instance_name.to_string();
        let total_u = total as u64;

        handles.push(tokio::spawn(async move {
            let _p = s.acquire().await.unwrap();
            if is_cancelled() { return; }
            let dest = tgt.join("mods").join(&fname);
            if dest.exists() { return; }
            if let Some(p) = dest.parent() { let _ = std::fs::create_dir_all(p); }

            // PCL: 镜像源回退
            let urls = if dl_url.contains("edge.forgecdn.net") || dl_url.contains("mediafilez.forgecdn.net") {
                let mirror = dl_url.replace("edge.forgecdn.net", "mod.mcimirror.top")
                    .replace("mediafilez.forgecdn.net", "mod.mcimirror.top");
                vec![mirror, dl_url]
            } else {
                vec![dl_url]
            };

            for u in &urls {
                if is_cancelled() { return; }
                if let Ok(resp) = c.get(u).send().await {
                    if let Ok(bytes) = resp.bytes().await {
                        let _ = std::fs::write(&dest, &bytes);
                        break;
                    }
                }
            }
            let n = cnt.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = a.emit("download-progress", serde_json::json!({
                "filename": iname, "downloaded": n as u64, "total": total_u,
                "percent": 25 + ((n as f64 / total_u as f64) * 50.0) as u32,
                "step": format!("下载模组 ({}/{})：{}", n, total_u, fname)
            }));
        }));
    }
    for h in handles { let _ = h.await; }
    Ok(())
}

// ═══ CurseForge 依赖解析 (PCL ModModpack.cs 第 374-424 行) ═══
fn parse_curseforge_deps(manifest: &serde_json::Value) -> (String, String, String) {
    let mc_ver = manifest["minecraft"]["version"].as_str().unwrap_or("").to_string();
    let mut forge_ver = String::new();
    let mut neoforge_ver = String::new();
    let mut fabric_ver = String::new();
    let mut quilt_ver = String::new();

    if let Some(loaders) = manifest["minecraft"]["modLoaders"].as_array() {
        for entry in loaders {
            let id = entry["id"].as_str().unwrap_or("").to_lowercase();
            if id.starts_with("forge-") {
                forge_ver = id.trim_start_matches("forge-").to_string();
            } else if id.starts_with("neoforge-") {
                neoforge_ver = id.trim_start_matches("neoforge-").to_string();
            } else if id.starts_with("fabric-") {
                fabric_ver = id.trim_start_matches("fabric-").to_string();
            } else if id.starts_with("quilt-") {
                quilt_ver = id.trim_start_matches("quilt-").to_string();
            }
        }
    }

    if !forge_ver.is_empty() { ("forge".to_string(), forge_ver, mc_ver) }
    else if !neoforge_ver.is_empty() { ("neoforge".to_string(), neoforge_ver, mc_ver) }
    else if !fabric_ver.is_empty() { ("fabric".to_string(), fabric_ver, mc_ver) }
    else if !quilt_ver.is_empty() { ("quilt".to_string(), quilt_ver, mc_ver) }
    else { ("vanilla".to_string(), String::new(), mc_ver) }
}

// ═══ PCL-CE MergeJson: vanilla + loader → 自包含 JSON ═══
async fn install_loader_with_vanilla_merge(
    app: &tauri::AppHandle, mc_dir: &Path, instance_name: &str, mc_ver: &str,
    loader_ver: &str, loader_type: &str, client: &reqwest::Client,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let vanilla_json = download_json_to_memory(client, mc_ver).await?;
    let profile_url = match loader_type {
        "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json", mc_ver, loader_ver),
        "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json", mc_ver, loader_ver),
        _ => return Err(format!("不支持的 loader: {}", loader_type)),
    };
    let resp = client.get(&profile_url).send().await.map_err(|e| e.to_string())?;
    let profile_str = resp.text().await.map_err(|e| e.to_string())?;
    let mut profile: serde_json::Value = serde_json::from_str(&profile_str).map_err(|e| e.to_string())?;

    // PCL MergeJson: 原版为基础，loader 合入
    let mut output = vanilla_json.clone();
    output.as_object_mut().map(|o| { o.remove("releaseTime"); o.remove("time"); });
    profile.as_object_mut().map(|o| { o.remove("releaseTime"); o.remove("time"); });

    // 合并 loader libraries 到原版
    if let Some(p_libs) = profile["libraries"].as_array().cloned() {
        let mut vanilla_libs = output["libraries"].as_array().cloned().unwrap_or_default();
        let mut seen: std::collections::HashSet<String> = vanilla_libs.iter()
            .filter_map(|l| l["name"].as_str().map(|s| s.to_string())).collect();
        for lib in p_libs {
            if let Some(name) = lib["name"].as_str() {
                if !seen.contains(name) { seen.insert(name.to_string()); vanilla_libs.push(lib); }
            }
        }
        output["libraries"] = serde_json::Value::Array(vanilla_libs);
    }

    // Loader 的 mainClass/arguments 覆盖
    if let Some(mc) = profile.get("mainClass").cloned() { output["mainClass"] = mc; }
    if let Some(args) = profile.get("arguments").cloned() { output["arguments"] = args; }
    output.as_object_mut().map(|o| o.remove("inheritsFrom"));
    output.as_object_mut().map(|o| o.remove("_comment_"));
    output["id"] = serde_json::Value::String(instance_name.to_string());

    let json_path = target.join(format!("{}.json", instance_name));
    std::fs::write(&json_path, serde_json::to_string_pretty(&output).unwrap_or_default())
        .map_err(|e| e.to_string())?;

    // 下载 libraries
    let libs = crate::version::library::mclib_list_from_json(&output, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if let Some(url) = vanilla_json["downloads"]["client"]["url"].as_str() {
        let jar_path = target.join(format!("{}.jar", instance_name));
        let urls = crate::download::source::source_launcher_or_meta(url, false);
        let checker = FileChecker::with_min_size(1024);
        if checker.check(&jar_path).is_some() {
            dl_files.push(DownloadFile::new(urls, jar_path, checker));
        }
    }
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }
    Ok(())
}

// ═══ Forge/NeoForge 整合包安装 (PCL-CE McDownloadForgelikeLoader + MergeJson) ═══
async fn install_forgelike_with_vanilla_merge(
    app: &tauri::AppHandle, mc_dir: &Path, instance_name: &str, mc_ver: &str,
    loader_ver: &str, loader_type: &str, client: &reqwest::Client,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let tmp_dir = mc_dir.join(format!(".tmp_{}_{}", loader_type, instance_name));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

    let mc_clean = mc_ver.replace('-', "_");
    let installer_path = tmp_dir.join("installer.jar");
    let installer_urls = match loader_type {
        "forge" => vec![
            format!("https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar", mc_clean, loader_ver, mc_clean, loader_ver),
            format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar", mc_clean, loader_ver, mc_clean, loader_ver),
        ],
        "neoforge" => {
            let pkg = if mc_ver == "1.20.1" { "forge" } else { "neoforge" };
            vec![
                format!("https://bmclapi2.bangbang93.com/maven/net/neoforged/{}/{}/{}-{}-installer.jar", pkg, loader_ver, pkg, loader_ver),
                format!("https://maven.neoforged.net/releases/net/neoforged/{}/{}/{}-{}-installer.jar", pkg, loader_ver, pkg, loader_ver),
            ]
        }
        _ => return Err(format!("不支持的: {}", loader_type)),
    };

    let mut dl = DownloadFile::new(installer_urls, installer_path.clone(), FileChecker::with_min_size(64*1024));
    download_file(app, &mut dl, false).await?;

    let archive_file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(archive_file).map_err(|e| e.to_string())?;

    // 读取 version.json (PCL-CE: 仅使用 version.json，profile 的 libs 是构建工具不混入)
    let read_json = |archive: &mut zip::ZipArchive<std::fs::File>, name: &str| -> Option<serde_json::Value> {
        let mut entry = archive.by_name(name).ok()?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut entry, &mut s).ok()?;
        serde_json::from_str(&s).ok()
    };
    let version_json = read_json(&mut archive, "version.json");

    let mut forge_runtime = version_json.unwrap_or(serde_json::json!({}));
    eprintln!("[modpack] Forge version.json libs: {}",
        forge_runtime["libraries"].as_array().map(|a| a.len()).unwrap_or(0));

    // PCL MergeJson: 原版为基础，Forge 合入
    let vanilla_json = download_json_to_memory(client, mc_ver).await?;
    let mut output = vanilla_json.clone();
    output.as_object_mut().map(|o| { o.remove("releaseTime"); o.remove("time"); });
    forge_runtime.as_object_mut().map(|o| { o.remove("releaseTime"); o.remove("time"); });

    // 合并 libraries
    if let Some(v_libs) = forge_runtime["libraries"].as_array().cloned() {
        let mut all = output["libraries"].as_array().cloned().unwrap_or_default();
        let mut seen: std::collections::HashSet<String> = all.iter()
            .filter_map(|l| l["name"].as_str().map(|s| s.to_string())).collect();
        for lib in v_libs {
            if let Some(name) = lib["name"].as_str() {
                if !seen.contains(name) { seen.insert(name.to_string()); all.push(lib); }
            }
        }
        output["libraries"] = serde_json::Value::Array(all);
    }

    // Forge 覆盖 mainClass/arguments
    if let Some(mc) = forge_runtime.get("mainClass").cloned() { output["mainClass"] = mc; }
    if let Some(args) = forge_runtime.get("arguments").cloned() { output["arguments"] = args; }
    output.as_object_mut().map(|o| o.remove("inheritsFrom"));
    output.as_object_mut().map(|o| o.remove("_comment_"));
    output["id"] = serde_json::Value::String(instance_name.to_string());

    let json_path = target.join(format!("{}.json", instance_name));
    std::fs::write(&json_path, serde_json::to_string_pretty(&output).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    eprintln!("[modpack] Forge 自包含 JSON: {} libraries",
        output["libraries"].as_array().map(|a| a.len()).unwrap_or(0));

    // 下载 libraries
    let libs = crate::version::library::mclib_list_from_json(&output, mc_dir);
    let mut dl_files = crate::version::library::mclib_to_download_files(&libs, false);
    if let Some(url) = vanilla_json["downloads"]["client"]["url"].as_str() {
        let jar_path = target.join(format!("{}.jar", instance_name));
        let urls = crate::download::source::source_launcher_or_meta(url, false);
        if FileChecker::with_min_size(1024).check(&jar_path).is_some() {
            dl_files.push(DownloadFile::new(urls, jar_path, FileChecker::with_min_size(1024)));
        }
    }
    if !dl_files.is_empty() {
        crate::download::engine::download_files_parallel(app, &mut dl_files, 8).await;
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}

async fn install_vanilla_direct(
    app: &tauri::AppHandle, mc_dir: &Path, instance_name: &str, mc_ver: &str, client: &reqwest::Client,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let inherits = serde_json::json!({"id":instance_name,"inheritsFrom":mc_ver,"mainClass":"net.minecraft.client.main.Main","type":"release"});
    std::fs::write(target.join(format!("{}.json", instance_name)), serde_json::to_string_pretty(&inherits).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_json_to_memory(client: &reqwest::Client, mc_ver: &str) -> Result<serde_json::Value, String> {
    let manifest: serde_json::Value = client.get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    let ver_url = manifest["versions"].as_array().ok_or("版本清单为空")?
        .iter().find(|v| v["id"].as_str() == Some(mc_ver))
        .and_then(|v| v["url"].as_str()).ok_or(format!("找不到版本 {}", mc_ver))?;
    let urls = crate::download::source::source_launcher_or_meta(ver_url, false);
    for url in &urls {
        if let Ok(resp) = client.get(url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json.get("libraries").is_some() { return Ok(json); }
            }
        }
    }
    Err(format!("无法下载原版 JSON: {}", mc_ver))
}
