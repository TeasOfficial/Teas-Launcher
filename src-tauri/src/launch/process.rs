use super::{LaunchContext, RUNNING_PID};
use super::java::{compare_versions, guess_game_version};
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

// ── 预启动处理 ──────────────────────────────────────────────

pub(super) fn pre_run_setup(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<(), String> {
    // 提取 log4j2.xml（1.7+）
    let game_version = guess_game_version(version);
    if let Some(ref gv) = game_version {
        if compare_versions(gv, "1.7") >= std::cmp::Ordering::Equal {
            extract_log4j_config(ctx, version)?;
        }
    }

    // 更新 options.txt 语言（游戏目录下，随版本隔离）
    update_options_txt(ctx);

    Ok(())
}

/// 提取 log4j2.xml 到版本目录（-Dlog4j.configurationFile=${path} 指向这里）
pub(super) fn extract_log4j_config(
    ctx: &LaunchContext,
    version: &serde_json::Value,
) -> Result<(), String> {
    let version_dir = ctx.dot_minecraft
        .join("versions").join(&ctx.instance_name);
    let target = version_dir.join("log4j2.xml");

    if target.exists() {
        return Ok(());
    }

    let game_version = guess_game_version(version).unwrap_or_default();
    let is_legacy = compare_versions(&game_version, "1.12") == std::cmp::Ordering::Less;

    let log4j_xml = if is_legacy {
        // 1.7-1.11: 旧版 log4j 2.0-beta9
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="WARN" packages="com.mojang.util">
    <Appenders>
        <Console name="SysOut" target="SYSTEM_OUT">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
        </Console>
        <Queue name="ServerGuiConsole">
            <PatternLayout pattern="[%d{HH:mm:ss} %level]: %msg{nolookups}%n" />
        </Queue>
        <RollingRandomAccessFile name="File" fileName="logs/latest.log" filePattern="logs/%d{yyyy-MM-dd}-%i.log.gz">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
            <Policies>
                <TimeBasedTriggeringPolicy />
                <OnStartupTriggeringPolicy />
            </Policies>
        </RollingRandomAccessFile>
    </Appenders>
    <Loggers>
        <Root level="info">
            <filters>
                <MarkerFilter marker="NETWORK_PACKETS" onMatch="DENY" onMismatch="NEUTRAL" />
            </filters>
            <AppenderRef ref="SysOut"/>
            <AppenderRef ref="File"/>
        </Root>
    </Loggers>
</Configuration>"#.to_string()
    } else {
        // 1.12+: 新版 log4j 2.x
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Configuration status="WARN">
    <Appenders>
        <Console name="SysOut" target="SYSTEM_OUT">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
        </Console>
        <Queue name="ServerGuiConsole">
            <PatternLayout pattern="[%d{HH:mm:ss} %level]: %msg{nolookups}%n" />
        </Queue>
        <RollingRandomAccessFile name="File" fileName="logs/latest.log" filePattern="logs/%d{yyyy-MM-dd}-%i.log.gz">
            <PatternLayout pattern="[%d{HH:mm:ss}] [%t/%level]: %msg{nolookups}%n" />
            <Policies>
                <TimeBasedTriggeringPolicy />
                <OnStartupTriggeringPolicy />
            </Policies>
        </RollingRandomAccessFile>
    </Appenders>
    <Loggers>
        <Root level="info">
            <filters>
                <MarkerFilter marker="NETWORK_PACKETS" onMatch="DENY" onMismatch="NEUTRAL" />
            </filters>
            <AppenderRef ref="SysOut"/>
            <AppenderRef ref="File"/>
            <AppenderRef ref="ServerGuiConsole"/>
        </Root>
    </Loggers>
</Configuration>"#.to_string()
    };

    std::fs::write(&target, &log4j_xml)
        .map_err(|e| format!("写入 log4j2.xml 失败: {}", e))?;

    log::debug!("[launch] 已提取 log4j2.xml");
    Ok(())
}

/// 更新 options.txt 语言设置（游戏目录，随版本隔离）
pub(super) fn update_options_txt(ctx: &LaunchContext) {
    let options_path = ctx.game_directory().join("options.txt");

    if !options_path.exists() {
        return;
    }

    if let Ok(content) = std::fs::read_to_string(&options_path) {
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let mut changed = false;
        for line in &mut lines {
            if line.starts_with("lang:") {
                if *line != "lang:zh_cn" {
                    *line = "lang:zh_cn".to_string();
                    changed = true;
                }
            }
        }
        if changed {
            let _ = std::fs::write(&options_path, lines.join("\n"));
            log::debug!("[launch] 已更新 options.txt 语言设置");
        }
    }
}

// ── 进程启动 ─────────────────────────────────────────────────

/// 直接使用 ProcessBuilder 启动进程（Vec<String>，不做拼接→再分割）
pub(super) fn spawn_process(
    ctx: &LaunchContext,
    flat_args: &[String],
    version: &serde_json::Value,
) -> Result<std::process::Child, String> {
    if flat_args.is_empty() {
        return Err("启动参数为空".to_string());
    }

    let program = &flat_args[0];
    let args: &[String] = &flat_args[1..];

    // 工作目录 = 游戏目录（随版本隔离设置）
    let working_dir = ctx.game_directory();

    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd.current_dir(&working_dir);

    // ── 环境变量 ──
    cmd.env("APPDATA", ctx.dot_minecraft.display().to_string());

    if let Some(java_bin) = ctx.java_path.parent() {
        let existing_path = std::env::var("Path").unwrap_or_default();
        cmd.env("Path", format!("{};{}", java_bin.display(), existing_path));
    }

    cmd.env("INST_NAME", &ctx.instance_name);
    cmd.env("INST_ID", &ctx.instance_name);
    cmd.env("INST_DIR", working_dir.display().to_string());
    // HMCL 语义: INST_MC_DIR 指向游戏目录（版本隔离时 = versions/<id>）
    cmd.env("INST_MC_DIR", working_dir.display().to_string());
    cmd.env("INST_JAVA", ctx.java_path.display().to_string());

    // 加载器环境变量（HMCL getEnvVars，模组/整合包可能检测）
    for (k, v) in detect_loader_envs(version) {
        cmd.env(k, v);
    }

    // ── 重定向输出 ──
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());

    // ── Windows: 不显示控制台窗口 ──
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn().map_err(|e| {
        format!(
            "启动 Minecraft 失败: {}\nJava 路径: {}\n请检查 Java 是否正确安装。",
            e, ctx.java_path.display()
        )
    })?;

    Ok(child)
}

