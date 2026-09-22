use std::path::Path;

// ── 工具函数 ────────────────────────────────────────────────

/// 检测 Java 主版本号（从 java -version 的 stderr 输出解析）
pub(super) fn detect_java_version(java_path: &Path) -> Result<u32, String> {
    let output = std::process::Command::new(java_path)
        .arg("-version")
        .output()
        .map_err(|_| format!("无法执行 Java: {}", java_path.display()))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}\n{}", stderr, stdout);

    for line in combined.lines() {
        if let Some(ver_str) = line.split("version").nth(1) {
            let ver_str = ver_str.trim().trim_matches('"').trim_matches('\'');
            if let Some(dot_pos) = ver_str.find('.') {
                if ver_str.starts_with("1.") {
                    // Java 1.x → 取第二个数字（1.8 → 8）
                    if let Some(rest) = ver_str.strip_prefix("1.") {
                        if let Some(next_dot) = rest.find('.') {
                            return rest[..next_dot].parse().map_err(|_| "无法解析 Java 版本".to_string());
                        }
                        return rest.parse().map_err(|_| "无法解析 Java 版本".to_string());
                    }
                } else {
                    // Java 9+ → 取第一个数字
                    return ver_str[..dot_pos].parse().map_err(|_| "无法解析 Java 版本".to_string());
                }
            }
        }
    }

    // 回退: XshowSettings
    let output2 = std::process::Command::new(java_path)
        .args(["-XshowSettings:all", "-version"])
        .output();
    if let Ok(out) = output2 {
        let stderr2 = String::from_utf8_lossy(&out.stderr);
        for line in stderr2.lines() {
            if line.contains("java.runtime.version") || line.contains("java.version") {
                for part in line.split_whitespace() {
                    if part.contains('.') && part.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                        if let Some(dot_pos) = part.find('.') {
                            if part.starts_with("1.") {
                                let rest = &part[2..];
                                if let Some(next_dot) = rest.find('.') {
                                    return rest[..next_dot].parse().map_err(|_| "无法解析 Java 版本".to_string());
                                }
                            } else {
                                return part[..dot_pos].parse().map_err(|_| "无法解析 Java 版本".to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Err(format!("无法检测 Java 版本: {}", java_path.display()))
}

/// 检测 Java 是否为 64-bit
pub(super) fn is_64bit_java(java_path: &Path) -> bool {
    if let Ok(output) = std::process::Command::new(java_path)
        .arg("-version")
        .output()
    {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        format!("{}\n{}", stderr, stdout).contains("64-Bit")
    } else {
        cfg!(target_arch = "x86_64")
    }
}

/// 从 version JSON 猜测 Minecraft 游戏版本号
pub(super) fn guess_game_version(version: &serde_json::Value) -> Option<String> {
    if let Some(parent) = version["inheritsFrom"].as_str() {
        return Some(parent.to_string());
    }
    if let Some(id) = version["id"].as_str() {
        if !id.contains("forge") && !id.contains("fabric") && !id.contains("quilt")
            && !id.contains("neoforge") && !id.contains("liteloader")
        {
            return Some(id.to_string());
        }
    }
    if let Some(cv) = version["clientVersion"].as_str() {
        return Some(cv.to_string());
    }
    if let Some(jar) = version["jar"].as_str() {
        return Some(jar.to_string());
    }
    None
}

/// 简单语义版本比较
pub(super) fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}


/// 校验所选 Java 与目标版本是否匹配
///
/// 版本 JSON 的 `javaVersion.majorVersion` 是官方给出的**最低**要求：
/// - 低于它必然起不来 → 直接阻断并给出可操作提示
/// - 明显高于它时 LWJGL 等本地库经常不兼容（实测 Java 25 跑 MC 1.21.1 报
///   `Module org.lwjgl.glfw not found`）→ 只警告，不阻断，因为部分组合仍可用
pub(super) fn check_java_compatibility(
    java_path: &Path,
    version: &serde_json::Value,
) -> Result<(), String> {
    let Some(required) = version["javaVersion"]["majorVersion"].as_u64() else {
        // 版本 JSON 未声明要求（旧版本），无法判断，放行
        return Ok(());
    };
    let required = required as u32;
    let actual = detect_java_version(java_path)?;

    if actual < required {
        return Err(format!(
            "该实例需要 Java {} 或更高，当前选中的是 Java {}。\n\
             请在「设置 → 启动 → Java 路径」中改选 Java {} 或更新版本。\n当前路径: {}",
            required,
            actual,
            required,
            java_path.display()
        ));
    }

    if actual > required + 1 {
        log::warn!(
            "[launch] Java {} 明显高于该实例要求的 Java {}，LWJGL 等本地库可能不兼容；\
             若启动失败请改用 Java {}",
            actual,
            required,
            required
        );
    }

    Ok(())
}
