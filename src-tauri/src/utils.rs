//! 工具函数：JAR 校验、文件操作、UUID 生成、Natives 提取等

/// 检查 JAR/ZIP 文件是否有有效的 EOCD (End of Central Directory) 签名
/// HMCL 风格: 模拟 Java ZipFileSystem 的严格校验
pub(crate) fn is_jar_valid(path: &std::path::Path) -> bool {
    let len = path.metadata().map(|m| m.len()).unwrap_or(0);
    if len < 22 { return false; }
    if len == 0 { return false; }

    if let Ok(mut f) = std::fs::File::open(path) {
        use std::io::{Read, Seek, SeekFrom};
        let search_start = if len > 65557 { len - 65557 } else { 0 };
        if f.seek(SeekFrom::Start(search_start)).is_ok() {
            let mut buf = vec![0u8; (len - search_start) as usize];
            if f.read_exact(&mut buf).is_ok() {
                let sig: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
                if buf.windows(4).any(|w| w == sig) {
                    return true;
                }
            }
        }
    }
    false
}

/// JAR 扫描策略
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum JarScanStrategy {
    /// 方案 A: 快速预检 — 先用文件大小做初筛，再 EOCD 校验
    QuickCheck,
    /// 方案 B: PCL-CE 风格 — 仅校验实例 JAR，跳过 libraries 目录全量扫描 (默认)
    PclStyle,
    /// 方案 C: 并行扫描 — 多线程递归扫描，速度最快但消耗更多资源
    Parallel,
}

impl JarScanStrategy {
    pub fn from_config(value: &str) -> Self {
        match value {
            "A" | "quick" => JarScanStrategy::QuickCheck,
            "C" | "parallel" => JarScanStrategy::Parallel,
            _ => JarScanStrategy::PclStyle, // 默认 B
        }
    }
}

/// 方案 A: 快速预检 — 用文件大小做初筛，只有可疑文件才做 EOCD 校验
fn scan_with_quickcheck(dir: &std::path::Path, deleted: &mut u32) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_with_quickcheck(&path, deleted);
            } else if path.extension().map_or(false, |e| e == "jar" || e == "zip") {
                // 快速预检：获取文件大小
                let len = path.metadata().map(|m| m.len()).unwrap_or(0);
                // 空文件或明显太小的 → 直接删
                if len == 0 || len < 22 {
                    let _ = std::fs::remove_file(&path);
                    *deleted += 1;
                    log::warn!("[scan:A] 删除空/过小JAR ({}B): {}", len, path.display());
                    continue;
                }
                // 正常大小范围 (>100KB) 且未发现异常 → 跳过 EOCD 校验
                // 只有小文件 (<100KB) 或大小异常的文件才做完整校验
                if len < 102400 {
                    if !is_jar_valid(&path) {
                        let _ = std::fs::remove_file(&path);
                        *deleted += 1;
                        log::warn!("[scan:A] 删除损坏JAR ({}B): {}", len, path.display());
                    }
                }
            }
        }
    }
}

/// 方案 B: PCL-CE 风格 — 仅校验实例自身的 JAR，不扫描 libraries
/// PCL-CE 不进行启动前全量扫描，依赖下载阶段的 SHA1 校验
fn scan_pcl_style(instance_jar: &std::path::Path, deleted: &mut u32) {
    if instance_jar.exists() && !is_jar_valid(instance_jar) {
        let len = instance_jar.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = std::fs::remove_file(instance_jar);
        *deleted += 1;
        log::warn!("[scan:B] 删除损坏实例JAR ({}B): {}", len, instance_jar.display());
    }
}

/// 方案 C: 并行扫描 — 多线程递归扫描 libraries 目录
fn scan_parallel(dir: std::path::PathBuf, deleted: std::sync::Arc<std::sync::atomic::AtomicU32>) {
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut handles = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let d = deleted.clone();
                handles.push(std::thread::spawn(move || {
                    scan_parallel(path, d);
                }));
            } else if path.extension().map_or(false, |e| e == "jar" || e == "zip") {
                if !is_jar_valid(&path) {
                    let len = path.metadata().map(|m| m.len()).unwrap_or(0);
                    let _ = std::fs::remove_file(&path);
                    deleted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    log::warn!("[scan:C] 删除损坏JAR ({}B): {}", len, path.display());
                }
            }
        }
        for h in handles {
            let _ = h.join();
        }
    }
}

