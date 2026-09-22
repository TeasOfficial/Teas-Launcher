use super::{GcMode, LaunchContext, LUA_JAR_BYTES};
use super::java::{detect_java_version, is_64bit_java};
use std::io::Write;
use crate::version::library::{json_rule_check, mclib_get, mclib_get_with_classifier};
use crate::BUILD;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── 参数构建 (核心, 纯函数) ──────────────────────────────────

/// 构建完整命令行参数列表（Vec<String>，不做拼接→再分割）
///
/// 参数顺序 (HMCL-style):
///   [java.exe] + [版本 JSON jvm + 默认补全 + 用户 + 生成, 统一去重]
///   + [mainClass] + [游戏参数, 去重]
pub(super) fn build_flat_args(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> Result<Vec<String>, String> {
    let java_version = detect_java_version(&ctx.java_path)?;
    let is_64bit = is_64bit_java(&ctx.java_path);
    Ok(build_flat_args_inner(ctx, version, natives_dir, java_version, is_64bit))
}

/// 内部实现（java 版本已知，便于单元测试）
pub(super) fn build_flat_args_inner(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
    java_version: u32,
    is_64bit: bool,
) -> Vec<String> {
    let map = build_placeholders(ctx, version, natives_dir);

    let mut args: Vec<String> = Vec::new();

    // Java executable（仅当文件名是 javaw 时替换，避免目录名误伤）
    let java_exe = {
        let fname = ctx.java_path.file_name().map(|f| f.to_string_lossy().to_string());
        let is_javaw = fname.as_deref().map_or(false, |f| {
            f.eq_ignore_ascii_case("javaw.exe") || f.eq_ignore_ascii_case("javaw")
        });
        if is_javaw {
            #[cfg(target_os = "windows")]
            { ctx.java_path.with_file_name("java.exe").display().to_string() }
            #[cfg(not(target_os = "windows"))]
            { ctx.java_path.with_file_name("java").display().to_string() }
        } else {
            ctx.java_path.display().to_string()
        }
    };
    args.push(java_exe);

    // JVM 参数（JSON + 默认补全 + 用户 + 生成，统一去重）
    let jvm = build_jvm_args(ctx, version, &map, natives_dir, java_version, is_64bit);
    args.extend(jvm);

    // Main class
    let main_class = version["mainClass"]
        .as_str()
        .unwrap_or("net.minecraft.client.main.Main");
    args.push(main_class.to_string());

    // 游戏参数
    let game = build_game_args(ctx, version, &map);
    args.extend(game);

    args
}

/// JVM 参数构建:
///   1. 版本 JSON arguments.jvm（rules 过滤 + 占位符替换）
///   2. 默认补全（旧版 JSON 无 -cp / -Djava.library.path 时，HMCL DEFAULT_JVM_ARGUMENTS）
///   3. 用户自定义参数
///   4. 启动器生成参数（内存/GC/编码/安全）
///   5. GC 管理（非 Custom: 清除所有 GC 参数后添加我们的）
///   6. 跨段去重（受保护 key 保留 JSON 版，其余后者覆盖前者）
pub(super) fn build_jvm_args(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    map: &HashMap<String, String>,
    natives_dir: &Path,
    java_version: u32,
    is_64bit: bool,
) -> Vec<String> {
    let mut jvm: Vec<String> = Vec::new();

    // 1) 版本 JSON jvm 参数
    let json_jvm = collect_json_args(Some(&version["arguments"]["jvm"]), map, false);
    let has_cp = json_jvm.iter().any(|a| a == "-cp" || a == "-classpath");
    let has_libpath = json_jvm.iter().any(|a| a.starts_with("-Djava.library.path"));
    jvm.extend(json_jvm);

    // 2) 默认补全（HMCL DEFAULT_JVM_ARGUMENTS: -Djava.library.path + -cp）
    if !has_cp {
        jvm.push("-cp".to_string());
        jvm.push(map.get("${classpath}").cloned().unwrap_or_default());
    }
    if !has_libpath {
        jvm.push(format!("-Djava.library.path={}", natives_dir.display()));
    }

    // 3) 用户自定义参数
    let mut user = split_args_keep_quoting(&ctx.jvm_args);

    // 4) 生成参数
    let mut gen: Vec<String> = Vec::new();
    gen.push("-XX:-OmitStackTraceInFastThrow".to_string());
    gen.push("-Djdk.lang.Process.allowAmbiguousCommands=True".to_string());
    gen.push("-Dfml.ignoreInvalidMinecraftCertificates=True".to_string());
    gen.push("-Dfml.ignorePatchDiscrepancies=True".to_string());
    gen.push(format!("-Xmx{}m", ctx.max_memory));
    gen.push("-Dlog4j2.formatMsgNoLookups=true".to_string());

    // ★ log4j 配置: 版本 JSON 的 logging.client.argument 里 ${path} 按官方语义是
    // "日志配置文件路径"，不是版本目录。启动器已把 log4j2.xml 释放到版本目录
    // （process::extract_log4j_config，在 spawn 之前完成），这里指向那个文件。
    // 先前把 ${path} 当版本目录替换，JVM 拿到的是一个目录，log4j 启动即报
    // "Cannot locate file ... 拒绝访问"，自己提取的配置根本没被用上。
    let log4j_xml = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name)
        .join("log4j2.xml");
    let log4j_arg = version["logging"]["client"]["argument"]
        .as_str()
        .map(|a| a.replace("${path}", &log4j_xml.display().to_string()))
        .unwrap_or_else(|| format!("-Dlog4j.configurationFile={}", log4j_xml.display()));
    gen.push(log4j_arg);

    // ★ 主 JAR 路径（HMCL 行为）: 某些 mod（OptiFine 等）通过
    // -Dminecraft.client.jar 定位客户端 jar
    gen.push(format!("-Dminecraft.client.jar={}", primary_jar_path(ctx, version).display()));

    if java_version >= 9 {
        gen.push("-Dstdout.encoding=UTF-8".to_string());
        gen.push("-Dstderr.encoding=UTF-8".to_string());
    }
    if java_version >= 18 {
        gen.push("-Dfile.encoding=COMPAT".to_string());
    }
    if java_version == 16 {
        gen.push("--illegal-access=permit".to_string());
    }
    if java_version >= 24 {
        gen.push("--sun-misc-unsafe-memory-access=allow".to_string());
    }

    // LWJGL 3.4.1 兼容 agent（PCL LUA.jar: 类移入 native JAR 的回调兼容层）
    if needs_lua_agent(version) {
        let lua_path = ctx.dot_minecraft.join("lua-agent.jar");
        if !lua_path.exists() {
            if let Ok(mut f) = std::fs::File::create(&lua_path) {
                let _ = f.write_all(LUA_JAR_BYTES);
            }
        }
        if lua_path.exists() {
            gen.push(format!("-javaagent:{}", lua_path.display()));
        }
    }

    // 5) 合并 + 清理问题参数
    let mut all = jvm;
    all.append(&mut user);
    all.append(&mut gen);
    // 清理 -XX:MaxDirectMemorySize=256M（PCL #3511）
    all.retain(|a| a != "-XX:MaxDirectMemorySize=256M");

    // GC 管理: 非 Custom 时清除所有 GC 参数（含用户/JSON 的），再添加我们的
    if ctx.gc_mode != GcMode::Custom {
        all.retain(|a| !is_gc_arg(a));
        all.extend(gen_gc_args(java_version, is_64bit));
    }

    // 6) 跨段去重
    deduplicate_jvm_args(all)
}

