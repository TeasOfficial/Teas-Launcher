//! 安装辅助函数 — 共享工具
#![allow(dead_code)]

use std::path::Path;

/// 创建 inheritsFrom JSON (轻量级，指向原版 MC 版本)
pub fn create_inherits_json(
    mc_dir: &Path,
    instance_name: &str,
    mc_version: &str,
) -> Result<(), String> {
    let target = mc_dir.join("versions").join(instance_name);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    let json = serde_json::json!({
        "id": instance_name,
        "inheritsFrom": mc_version,
        "mainClass": "net.minecraft.client.main.Main",
        "type": "release"
    });

    std::fs::write(
        target.join(format!("{}.json", instance_name)),
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// 清空并删除目录
pub fn clean_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 移动目录内容
#[allow(dead_code)]
pub fn move_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    if !src.exists() { return Ok(()); }
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;

    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let dest = dst.join(&name);

        if ty.is_dir() {
            move_dir_contents(&entry.path(), &dest)?;
        } else {
            if dest.exists() {
                let _ = std::fs::remove_file(&dest);
            }
            std::fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
