//! JSON 合并 — 参照 PCL-CE MergeJson()
//!
//! 多 loader 同时安装时合并 version.json:
//! - 合并所有 libraries (去重)
//! - 合并 arguments (game + jvm)
//! - 根据 loader 类型决定是否保留 inheritsFrom
//! #![allow(dead_code)] — 未来前端将调用

use std::collections::HashSet;
use std::path::Path;

/// 合并多个 loader 的 version JSON 到一个
///
/// 参照 PCL-CE MergeJson:
#[allow(dead_code)]
pub fn merge_instance_json(
    mc_dir: &Path,
    instance_name: &str,
    vanilla_mc_version: &str,
    loader_type: &str,
    loader_json: &serde_json::Value,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    let json_path = target.join(format!("{}.json", instance_name));

    // 读取现有的 JSON (如果已由原版安装器创建)
    let mut merged = if json_path.exists() {
        let content = std::fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&content).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({
            "id": instance_name,
            "inheritsFrom": vanilla_mc_version,
        })
    };

    // 特殊处理：Forge/NeoForge/Cleanroom 的 JSON 已经是完整 JSON
    // 它们包含自己的 mainClass 和完整的 libraries
    match loader_type {
        "forge" | "neoforge" | "cleanroom" => {
            // 这些 loader 的 JSON 已经是完整版本
            // 我们只需保留它们的核心字段
            if let Some(mc) = loader_json.get("mainClass").cloned() {
                merged["mainClass"] = mc;
            }
            if let Some(args) = loader_json.get("arguments").cloned() {
                merged["arguments"] = args;
            }
            // 合并 libraries
            merge_libraries(&mut merged, loader_json);
            // Forge/NeoForge 可能不需要 inheritsFrom (1.17+ 版本的 Forge
            // 在 profile JSON 中已包含完整的 libraries 列表)
            if let Some(inh) = loader_json.get("inheritsFrom").and_then(|v| v.as_str()) {
                merged["inheritsFrom"] = serde_json::Value::String(inh.to_string());
            }
        }
        "fabric" | "quilt" | "legacyfabric" => {
            // 这些 loader 的 profile JSON 包含完整的 libraries (无 inheritsFrom)
            // 直接使用 loader JSON 作为基础
            merged = loader_json.clone();
            merged["id"] = serde_json::Value::String(instance_name.to_string());
        }
        "optifine" | "liteloader" | "labymod" => {
            // 这些 loader 创建的是轻量 inheritsFrom JSON
            // 追加 libraries
            merge_libraries(&mut merged, loader_json);
        }
        _ => {
            merge_libraries(&mut merged, loader_json);
        }
    }

    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&merged).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// 合并 libraries 列表 (去重)
#[allow(dead_code)]
fn merge_libraries(target: &mut serde_json::Value, source: &serde_json::Value) {
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

    // 去重 key: name 字段
    let mut seen: HashSet<String> = cur_libs
        .iter()
        .filter_map(|l| l.get("name").and_then(|n| n.as_str().map(|s| s.to_string())))
        .collect();

    for lib in source_libs {
        let key = lib
            .get("name")
            .and_then(|n| n.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if !key.is_empty() && !seen.contains(&key) {
            seen.insert(key);
            cur_libs.push(lib);
        }
    }

    target["libraries"] = serde_json::Value::Array(cur_libs);
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