/// 判断是否为 GC 相关参数（清除时使用）
pub(super) fn is_gc_arg(a: &str) -> bool {
    const EXACT: &[&str] = &[
        "-XX:+UseG1GC", "-XX:-UseG1GC",
        "-XX:+UseZGC", "-XX:-UseZGC",
        "-XX:+UseSerialGC", "-XX:-UseSerialGC",
        "-XX:+UseParallelGC", "-XX:-UseParallelGC",
        "-XX:+UseConcMarkSweepGC", "-XX:-UseConcMarkSweepGC",
        "-XX:+UseShenandoahGC", "-XX:-UseShenandoahGC",
        "-XX:+ZGenerational", "-XX:-ZGenerational",
        "-XX:+UseCompactObjectHeaders", "-XX:-UseCompactObjectHeaders",
    ];
    const PREFIX: &[&str] = &[
        "-XX:G1NewSizePercent", "-XX:G1ReservePercent",
        "-XX:G1HeapRegionSize", "-XX:MaxGCPauseMillis",
        "-XX:MinHeapFreeRatio", "-XX:MaxHeapFreeRatio",
        "-XX:G1MixedGCCountTarget", "-XX:+ParallelRefProcEnabled",
        "-XX:+PerfDisableSharedMem",
    ];
    EXACT.contains(&a) || PREFIX.iter().any(|p| a.starts_with(p))
}

