//! 镜像下载源系统 — 参照 PCL-CE ModDownload.DlSource* 系列
//!
//! 四种 URL 转换:
//! 1. source_launcher_or_meta  — 启动器/元数据 JSON (版本清单, 版本 JSON)
//! 2. source_library           — 库文件 (libraries) 含 BMCLAPI maven
//! 3. source_assets            — 资源文件 (assets/objects)
//! 4. source_mod               — Mod API/下载 (Modrinth/CurseForge)
//!
//! 源优先级策略 (参照 PCL-CE DlSourcePreferMojang):
//! - file_source == 2: 始终优先官方源
//! - file_source == 1 && 官方源快: 优先官方源
//! - 否则: 优先镜像源

/// 下载源顺序：官方优先 or 镜像优先
/// 参照 PCL-CE: DlSourceOrder(officialUrls, mirrorUrls)
fn source_order(official: Vec<String>, mirror: Vec<String>, prefer_official: bool) -> Vec<String> {
    if prefer_official {
        // 官方优先: 官方 + 镜像 去重
        let mut result = official;
        for m in mirror {
            if !result.contains(&m) {
                result.push(m);
            }
        }
        result
    } else {
        // 镜像优先: 镜像 + 官方 去重
        let mut result = mirror;
        for o in official {
            if !result.contains(&o) {
                result.push(o);
            }
        }
        result
    }
}

/// 是否优先使用官方源 (参照 PCL-CE DlSourcePreferMojang)
#[allow(dead_code)]
pub fn prefer_official(file_source: u32, mojang_is_fast: bool) -> bool {
    file_source == 2 || (file_source == 1 && mojang_is_fast)
}

/// 是否优先使用官方源获取版本列表 (参照 PCL-CE DlVersionListPreferMojang)
#[allow(dead_code)]
pub fn prefer_official_for_list(version_list_source: u32, mojang_is_fast: bool) -> bool {
    version_list_source == 2 || (version_list_source == 1 && mojang_is_fast)
}

/// 启动器/Launcher/Meta URL 镜像转换
///
/// 参照 PCL-CE DlSourceLauncherOrMetaGet:
/// ```vb
/// Return DlSourceOrder({original}, {
///     original.Replace("piston-data.mojang.com", "bmclapi2.bangbang93.com")
///            .Replace("piston-meta.mojang.com", "bmclapi2.bangbang93.com")
///            .Replace("launcher.mojang.com", "bmclapi2.bangbang93.com")
///            .Replace("launchermeta.mojang.com", "bmclapi2.bangbang93.com"),
///     original
/// })
/// ```
pub fn source_launcher_or_meta(original: &str, prefer_official: bool) -> Vec<String> {
    let mirror = original
        .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com")
        .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com")
        .replace("https://launcher.mojang.com", "https://bmclapi2.bangbang93.com")
        .replace("https://launchermeta.mojang.com", "https://bmclapi2.bangbang93.com")
        .replace(
            "https://zkitefly.github.io/unlisted-versions-of-minecraft",
            "https://alist.8mi.tech/d/mirror/unlisted-versions-of-minecraft/Auto",
        );
    let official = vec![original.to_string()];
    let mirrors = vec![mirror, original.to_string()]; // PCL-CE 在 mirror 列表最后也加了 original
    source_order(official, mirrors, prefer_official)
}

/// Library 文件 URL 镜像转换
///
/// 参照 PCL-CE DlSourceLibraryGet:
/// - 如果 URL 含 minecraftforge / fabricmc / neoforged → 只用 BMCLAPI maven，不加原版源
/// - 否则: official + BMCLAPI maven + BMCLAPI libraries + original
pub fn source_library(original: &str, prefer_official: bool) -> Vec<String> {
    // 特判: forge/fabric/neoforge 的 maven 不使用原版 libraries.minecraft.net
    let is_special_maven = ["minecraftforge", "fabricmc", "neoforged"]
        .iter()
        .any(|k| original.contains(k));

    if is_special_maven {
        return vec![
            original
                .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com/maven")
                .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com/maven")
                .replace("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/maven"),
            original
                .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com/libraries")
                .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com/libraries")
                .replace("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/libraries"),
        ];
    }

    let official = vec![original.to_string()];
    let mirrors = vec![
        original
            .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com/maven")
            .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com/maven")
            .replace("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/maven"),
        original
            .replace("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com/libraries")
            .replace("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com/libraries")
            .replace("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/libraries"),
        original.to_string(),
    ];
    source_order(official, mirrors, prefer_official)
}