/// 从 libraries 检测加载器并生成 INST_* 环境变量
pub(super) fn detect_loader_envs(version: &serde_json::Value) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    if let Some(libs) = version["libraries"].as_array() {
        for lib in libs {
            let name = lib["name"].as_str().unwrap_or("");
            if name.starts_with("net.minecraftforge:forge:")
                || name.starts_with("net.minecraftforge:fmlloader")
            {
                if !out.iter().any(|(k, _)| *k == "INST_FORGE") {
                    out.push(("INST_FORGE", "1"));
                }
            }
            if name.starts_with("net.neoforged:") {
                if !out.iter().any(|(k, _)| *k == "INST_NEOFORGE") {
                    out.push(("INST_NEOFORGE", "1"));
                }
            }
            if name.starts_with("net.fabricmc:fabric-loader") {
                out.push(("INST_FABRIC", "1"));
            }
            if name.starts_with("org.quiltmc:quilt-loader") {
                out.push(("INST_QUILT", "1"));
            }
            if name.starts_with("com.mumfrey:liteloader") {
                out.push(("INST_LITELOADER", "1"));
            }
            if name.starts_with("optifine:OptiFine") || name.starts_with("optifine:optifine") {
                out.push(("INST_OPTIFINE", "1"));
            }
            if name.starts_with("zone.rong:cleanroomloader") {
                out.push(("INST_CLEANROOM", "1"));
            }
        }
    }
    out
}

// ── 进程监控 ─────────────────────────────────────────────────