/// 生成 GC 优化参数（Java 版本自适应）
pub(super) fn gen_gc_args(java_version: u32, is_64bit: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Auto/G1GC 均使用 G1GC（PCL 行为，ZGC 在新版 LWJGL 上兼容性差；
    // GcMode::Custom 不调用本函数，GC 由用户全权负责）
    out.push("-XX:+UnlockExperimentalVMOptions".to_string());

    if java_version >= 24 && is_64bit {
        out.push("-XX:+UseCompactObjectHeaders".to_string());
    }
    out.push("-XX:+UseG1GC".to_string());
    out.push("-XX:G1NewSizePercent=20".to_string());
    out.push("-XX:G1ReservePercent=20".to_string());
    out.push("-XX:G1HeapRegionSize=32M".to_string());
    out.push("-XX:MaxGCPauseMillis=50".to_string());
    out.push("-XX:+PerfDisableSharedMem".to_string());
    if java_version >= 12 {
        out.push("-XX:MinHeapFreeRatio=25".to_string());
        out.push("-XX:MaxHeapFreeRatio=40".to_string());
    }

    if !is_64bit {
        out.push("-Xss1m".to_string());
    }

    out
}

/// 判断是否使用 LWJGL 3.4.1（需要 LUA agent 兼容层）
pub(super) fn needs_lua_agent(version: &serde_json::Value) -> bool {
    version["libraries"].as_array().map_or(false, |libs| {
        libs.iter().any(|lib| {
            lib["name"].as_str().unwrap_or("") == "org.lwjgl:lwjgl:3.4.1"
        })
    })
}

/// 游戏参数构建:
///   1. minecraftArguments（旧版格式，≤1.12.2）
///   2. arguments.game（新版格式，rules/features 过滤 + quickPlay 跳过）
///   3. 两者皆无时生成默认参数
///   4. 宽高补充 → OptiFine tweakClass 重排 → 去重
pub(super) fn build_game_args(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    map: &HashMap<String, String>,
) -> Vec<String> {
    let mut game: Vec<String> = Vec::new();
    let mut has_json_args = false;

    // 1) minecraftArguments（旧版）
    if let Some(mc_args) = version["minecraftArguments"].as_str() {
        if !mc_args.is_empty() {
            has_json_args = true;
            let resolved = substitute(mc_args, map);
            game.extend(split_args_keep_quoting(&resolved));
        }
    }

    // 2) arguments.game（新版）
    let json_game = collect_json_args(Some(&version["arguments"]["game"]), map, true);
    if !json_game.is_empty() {
        has_json_args = true;
    }
    game.extend(json_game);

    // 3) 两者皆无 → 默认参数
    if !has_json_args {
        let asset_index = version["assets"].as_str().unwrap_or("legacy");
        let version_type = version["type"].as_str().unwrap_or("release");
        let version_id = version["id"].as_str().unwrap_or(&ctx.instance_name);
        let defaults = [
            "--username", &ctx.auth.player_name,
            "--version", version_id,
            "--gameDir", "${game_directory}",
            "--assetsDir", "${assets_root}",
            "--assetIndex", asset_index,
            "--uuid", "${auth_uuid}",
            "--accessToken", "${auth_access_token}",
            "--userType", "${user_type}",
            "--versionType", version_type,
        ];
        for s in defaults {
            game.push(substitute(s, map));
        }
    }

    // 4) 宽高（JSON 未提供时补充，旧版 minecraftArguments 无宽高）
    if !game.iter().any(|a| a == "--width") {
        game.push("--width".to_string());
        game.push(ctx.window_width.to_string());
        game.push("--height".to_string());
        game.push(ctx.window_height.to_string());
    }

    // 5) OptiFine 的 tweakClass 移到末尾（PCL-style）
    if let Some(pos) = game.iter().position(|a| a == "--tweakClass") {
        if game.get(pos + 1).map_or(false, |tc| tc.contains("OptiFine")) {
            let tc = game.remove(pos + 1);
            game.remove(pos);
            game.push("--tweakClass".to_string());
            game.push(tc);
        }
    }

    // 6) 去重（游戏参数同 key 覆盖，--tweakClass 除外）
    deduplicate_args(&game, false)
}