/// Asset 文件 URL 镜像转换
///
/// 参照 PCL-CE DlSourceAssetsGet:
/// ```vb
/// Return DlSourceOrder({original}, {
///     original.Replace("piston-data.mojang.com", "bmclapi2.bangbang93.com/assets")
///            .Replace("piston-meta.mojang.com", "bmclapi2.bangbang93.com/assets")
///            .Replace("resources.download.minecraft.net", "bmclapi2.bangbang93.com/assets")
/// })
/// ```
pub fn source_assets(original: &str, prefer_official: bool) -> Vec<String> {
    let mirror = original
        .replace(
            "https://piston-data.mojang.com",
            "https://bmclapi2.bangbang93.com/assets",
        )
        .replace(
            "https://piston-meta.mojang.com",
            "https://bmclapi2.bangbang93.com/assets",
        )
        .replace(
            "https://resources.download.minecraft.net",
            "https://bmclapi2.bangbang93.com/assets",
        );
    let official = vec![original.to_string()];
    let mirrors = vec![mirror];
    source_order(official, mirrors, prefer_official)
}

/// Mod API/Download 镜像源
///
/// 参照 PCL-CE DlSourceModGet (API) + DlSourceModDownloadGet (下载):
#[allow(dead_code)]
pub fn source_mod_api(original: &str) -> String {
    original
        .replace("https://api.modrinth.com", "https://mod.mcimirror.top/modrinth")
        .replace("https://api.curseforge.com", "https://mod.mcimirror.top/curseforge")
}

/// Mod 文件下载源列表 (含回退)
/// 参照 PCL-CE: comp_source_solution 决定优先级
pub fn source_mod_download(original: &str, comp_source: u32) -> Vec<String> {
    let mirror = original
        .replace("https://cdn.modrinth.com", "https://mod.mcimirror.top")
        .replace("https://edge.forgecdn.net", "https://mod.mcimirror.top");

    match comp_source {
        0 => vec![mirror.clone(), mirror, original.to_string()],        // 镜像优先
        1 => vec![original.to_string(), mirror.clone(), original.to_string(), mirror], // 平衡
        2 => vec![original.to_string(), original.to_string(), mirror],  // 官方优先
        _ => vec![original.to_string(), mirror],                        // 默认
    }
}

/// BMCLAPI maven URL 构建
/// 参照 PCL-CE: 根据 token.Url 中提取的 maven 路径构建 BMCLAPI 对应 URL
pub fn source_maven_bmclapi(original: &str) -> String {
    // 找到 "maven" 位置，替换前面的部分
    if let Some(pos) = original.find("/maven/") {
        format!("https://bmclapi2.bangbang93.com/{}", &original[pos + 1..])
    } else if let Some(pos) = original.find("maven.") {
        // maven.minecraftforge.net, maven.fabricmc.net, maven.neoforged.net
        let after_maven = &original[pos + 6..]; // skip "maven."
        if let Some(slash) = after_maven.find('/') {
            format!("https://bmclapi2.bangbang93.com/maven/{}", &after_maven[slash + 1..])
        } else {
            original.replace(
                &original[..original.find("maven.").unwrap()],
                "https://bmclapi2.bangbang93.com/",
            )
            .replace("maven.fabricmc.net", "maven")
            .replace("maven.minecraftforge.net", "maven")
            .replace("maven.neoforged.net/releases", "maven")
        }
    } else {
        original.to_string()
    }
}

/// 源优先级: 官方源优先 → 镜像源接在官方源后面
/// 镜像源优先 → 官方源接在镜像源后面
/// 参照 PCL-CE DlSourceOrder()
#[allow(dead_code)]
pub fn interleave_sources(official: &[String], mirrors: &[String], prefer_official: bool) -> Vec<String> {
    let mut result = Vec::new();
    if prefer_official {
        result.extend(official.iter().cloned());
        for m in mirrors {
            if !result.contains(m) {
                result.push(m.clone());
            }
        }
    } else {
        result.extend(mirrors.iter().cloned());
        for o in official {
            if !result.contains(o) {
                result.push(o.clone());
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_launcher_or_meta() {
        let url = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
        let urls = source_launcher_or_meta(url, false);
        assert_eq!(urls[0], "https://bmclapi2.bangbang93.com/mc/game/version_manifest.json");
        assert_eq!(urls[1], url);
    }

    #[test]
    fn test_source_assets() {
        let url = "https://resources.download.minecraft.net/ab/abcdef1234567890";
        let urls = source_assets(url, false);
        assert_eq!(urls[0], "https://bmclapi2.bangbang93.com/assets/ab/abcdef1234567890");
    }

    #[test]
    fn test_source_mod_api() {
        let url = "https://api.modrinth.com/v2/projects/test";
        let result = source_mod_api(url);
        assert_eq!(result, "https://mod.mcimirror.top/modrinth/v2/projects/test");
    }
}