/// 根据策略扫描并修复损坏的 JAR 文件
/// - strategy: 扫描策略 (A=快速预检, B=PCL风格(默认), C=并行)
/// - instance_jar: 实例版本 JAR 路径 (方案 B 使用)
/// - libs_dir: libraries 目录路径 (方案 A/C 使用)
pub(crate) fn scan_and_fix_jars_with_strategy(
    strategy: JarScanStrategy,
    instance_jar: &std::path::Path,
    libs_dir: &std::path::Path,
) -> u32 {
    match strategy {
        JarScanStrategy::PclStyle => {
            log::debug!("[scan] 策略B: PCL-CE风格 — 仅校验实例JAR");
            let mut deleted = 0u32;
            if libs_dir.exists() {
                // 仅清理空文件（快速），不做 EOCD 校验
                clean_empty_jars(libs_dir, &mut deleted);
            }
            scan_pcl_style(instance_jar, &mut deleted);
            deleted
        }
        JarScanStrategy::QuickCheck => {
            log::debug!("[scan] 策略A: 快速预检 — 大小初筛 + 小文件EOCD校验");
            let mut deleted = 0u32;
            if libs_dir.exists() {
                scan_with_quickcheck(libs_dir, &mut deleted);
            }
            if instance_jar.exists() && !is_jar_valid(instance_jar) {
                let _ = std::fs::remove_file(instance_jar);
                deleted += 1;
            }
            deleted
        }
        JarScanStrategy::Parallel => {
            log::debug!("[scan] 策略C: 并行扫描 — 多线程递归校验");
            let deleted = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            if libs_dir.exists() {
                scan_parallel(libs_dir.to_path_buf(), deleted.clone());
            }
            if instance_jar.exists() && !is_jar_valid(instance_jar) {
                let _ = std::fs::remove_file(instance_jar);
                deleted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            deleted.load(std::sync::atomic::Ordering::Relaxed)
        }
    }
}

/// 快速清理空文件（不解压校验）
fn clean_empty_jars(dir: &std::path::Path, deleted: &mut u32) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                clean_empty_jars(&path, deleted);
            } else if path.extension().map_or(false, |e| e == "jar" || e == "zip") {
                if let Ok(meta) = path.metadata() {
                    if meta.len() == 0 {
                        let _ = std::fs::remove_file(&path);
                        *deleted += 1;
                        log::warn!("[scan] 删除空JAR: {}", path.display());
                    }
                }
            }
        }
    }
}

/// 检测实例的加载器类型
pub(crate) fn detect_loader(path: &std::path::Path) -> &str {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let jp = path.join(format!("{}.json", name));
        if let Ok(c) = std::fs::read_to_string(&jp) {
            if c.contains("\"mainClass\": \"net.fabricmc.loader.impl.launch.knot.KnotClient\"") {
                return "Fabric";
            }
            if c.contains("\"mainClass\": \"cpw.mods.bootstraplauncher.BootstrapLauncher\"") {
                if c.contains("net.neoforged") { return "NeoForge"; }
                return "Forge";
            }
            if c.contains("\"mainClass\": \"org.quiltmc.loader.impl.launch.knot.KnotClient\"") {
                return "Quilt";
            }
        }
    }
    if path.join("mods").exists() { return "Forge"; }
    "Vanilla"
}

/// 生成离线 UUID（HMCL 风格：MD5("OfflinePlayer:" + name)）
pub(crate) fn offline_uuid(name: &str) -> String {
    let input = format!("OfflinePlayer:{}", name);
    let digest = md5::compute(input.as_bytes());
    let b = digest.as_slice();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6] & 0x0f | 0x30, b[7],
        b[8] & 0x3f | 0x80, b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// 递归移动目录内容（源目录保留，不删除）
pub(crate) fn move_dir_contents(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if !src.exists() { return Ok(()); }
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;

    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let dest = dst.join(entry.file_name());

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