/// Classpath 构建:
/// - library rules 过滤（OS/features）
/// - natives classifier 正确解析（含 ${arch} 替换、跳过非 Windows natives）
/// - OptiFine 放倒数第二位（PCL-style）
/// - 主 JAR 最后（jar 字段 / inheritsFrom 重定向）
pub(super) fn build_classpath(ctx: &LaunchContext, version: &serde_json::Value) -> Vec<String> {
    let mut cp: Vec<String> = Vec::new();
    let mut optifine_path: Option<String> = None;

    if let Some(libs) = version["libraries"].as_array() {
        for lib in libs {
            // rules 过滤（跳过其他平台的库）
            if !json_rule_check(lib.get("rules")) {
                continue;
            }
            let name = lib["name"].as_str().unwrap_or("");

            let path = match lib_classifier(lib) {
                ClassifierKind::Skip => continue,
                // ★ mclib_get/mclib_get_with_classifier 内部会自行 join "libraries"，
                // 必须传 .minecraft 根目录 —— 传 libs_dir 会导致路径多一层 libraries\，
                // classpath 全部指向不存在的文件（bootstraplauncher 找不到 modlauncher）
                ClassifierKind::Plain => mclib_get(name, &ctx.dot_minecraft),
                ClassifierKind::Named(c) => mclib_get_with_classifier(name, &c, &ctx.dot_minecraft),
            };
            let path_str = path.display().to_string().replace('/', "\\");

            if name.starts_with("optifine:OptiFine") || name.starts_with("optifine:optifine") {
                optifine_path = Some(path_str);
            } else {
                cp.push(path_str);
            }
        }
    }

    // OptiFine 放到倒数第二位（主 JAR 之前）
    if let Some(optifine) = optifine_path {
        if cp.len() >= 2 {
            cp.insert(cp.len() - 1, optifine);
        } else {
            cp.push(optifine);
        }
    }

    // 主 JAR 最后（jar 字段重定向）
    let primary = primary_jar_path(ctx, version);
    cp.push(primary.display().to_string().replace('/', "\\"));

    // ★ 去重（保持首次出现顺序）: 部分启动器（如 HMCL）生成的版本 JSON 含重复 libraries
    // 条目，若同一 jar 在 classpath 出现两次，bootstraplauncher 的 UnionFileSystem 构建
    // 模块时会抛 "Duplicate key ..." 异常
    let mut seen = std::collections::HashSet::new();
    cp.retain(|p| seen.insert(p.clone()));

    cp
}

/// library 的 classifier 分类
pub(super) enum ClassifierKind {
    /// 无 classifier（普通 JAR）
    Plain,
    /// 非 Windows 平台的 natives（跳过）
    Skip,
    /// 指定 classifier（含 ${arch} 已替换）
    Named(String),
}