/// 启动 stdout/stderr 流监控 + 退出等待 + 崩溃检测
///
/// 参照 HMCL DefaultLauncher.startMonitors:
/// - stdout/stderr pump 线程（同时累积日志用于退出分类）
/// - ExitWaiter（spawn_blocking 等待退出，避免阻塞 tokio）
pub(super) fn monitor_process(
    app: tauri::AppHandle,
    mut child: std::process::Child,
    ctx: LaunchContext,
    mc_dir: String,
) {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // 共享日志缓冲（pump 线程写入，退出分类读取）
    let log_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    // stderr 单独留一份，退出时落盘成 .teas/last_stderr.log 供 crash.rs 分析
    let stderr_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // stdout pump
    if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let instance = ctx.instance_name.clone();
        let log_buf = log_buf.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(text) = line {
                    let mut buf = log_buf.lock().unwrap();
                    buf.push(text.clone());
                    if buf.len() > 500 {
                        buf.remove(0);
                    }
                    drop(buf);
                    // debug 级：游戏自身有 latest.log/debug.log，启动器日志不该被它淹没
                    log::debug!("[MC:stdout] {}", text);
                    let _ = app_clone.emit("game-log", serde_json::json!({
                        "instance": instance,
                        "stream": "stdout",
                        "message": text,
                    }));
                }
            }
        });
    }

    // stderr pump
    if let Some(stderr) = stderr {
        let app_clone = app.clone();
        let instance = ctx.instance_name.clone();
        let log_buf = log_buf.clone();
        let stderr_buf = stderr_buf.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(text) = line {
                    let mut buf = log_buf.lock().unwrap();
                    buf.push(text.clone());
                    if buf.len() > 500 {
                        buf.remove(0);
                    }
                    drop(buf);

                    // 单独累积 stderr（限长，避免内存无限增长）
                    {
                        let mut sb = stderr_buf.lock().unwrap();
                        sb.push(text.clone());
                        if sb.len() > 2000 {
                            sb.remove(0);
                        }
                    }
                    log::debug!("[MC:stderr] {}", text);
                    let _ = app_clone.emit("game-log", serde_json::json!({
                        "instance": instance,
                        "stream": "stderr",
                        "message": text,
                    }));
                }
            }
        });
    }

    // ExitWaiter
    let app_clone = app;
    let instance_name = ctx.instance_name.clone();
    let ctx_clone = ctx.clone();

    tokio::task::spawn_blocking(move || {
        let status = child.wait();
        let exit_code = status.as_ref().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);

        // 清除 PID
        *RUNNING_PID.lock().unwrap() = None;

        log::info!("[launch] Minecraft 进程已退出，退出码: {}", exit_code);

        // 等待 pump 线程 flush 完管道尾部日志（避免尾部崩溃信息丢失导致误分类）
        std::thread::sleep(std::time::Duration::from_millis(300));

        // 把本次 stderr 落盘成 .teas/last_stderr.log — crash.rs 依赖它做崩溃分析。
        // 此前这个文件没有任何写入端（只有读取端），导致该检测路径从未生效。
        if let Ok(teas) = crate::config::teas_dir() {
            let sb = stderr_buf.lock().unwrap();
            let joined = sb.join("\n");
            const KEEP: usize = 32_000; // crash.rs 只读前 8000 字符，留足余量
            let n = joined.chars().count();
            let tail: String = if n > KEEP {
                joined.chars().skip(n - KEEP).collect()
            } else {
                joined
            };
            if let Err(e) = std::fs::write(teas.join("last_stderr.log"), tail) {
                log::warn!("[launch] 写入 last_stderr.log 失败: {}", e);
            }
        }

        // 用户主动终止：taskkill 会给出非零退出码，与崩溃无法从退出码区分，
        // 直接按"已停止"上报，不做崩溃检测，否则每次点停止都会误报崩溃。
        if super::USER_KILLED.load(std::sync::atomic::Ordering::SeqCst) {
            log::info!("[launch] {} 由用户主动终止", instance_name);
            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "exitType": "STOPPED",
                "crashed": false,
            }));
            drop(ctx_clone);
            return;
        }

        // 退出分类
        let logs: Vec<String> = log_buf.lock().unwrap().clone();
        let exit_type = classify_exit(exit_code, &logs);

        if exit_type == "NORMAL" {
            log::info!("[launch] {} 正常退出", instance_name);
            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "exitType": exit_type,
                "crashed": false,
            }));
        } else {
            log::error!("[launch] {} 异常退出 ({}, {}), 开始崩溃检测...",
                instance_name, exit_code, exit_type);

            // 等待日志文件写完
            std::thread::sleep(std::time::Duration::from_secs(1));

            let crash_result = crate::crash::check_crash(mc_dir, instance_name.clone());

            let _ = app_clone.emit("game-exit", serde_json::json!({
                "instance": instance_name,
                "exitCode": exit_code,
                "exitType": exit_type,
                "crashed": crash_result.is_some(),
                "crashInfo": crash_result,
            }));
        }

        // ctx 保持存活到进程退出（引用内部字段）
        drop(ctx_clone);
    });
}

/// 退出类型分类（HMCL ExitType 简化版）:
/// - JVM_ERROR: JVM 启动失败（参数错误/找不到类等，含中文 Windows 本地化消息）
/// - CRASH: 游戏崩溃（Minecraft Crash Report）
/// - NORMAL: 退出码 0
/// - ERROR: 其他非零退出
pub(super) fn classify_exit(exit_code: i32, logs: &[String]) -> &'static str {
    const JVM_FAIL: &[&str] = &[
        // 英文
        "Error: Could not find or load main class",
        "Unrecognized option",
        "Unrecognized VM option",
        "Could not create the Java Virtual Machine",
        "Unable to access jarfile",
        "Error occurred during initialization of VM",
        "Invalid maximum heap size",
        "Could not reserve enough space",
        "Unknown module",
        "Module not found",
        // 中文（Windows 本地化 JVM 消息）
        "找不到或无法加载主类",
        "无法创建 Java 虚拟机",
        "无法识别",
        "无法访问 jarfile",
        "初始化 VM 期间发生错误",
        "无法解析的模块",
        "无效的最大堆大小",
        "无法保留足够的空间",
        "模块未找到",
        "模块不存在",
    ];
    const CRASH: &[&str] = &[
        "---- Minecraft Crash Report ----",
        "Crash report saved to",
        "#@!@#",
    ];

    let joined = logs.join("\n");
    if JVM_FAIL.iter().any(|k| joined.contains(k)) {
        return "JVM_ERROR";
    }
    if CRASH.iter().any(|k| joined.contains(k)) {
        return "CRASH";
    }
    if exit_code == 0 {
        "NORMAL"
    } else {
        "ERROR"
    }
}