/// 解析 library 的 natives classifier:
/// 1. name 第 4 段（如 org.lwjgl:lwjgl-platform:3.4.1:natives-windows）
/// 2. natives map 的 windows 键（如 lwjgl 3.x）
pub(super) fn lib_classifier(lib: &serde_json::Value) -> ClassifierKind {
    if let Some(name) = lib["name"].as_str() {
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() >= 4 {
            let arch = if cfg!(target_arch = "x86_64") { "64" } else { "32" };
            let c = parts[3].replace("${arch}", arch);
            if c.starts_with("natives-") {
                return if c.contains("windows") {
                    ClassifierKind::Named(c)
                } else {
                    ClassifierKind::Skip
                };
            }
            return ClassifierKind::Named(c);
        }
    }
    if let Some(n) = lib.get("natives")
        .and_then(|n| n.get("windows"))
        .and_then(|v| v.as_str())
    {
        return ClassifierKind::Named(n.to_string());
    }
    ClassifierKind::Plain
}

/// 主游戏 JAR 路径（jar 字段重定向）:
///   1. versions/<id>/<id>.jar（自身，含 downloads.client 的版本）
///   2. versions/<jar>/<jar>.jar（1.13- 的 "jar" 字段）
///   3. versions/<inheritsFrom>/<inheritsFrom>.jar（父版本）
/// 全部不存在时返回自身路径（便于报错提示）
pub(super) fn primary_jar_path(ctx: &LaunchContext, version: &serde_json::Value) -> PathBuf {
    let versions = ctx.dot_minecraft.join("versions");
    let self_path = versions
        .join(&ctx.instance_name)
        .join(format!("{}.jar", ctx.instance_name));

    let mut candidates: Vec<PathBuf> = vec![self_path.clone()];
    if let Some(jar) = version["jar"].as_str() {
        if !jar.is_empty() && jar != ctx.instance_name {
            candidates.push(versions.join(jar).join(format!("{}.jar", jar)));
        }
    }
    if let Some(p) = version["inheritsFrom"].as_str() {
        if !p.is_empty() && p != ctx.instance_name {
            candidates.push(versions.join(p).join(format!("{}.jar", p)));
        }
    }

    candidates.into_iter().find(|p| p.exists()).unwrap_or(self_path)
}

/// 占位符映射（Mojang 官方 + HMCL 扩展）
///
/// 注意: `${path}` 不在本表内。它只出现在 logging.client.argument 里，官方语义是
/// "日志配置文件路径"，由 build_jvm_args 单独替换 —— 曾经把它全局映射成版本目录，
/// 导致 log4j 收到一个目录而启动报错。
pub(super) fn build_placeholders(
    ctx: &LaunchContext,
    version: &serde_json::Value,
    natives_dir: &Path,
) -> HashMap<String, String> {
    let libs_dir = ctx.dot_minecraft.join("libraries");
    let assets_dir = ctx.dot_minecraft.join("assets");
    let game_dir = ctx.game_directory();
    let asset_index = version["assets"].as_str().unwrap_or("legacy");
    let version_type = version["type"].as_str().unwrap_or("release");
    let primary = primary_jar_path(ctx, version);
    let launcher_ver = format!("v{}.{:04}", env!("CARGO_PKG_VERSION"), BUILD);

    let cp_sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let file_sep = if cfg!(target_os = "windows") { "\\" } else { "/" };
    let cp_str = build_classpath(ctx, version).join(cp_sep);

    let mut m: HashMap<String, String> = HashMap::new();

    // 账户（AuthInfo 注入，为微软登录预留）
    m.insert("${auth_player_name}".into(), ctx.auth.player_name.clone());
    m.insert("${auth_uuid}".into(), ctx.auth.uuid.clone());
    m.insert("${auth_access_token}".into(), ctx.auth.access_token.clone());
    m.insert("${access_token}".into(), ctx.auth.access_token.clone());
    m.insert("${auth_session}".into(), ctx.auth.access_token.clone());
    m.insert("${auth_xuid}".into(), ctx.auth.xuid.clone());
    m.insert("${clientid}".into(), ctx.auth.client_id.clone());
    m.insert("${user_type}".into(), ctx.auth.user_type.clone());
    m.insert("${user_properties}".into(), ctx.auth.user_properties.clone());

    // 版本信息
    m.insert("${version_name}".into(), ctx.instance_name.clone());
    m.insert("${version_type}".into(), version_type.to_string());
    m.insert("${assets_index_name}".into(), asset_index.to_string());

    // 目录
    m.insert("${assets_root}".into(), assets_dir.display().to_string());
    m.insert("${game_assets}".into(), assets_dir.display().to_string());
    m.insert("${game_directory}".into(), game_dir.display().to_string());
    m.insert("${library_directory}".into(), libs_dir.display().to_string());
    m.insert("${libraries_directory}".into(), libs_dir.display().to_string());
    m.insert("${natives_directory}".into(), natives_dir.display().to_string());
    // ${path} 故意不在这里 —— 见本函数文档注释

    // 分辨率
    m.insert("${resolution_width}".into(), ctx.window_width.to_string());
    m.insert("${resolution_height}".into(), ctx.window_height.to_string());

    // 分隔符
    m.insert("${classpath_separator}".into(), cp_sep.to_string());
    m.insert("${file_separator}".into(), file_sep.to_string());

    // 启动器信息
    m.insert("${launcher_name}".into(), "TeasLauncher".to_string());
    m.insert("${launcher_version}".into(), launcher_ver);
    m.insert("${profile_name}".into(), "TeasLauncher".to_string());
    m.insert("${language}".into(), "zh_cn".to_string());

    // 主 JAR
    m.insert("${primary_jar}".into(), primary.display().to_string());
    m.insert(
        "${primary_jar_name}".into(),
        primary.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
    );

    // classpath（作为占位符，版本 JSON 的 -cp ${classpath} 需要）
    m.insert("${classpath}".into(), cp_str);

    // QuickPlay（不支持，置空）
    m.insert("${quickPlayPath}".into(), String::new());
    m.insert("${quickPlaySingleplayer}".into(), String::new());
    m.insert("${quickPlayMultiplayer}".into(), String::new());
    m.insert("${quickPlayRealms}".into(), String::new());

    m
}

/// 占位符替换: 一次扫描替换 ${key}，未命中的占位符保留原文
/// （HMCL StringArgument 语义 — 未命中保留对 Forge 的 ${library_directory} 等很重要）
pub(super) fn substitute(input: &str, map: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(pos) = rest.find("${") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        if let Some(end) = after.find('}') {
            let key = &after[..end];
            // 映射表 key 形如 "${name}"
            if let Some(val) = map.get(&format!("${{{}}}", key)) {
                out.push_str(val);
                rest = &after[end + 1..];
            } else {
                // 未命中: 保留 "${" 原文，继续向后扫描
                out.push_str("${");
                rest = after;
            }
        } else {
            // 无闭合 }，保留剩余原文
            out.push_str(&rest[pos..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// 解析版本 JSON 的 arguments 数组（字符串 + rules 对象），
/// 过滤 + 占位符替换后返回参数列表
pub(super) fn collect_json_args(
    arr: Option<&serde_json::Value>,
    map: &HashMap<String, String>,
    is_game: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(list) = arr.and_then(|a| a.as_array()) else {
        return out;
    };

    for arg in list {
        if let Some(s) = arg.as_str() {
            if is_game && is_quickplay(s) {
                continue;
            }
            out.push(substitute(s, map));
        } else if let Some(obj) = arg.as_object() {
            if !apply_rules(obj) {
                continue;
            }
            if let Some(val) = obj.get("value") {
                if let Some(s) = val.as_str() {
                    if is_game && is_quickplay(s) {
                        continue;
                    }
                    out.push(substitute(s, map));
                } else if let Some(items) = val.as_array() {
                    for item in items {
                        if let Some(s) = item.as_str() {
                            if is_game && is_quickplay(s) {
                                continue;
                            }
                            out.push(substitute(s, map));
                        }
                    }
                }
            }
        }
    }
    out
}

pub(super) fn is_quickplay(s: &str) -> bool {
    s.contains("${quickPlay") || s.contains("${QuickPlay")
}

/// CompatibilityRule 语义的 rules 过滤（HMCL CompatibilityRule）:
/// - 无 rules → 允许
/// - 有 rules → 默认 disallow，按顺序遍历，最后命中的 action 生效
pub(super) fn apply_rules(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let rules = match obj.get("rules").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return true,
    };

    let mut required = false;

    for rule in rules {
        let action = rule.get("action").and_then(|a| a.as_str()).unwrap_or("allow");
        let mut matches = true;

        // OS 检查
        if let Some(os) = rule.get("os") {
            if let Some(name) = os.get("name").and_then(|n| n.as_str()) {
                matches = name == current_os_name();
            }
            if matches {
                if let Some(arch) = os.get("arch").and_then(|a| a.as_str()) {
                    let is_32bit = cfg!(target_arch = "x86");
                    matches = (arch == "x86") == is_32bit;
                }
            }
            if matches {
                if let Some(ver) = os.get("version").and_then(|v| v.as_str()) {
                    #[cfg(target_os = "windows")]
                    { matches = windows_version_match(ver); }
                    #[cfg(not(target_os = "windows"))]
                    { matches = false; }
                }
            }
        }

        // features 检查
        if matches {
            if let Some(features) = rule.get("features").and_then(|f| f.as_object()) {
                for (k, v) in features {
                    let actual = feature_value(k);
                    let want = v.as_bool().unwrap_or(false);
                    if actual != want {
                        matches = false;
                        break;
                    }
                }
            }
        }

        match action {
            "allow" => {
                if matches {
                    required = true;
                }
            }
            "disallow" => {
                if matches {
                    required = false;
                }
            }
            _ => {}
        }
    }

    required
}

pub(super) fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// 启动器支持的 features（决定 rules 中 feature 条件的真值）
pub(super) fn feature_value(key: &str) -> bool {
    match key {
        // 启动器总是设置 --width/--height
        "has_custom_resolution" => true,
        // 非 demo 用户
        "is_demo_user" => false,
        // 不支持 quickPlay
        "quick_play_singleplayer" | "quick_play_multiplayer" | "quick_play_realms" => false,
        _ => false,
    }
}

/// Windows 版本正则的简化匹配（官方 JSON 仅使用 ^10\. 之类）
#[cfg(target_os = "windows")]
pub(super) fn windows_version_match(pattern: &str) -> bool {
    let p = pattern.trim_start_matches('^').trim_start_matches('\\');
    p == "10" || p.starts_with("10.") || p.starts_with("10\\.") || p.starts_with("6.")
}

/// JVM 键值对选项（选项名与值作为两个独立参数）
pub(super) const PAIR_OPTIONS: &[&str] = &[
    "-cp", "-classpath", "-p", "--module-path",
    "--add-modules", "--add-opens", "--add-exports", "--add-reads",
    "--patch-module", "--limit-modules", "--upgrade-module-path",
    "--enable-native-access", "-agentlib", "-agentpath",
];

/// 不去重选项: 每条都打开不同的模块/包，必须全部保留
/// （--add-opens java.base/java.util.jar 与 --add-opens java.base/java.lang.invoke 是两条不同的授权）
pub(super) const NEVER_DEDUP: &[&str] = &["--add-opens", "--add-exports", "--add-reads"];

/// JVM 参数跨段去重:
/// - 受保护 key（-cp/-p/--add-modules/-Djava.library.path）保留最先出现（JSON 版）
/// - NEVER_DEDUP 选项全部保留（--add-opens/--add-exports/--add-reads）
/// - 其余 key 保留最后出现（用户/生成的覆盖 JSON 的，避免 -Xmx 重复导致 JVM 报错）
pub(super) fn deduplicate_jvm_args(args: Vec<String>) -> Vec<String> {
    struct Unit {
        key: String,
        start: usize,
        len: usize,
        never_dedup: bool,
    }

    // 1) 划分为参数单元（键值对如 -cp <path> / --add-opens <target> 是一个单元）
    let mut units: Vec<Unit> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let is_pair = PAIR_OPTIONS.contains(&a.as_str())
            || (a.starts_with("-D") && !a.contains('=') && i + 1 < args.len()
                && !args[i + 1].starts_with('-'));
        let len = if is_pair && i + 1 < args.len() { 2 } else { 1 };
        let key = jvm_key_of(a);
        let never_dedup = NEVER_DEDUP.contains(&key.as_str());
        units.push(Unit { key, start: i, len, never_dedup });
        i += len;
    }

    // 2) 冲突解决
    let mut keep = vec![true; args.len()];
    let mut first: HashMap<String, usize> = HashMap::new();
    let mut last: HashMap<String, usize> = HashMap::new();
    for (ui, u) in units.iter().enumerate() {
        if u.never_dedup {
            continue;
        }
        if is_protected_key(&u.key) {
            first.entry(u.key.clone()).or_insert(ui);
        } else {
            last.insert(u.key.clone(), ui);
        }
    }
    for (ui, u) in units.iter().enumerate() {
        if u.never_dedup {
            continue;
        }
        let dup = if is_protected_key(&u.key) {
            first.get(&u.key) != Some(&ui)
        } else {
            last.get(&u.key) != Some(&ui)
        };
        if dup {
            for k in u.start..u.start + u.len {
                keep[k] = false;
            }
        }
    }

    args.into_iter().zip(keep).filter(|(_, k)| *k).map(|(a, _)| a).collect()
}

/// 参数 key 归一化: -Xmx2048m → "-Xmx"; -Dfoo=1 → "-Dfoo";
/// -classpath → -cp; --module-path → -p; 其余 → 自身
pub(super) fn jvm_key_of(arg: &str) -> String {
    if let Some(_) = arg.strip_prefix("-Xmx") {
        return "-Xmx".to_string();
    }
    if let Some(_) = arg.strip_prefix("-Xms") {
        return "-Xms".to_string();
    }
    if let Some(_) = arg.strip_prefix("-Xss") {
        return "-Xss".to_string();
    }
    if let Some(rest) = arg.strip_prefix("-D") {
        if let Some(eq) = rest.find('=') {
            return format!("-D{}", &rest[..eq]);
        }
        return format!("-D{}", rest);
    }
    match arg {
        "-classpath" => "-cp".to_string(),
        "--module-path" => "-p".to_string(),
        _ => arg.to_string(),
    }
}

/// 受保护 key: 即使重复也保留版本 JSON 的（用户覆盖会破坏启动）
pub(super) fn is_protected_key(key: &str) -> bool {
    matches!(key, "-cp" | "-p" | "--add-modules" | "-Djava.library.path")
}

/// 分割 Java 参数字符串（保留引号内的空格，转义引号正确处理）
pub(super) fn split_args_keep_quoting(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
            current.push('\\');
            current.push('"');
            i += 1;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ' ' && !in_quotes {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// 游戏参数去重:
/// - 键值对（--xxx value）: 同 key 用新值覆盖旧值（--tweakClass 除外，保留多个）
/// - 单参数: 完全相同的只保留一个
pub(super) fn deduplicate_args(args: &[String], is_jvm: bool) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let key = &args[i];

        // 非 - 开头或单独参数
        if !key.starts_with('-')
            || i + 1 >= args.len()
            || (args[i + 1].starts_with('-') && args[i + 1].chars().nth(1).map_or(false, |c| !c.is_ascii_digit()))
        {
            if !result.contains(key) {
                result.push(key.clone());
            }
            i += 1;
        } else {
            // 键值对
            let value = &args[i + 1];
            let to_skip;

            if !is_jvm && key != "--tweakClass" {
                // 游戏参数: 查找并覆盖
                let mut found = false;
                let mut j = 0;
                while j < result.len() {
                    if j + 1 < result.len() && result[j] == *key {
                        result[j + 1] = value.clone();
                        found = true;
                        break;
                    }
                    j += 1;
                }
                to_skip = found;
            } else {
                // JVM 参数或 --tweakClass: 查找并跳过重复
                let mut found = false;
                let mut j = 0;
                while j < result.len() {
                    if j + 1 < result.len() && result[j] == *key && result[j + 1] == *value {
                        found = true;
                        break;
                    }
                    j += 1;
                }
                to_skip = found;
            }

            if !to_skip {
                result.push(key.clone());
                result.push(value.clone());
            }
            i += 2;
        }
    }

    result
}

