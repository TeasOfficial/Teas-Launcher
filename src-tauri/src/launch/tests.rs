    use super::args::*;
    use super::natives::natives_subdir;
    use super::process::classify_exit;
    use super::*;
    use crate::instance::resolve_version;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn fixture(name: &str) -> serde_json::Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests").join("fixtures").join(name);
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("读取 fixture {} 失败: {}", name, e)
        });
        serde_json::from_str(&content).unwrap_or_else(|e| {
            panic!("解析 fixture {} 失败: {}", name, e)
        })
    }

    fn test_ctx(instance_name: &str) -> LaunchContext {
        LaunchContext {
            dot_minecraft: PathBuf::from("C:\\fake\\mc"),
            instance_name: instance_name.to_string(),
            auth: AuthInfo::offline("Steve"),
            java_path: PathBuf::from("C:\\fake\\java\\bin\\java.exe"),
            max_memory: 2048,
            jvm_args: String::new(),
            window_width: 854,
            window_height: 480,
            prefer_official: true,
            gc_mode: GcMode::Auto,
            version_isolation: true,
        }
    }

    // ── substitute ──

    #[test]
    fn substitute_basic() {
        let mut m = HashMap::new();
        m.insert("${a}".into(), "1".into());
        m.insert("${b}".into(), "2".into());
        assert_eq!(substitute("x${a}y${b}z", &m), "x1y2z");
    }

    #[test]
    fn substitute_unknown_kept() {
        let m = HashMap::new();
        // 未命中的占位符保留原文（HMCL 语义）
        assert_eq!(substitute("${unknown} x", &m), "${unknown} x");
    }

    #[test]
    fn substitute_adjacent_and_missing_close() {
        let mut m = HashMap::new();
        m.insert("${a}".into(), "A".into());
        assert_eq!(substitute("${a}${a}", &m), "AA");
        assert_eq!(substitute("${a${b}", &m), "${a${b}");
        assert_eq!(substitute("no placeholder", &m), "no placeholder");
    }

    // ── apply_rules ──

    #[test]
    fn rules_no_rules_allows() {
        let obj = serde_json::json!({"value": "-Xss1M"}).as_object().unwrap().clone();
        assert!(apply_rules(&obj));
    }

    #[test]
    fn rules_default_disallow() {
        // 只有 disallow windows → windows 下 disallow → 结果 false
        let obj = serde_json::json!({
            "value": "-X",
            "rules": [{"action": "disallow", "os": {"name": "windows"}}]
        });
        let obj = obj.as_object().unwrap().clone();
        #[cfg(target_os = "windows")]
        assert!(!apply_rules(&obj));
        #[cfg(not(target_os = "windows"))]
        assert!(apply_rules(&obj));
    }

    #[test]
    fn rules_allow_windows() {
        let obj = serde_json::json!({
            "value": "-X",
            "rules": [{"action": "allow", "os": {"name": "windows"}}]
        });
        let obj = obj.as_object().unwrap().clone();
        #[cfg(target_os = "windows")]
        assert!(apply_rules(&obj));
        #[cfg(not(target_os = "windows"))]
        assert!(!apply_rules(&obj));
    }

    #[test]
    fn rules_last_match_wins() {
        // 先 allow 再 disallow → 最后命中 disallow → false
        let obj = serde_json::json!({
            "value": "-X",
            "rules": [
                {"action": "allow", "os": {"name": "windows"}},
                {"action": "disallow", "os": {"name": "windows"}}
            ]
        });
        let obj = obj.as_object().unwrap().clone();
        #[cfg(target_os = "windows")]
        assert!(!apply_rules(&obj));
    }

    #[test]
    fn rules_features() {
        // has_custom_resolution=true（我们总是设置分辨率）→ 允许
        let obj = serde_json::json!({
            "value": "--width",
            "rules": [{"action": "allow", "features": {"has_custom_resolution": true}}]
        });
        let obj = obj.as_object().unwrap().clone();
        assert!(apply_rules(&obj));

        // is_demo_user=false（我们不是 demo）→ 允许
        let obj = serde_json::json!({
            "value": "-X",
            "rules": [{"action": "allow", "features": {"is_demo_user": false}}]
        });
        let obj = obj.as_object().unwrap().clone();
        assert!(apply_rules(&obj));

        // 要求 is_demo_user=true → 不允许
        let obj = serde_json::json!({
            "value": "-X",
            "rules": [{"action": "allow", "features": {"is_demo_user": true}}]
        });
        let obj = obj.as_object().unwrap().clone();
        assert!(!apply_rules(&obj));
    }

    // ── deduplicate_jvm_args ──

    #[test]
    fn dedup_xmx_keeps_last() {
        let args = vec!["-Xmx1G".to_string(), "-Xmx2G".to_string()];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out, vec!["-Xmx2G"]);
    }

    #[test]
    fn dedup_protected_cp_keeps_first() {
        let args = vec![
            "-cp".to_string(), "a".to_string(),
            "-cp".to_string(), "b".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out, vec!["-cp", "a"]);
    }

    #[test]
    fn dedup_d_property() {
        let args = vec![
            "-Dfoo=1".to_string(),
            "-Dfoo=2".to_string(),
            "-Dbar=1".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out, vec!["-Dfoo=2", "-Dbar=1"]);
    }

    #[test]
    fn dedup_module_path_protected() {
        let args = vec![
            "-p".to_string(), "path1".to_string(),
            "-p".to_string(), "path2".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out, vec!["-p", "path1"]);
    }

    #[test]
    fn dedup_add_opens_all_kept() {
        // 模拟 forge/neoforge JSON: 多条 --add-opens/--add-exports 必须全部保留（每条授权不同包）
        let args = vec![
            "--add-opens".to_string(), "java.base/java.util.jar=cpw.mods.securejarhandler".to_string(),
            "--add-opens".to_string(), "java.base/java.lang.invoke=cpw.mods.securejarhandler".to_string(),
            "--add-exports".to_string(), "java.base/sun.security.util=cpw.mods.securejarhandler".to_string(),
            "--add-exports".to_string(), "jdk.naming.dns/com.sun.jndi.dns=java.naming".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out.len(), 8, "add-opens/exports 不应被去重: {:?}", out);
        // 键值配对: 偶数位都是选项名
        for i in (0..out.len()).step_by(2) {
            assert!(
                out[i] == "--add-opens" || out[i] == "--add-exports",
                "位置 {} 应为选项名: {:?}", i, out
            );
        }
    }

    #[test]
    fn dedup_add_opens_preserves_pairing() {
        // 回归: --add-opens 被当单参数去重会导致值裸奔（真实启动失败根因）
        let args = vec![
            "-p".to_string(), "modulepath".to_string(),
            "--add-modules".to_string(), "ALL-MODULE-PATH".to_string(),
            "--add-opens".to_string(), "java.base/java.util.jar=cpw.mods.securejarhandler".to_string(),
            "--add-opens".to_string(), "java.base/java.lang.invoke=cpw.mods.securejarhandler".to_string(),
            "-Xmx2G".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(
            out,
            vec![
                "-p", "modulepath",
                "--add-modules", "ALL-MODULE-PATH",
                "--add-opens", "java.base/java.util.jar=cpw.mods.securejarhandler",
                "--add-opens", "java.base/java.lang.invoke=cpw.mods.securejarhandler",
                "-Xmx2G",
            ]
        );
    }

    #[test]
    fn dedup_library_path_protected() {
        // 空格形式 + = 形式 → 同 key，保留最早
        let args = vec![
            "-Djava.library.path".to_string(), "p1".to_string(),
            "-Djava.library.path=p2".to_string(),
        ];
        let out = deduplicate_jvm_args(args);
        assert_eq!(out, vec!["-Djava.library.path", "p1"]);
    }

    // ── split_args_keep_quoting ──

    #[test]
    fn split_quoting() {
        let out = split_args_keep_quoting("-Xmx2G \"-Dfoo=hello world\" -Dbar=1");
        assert_eq!(out, vec!["-Xmx2G", "-Dfoo=hello world", "-Dbar=1"]);
    }

    // ── 版本隔离 ──

    #[test]
    fn parse_memory_formats() {
        // 回归: 前端滑块保存 "8192 MB"（带空格），旧解析残留尾部空格导致
        // parse 失败回退 2048（用户设置内存永远不生效）
        assert_eq!(parse_memory_mb("8192 MB"), 8192);
        assert_eq!(parse_memory_mb("4096"), 4096);
        assert_eq!(parse_memory_mb("8G"), 8192);
        assert_eq!(parse_memory_mb("8GB"), 8192);
        assert_eq!(parse_memory_mb("1.5G"), 1536);
        assert_eq!(parse_memory_mb("4 GB"), 4096);
        assert_eq!(parse_memory_mb(" 2048mb "), 2048);
        // 非法输入回退默认值
        assert_eq!(parse_memory_mb(""), 2048);
        assert_eq!(parse_memory_mb("abc"), 2048);
    }

    #[test]
    fn game_directory_isolation() {
        let ctx = test_ctx("1.21.4");
        assert_eq!(
            ctx.game_directory(),
            PathBuf::from("C:\\fake\\mc\\versions\\1.21.4")
        );
        let mut ctx2 = ctx.clone();
        ctx2.version_isolation = false;
        assert_eq!(ctx2.game_directory(), PathBuf::from("C:\\fake\\mc"));
    }

    // ── 主 JAR 重定向 ──

    #[test]
    fn primary_jar_self_when_exists() {
        // 无法创建真实文件时，fallback 自身路径
        let ctx = test_ctx("1.12.2-forge-14.23.5.2860");
        let v = fixture("forge-1.12.2-14.23.5.2860.json");
        let p = primary_jar_path(&ctx, &v);
        assert_eq!(
            p,
            PathBuf::from("C:\\fake\\mc\\versions\\1.12.2-forge-14.23.5.2860\\1.12.2-forge-14.23.5.2860.jar")
        );
    }

    #[test]
    fn primary_jar_inherits_from() {
        // 在临时目录创建父版本 jar，验证重定向
        let tmp = std::env::temp_dir().join("teas-launch-test-primary-jar");
        let _ = std::fs::remove_dir_all(&tmp);
        let parent_dir = tmp.join("versions").join("1.21.4");
        std::fs::create_dir_all(&parent_dir).unwrap();
        let parent_jar = parent_dir.join("1.21.4.jar");
        std::fs::write(&parent_jar, "fake jar").unwrap();

        let ctx = LaunchContext {
            dot_minecraft: tmp.clone(),
            instance_name: "fabric-loader-0.16.10-1.21.4".to_string(),
            auth: AuthInfo::offline("Steve"),
            java_path: PathBuf::from("java"),
            max_memory: 2048,
            jvm_args: String::new(),
            window_width: 854,
            window_height: 480,
            prefer_official: true,
            gc_mode: GcMode::Auto,
            version_isolation: true,
        };
        let v = fixture("fabric-1.21.4-0.16.10.json");
        let p = primary_jar_path(&ctx, &v);
        assert_eq!(p, parent_jar);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── natives 子目录 ──

    #[test]
    fn natives_subdir_detected() {
        let v = serde_json::json!({
            "arguments": {"jvm": [
                "-Djava.library.path=${natives_directory}/java",
                "-cp", "${classpath}"
            ]}
        });
        assert_eq!(natives_subdir(&v).as_deref(), Some("java"));
    }

    #[test]
    fn natives_subdir_none() {
        let v = serde_json::json!({
            "arguments": {"jvm": [
                "-Djava.library.path=${natives_directory}",
                "-cp", "${classpath}"
            ]}
        });
        assert_eq!(natives_subdir(&v), None);
    }

    #[test]
    fn natives_subdir_in_ruled_arg() {
        // 1.17+ 的 -Djava.library.path 在 windows rule 的 value 数组里
        let v = serde_json::json!({
            "arguments": {"jvm": [
                {"rules": [{"action": "allow", "os": {"name": "windows"}}],
                 "value": ["-Djava.library.path=${natives_directory}/java"]}
            ]}
        });
        assert_eq!(natives_subdir(&v).as_deref(), Some("java"));
    }

    #[test]
    fn natives_subdir_skips_unmatched_rules() {
        // rules 不匹配当前平台（如仅 linux 的 libpath）→ 不生效，不应重定向
        let v = serde_json::json!({
            "arguments": {"jvm": [
                {"rules": [{"action": "allow", "os": {"name": "linux"}}],
                 "value": ["-Djava.library.path=${natives_directory}/java"]}
            ]}
        });
        assert_eq!(natives_subdir(&v), None);
    }

    #[test]
    fn natives_subdir_first_libpath_wins() {
        // 第一条 libpath 无子目录 → 解压 base（即使后面有带子目录的）
        let v = serde_json::json!({
            "arguments": {"jvm": [
                "-Djava.library.path=${natives_directory}",
                "-Djava.library.path=${natives_directory}/java"
            ]}
        });
        assert_eq!(natives_subdir(&v), None);
    }

    // ── 退出分类 ──

    #[test]
    fn classify_exit_normal() {
        assert_eq!(classify_exit(0, &[]), "NORMAL");
    }

    #[test]
    fn classify_exit_jvm_error() {
        let logs = vec!["Error: Could not find or load main class net.minecraft.client.main.Main".to_string()];
        assert_eq!(classify_exit(1, &logs), "JVM_ERROR");
    }

    #[test]
    fn classify_exit_jvm_error_chinese() {
        // 中文 Windows 的 JVM 本地化错误消息
        let logs = vec![
            "错误: 找不到或无法加载主类 java.base/java.util.jar=cpw.mods.securejarhandler".to_string(),
        ];
        assert_eq!(classify_exit(1, &logs), "JVM_ERROR");
        let logs2 = vec!["错误: 无法创建 Java 虚拟机。".to_string()];
        assert_eq!(classify_exit(1, &logs2), "JVM_ERROR");
    }

    #[test]
    fn classify_exit_crash() {
        let logs = vec!["---- Minecraft Crash Report ----".to_string()];
        assert_eq!(classify_exit(-1, &logs), "CRASH");
    }

    #[test]
    fn classify_exit_error() {
        assert_eq!(classify_exit(1, &[]), "ERROR");
    }

    // ── 参数构建集成（四大版本）──

    /// 断言所有模块授权参数值（java.base/...=...）都紧跟其 --add-opens/--add-exports 选项名
    /// （回归: --add-opens 被错误去重后值裸奔导致 JVM "Unrecognized option" 失败）
    fn assert_module_grants_paired(args: &[String]) {
        let values = [
            "java.base/java.util.jar=cpw.mods.securejarhandler",
            "java.base/java.lang.invoke=cpw.mods.securejarhandler",
            "java.base/sun.security.util=cpw.mods.securejarhandler",
            "jdk.naming.dns/com.sun.jndi.dns=java.naming",
        ];
        for v in values {
            if let Some(pos) = args.iter().position(|a| a == v) {
                assert!(
                    args[pos - 1] == "--add-opens" || args[pos - 1] == "--add-exports",
                    "授权参数 {} 前是 {:?}（应为 --add-opens/--add-exports）",
                    v, args[pos - 1]
                );
            }
        }
    }

    fn build_args(name: &str) -> Vec<String> {
        let ctx = test_ctx(name);
        let v = fixture(&format!("{}.json", name));
        let natives = PathBuf::from("C:\\fake\\mc\\versions").join(name).join(format!("{}-natives", name));
        build_flat_args_inner(&ctx, &v, &natives, 21, true)
    }

    #[test]
    fn vanilla_1_21_4_full_args() {
        let args = build_args("vanilla-1.21.4");
        // java exe
        assert_eq!(args[0], "C:\\fake\\java\\bin\\java.exe");
        // main class
        assert!(args.contains(&"net.minecraft.client.main.Main".to_string()));
        // JSON 自带 -Djava.library.path 与 -cp（保留，不重复补充）
        let libpath_count = args.iter().filter(|a| a.starts_with("-Djava.library.path")).count();
        assert_eq!(libpath_count, 1);
        // 游戏参数
        assert!(args.iter().any(|a| a == "--username"));
        let uname_pos = args.iter().position(|a| a == "--username").unwrap();
        assert_eq!(args[uname_pos + 1], "Steve");
        // 宽高
        assert!(args.iter().any(|a| a == "--width"));
        // 占位符全部替换: 不应残留 ${...}
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
        // 无重复 -Xmx
        let xmx: Vec<&String> = args.iter().filter(|a| a.starts_with("-Xmx")).collect();
        assert_eq!(xmx.len(), 1);
    }

    #[test]
    fn forge_1_12_2_launchwrapper() {
        let args = build_args("forge-1.12.2-14.23.5.2860");
        // main class = launchwrapper.Launch
        assert!(args.contains(&"net.minecraft.launchwrapper.Launch".to_string()));
        // 旧版无 arguments.jvm → 自动补 -cp 与 -Djava.library.path
        assert!(args.iter().any(|a| a == "-cp"));
        assert!(args.iter().any(|a| a.starts_with("-Djava.library.path")));
        // minecraftArguments 解析出游戏参数
        assert!(args.iter().any(|a| a == "--username"));
        // forge universal jar 在 classpath
        let cp_pos = args.iter().position(|a| a == "-cp").unwrap();
        let cp = &args[cp_pos + 1];
        assert!(cp.contains("forge-1.12.2-14.23.5.2860"), "classpath 缺少 forge universal jar: {}", cp);
        // 无残留占位符
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }

    #[test]
    fn forge_1_16_5_modlauncher() {
        let args = build_args("forge-1.16.5-36.2.42");
        // main class = cpw.mods.modlauncher.Launcher
        assert!(args.contains(&"cpw.mods.modlauncher.Launcher".to_string()));
        // --launchTarget fmlclient（来自 JSON）
        let lt_pos = args.iter().position(|a| a == "--launchTarget").unwrap();
        assert_eq!(args[lt_pos + 1], "fmlclient");
        // --fml.mcVersion 1.16.5
        let pos = args.iter().position(|a| a == "--fml.mcVersion").unwrap();
        assert_eq!(args[pos + 1], "1.16.5");
        // 无 jvm arguments → 补 -cp / -Djava.library.path
        assert!(args.iter().any(|a| a == "-cp"));
        // 无残留占位符
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }

    #[test]
    fn forge_1_20_1_bootstraplauncher() {
        let args = build_args("forge-1.20.1-47.3.0");
        // main class = BootstrapLauncher
        assert!(args.contains(&"cpw.mods.bootstraplauncher.BootstrapLauncher".to_string()));
        // -p module path 保留（子 JSON 差异式，无 -cp → 自动补）
        let p_pos = args.iter().position(|a| a == "-p").unwrap();
        assert!(args[p_pos + 1].contains("bootstraplauncher-1.1.2.jar"), "module path 内容错误: {}", args[p_pos + 1]);
        // 自动补 -cp（子 JSON 无）
        assert!(args.iter().any(|a| a == "-cp"));
        // --launchTarget forgeclient
        let lt_pos = args.iter().position(|a| a == "--launchTarget").unwrap();
        assert_eq!(args[lt_pos + 1], "forgeclient");
        // --add-modules ALL-MODULE-PATH
        let am_pos = args.iter().position(|a| a == "--add-modules").unwrap();
        assert_eq!(args[am_pos + 1], "ALL-MODULE-PATH");
        // 模块授权参数键值配对（回归: --add-opens/--add-exports 去重错位）
        assert_module_grants_paired(&args);
        // ${classpath_separator} 已替换为 ;
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }

    #[test]
    fn neoforge_21_1_bootstraplauncher() {
        let args = build_args("neoforge-21.1.60");
        assert!(args.contains(&"cpw.mods.bootstraplauncher.BootstrapLauncher".to_string()));
        // -p 指向 securejarhandler（NeoForge 的 module path 首位）
        let p_pos = args.iter().position(|a| a == "-p").unwrap();
        assert!(args[p_pos + 1].contains("securejarhandler-3.0.8.jar"), "module path 错误: {}", args[p_pos + 1]);
        // --launchTarget（NeoForge 用 forgeclient）
        let lt_pos = args.iter().position(|a| a == "--launchTarget").unwrap();
        assert_eq!(args[lt_pos + 1], "forgeclient");
        // 游戏参数 --fml.neoForgeVersion 或 --fml.forgeVersion
        let has_fml = args.iter().any(|a| a == "--fml.forgeVersion" || a == "--fml.neoForgeVersion");
        assert!(has_fml, "缺少 --fml.* 版本参数");
        // 模块授权参数键值配对（回归）
        assert_module_grants_paired(&args);
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }

    #[test]
    fn fabric_1_21_4_knot() {
        let args = build_args("fabric-1.21.4-0.16.10");
        // main class = KnotClient
        assert!(args.contains(&"net.fabricmc.loader.impl.launch.knot.KnotClient".to_string()));
        // 子 JSON 只有 -DFabricMcEmu，无 -cp → 自动补
        assert!(args.iter().any(|a| a.starts_with("-DFabricMcEmu")));
        assert!(args.iter().any(|a| a == "-cp"));
        assert!(args.iter().any(|a| a.starts_with("-Djava.library.path")));
        for a in &args {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }

    // ── classpath 构建 ──

    /// 断言 classpath 中的库路径均为单层 libraries（回归: mclib_get 传错目录导致
    /// 路径多一层 libraries\，所有 jar 指向不存在的文件 → bootstraplauncher 找不到 modlauncher）
    /// 主 JAR 位于 versions/ 下，不参与检查
    fn assert_classpath_single_layered(cp: &[String]) {
        for p in cp {
            if !p.starts_with("C:\\fake\\mc\\libraries") {
                continue;
            }
            assert!(
                !p.contains("libraries\\libraries"),
                "库路径不应含双层 libraries: {}", p
            );
        }
    }

    #[test]
    fn classpath_contains_forge_libs_and_primary() {
        let ctx = test_ctx("1.12.2-forge-14.23.5.2860");
        let v = fixture("forge-1.12.2-14.23.5.2860.json");
        let cp = build_classpath(&ctx, &v);
        assert_classpath_single_layered(&cp);
        // 主 JAR 在最后（自身路径，不存在时 fallback）
        let last = cp.last().unwrap();
        assert!(last.ends_with("1.12.2-forge-14.23.5.2860.jar"), "主 JAR 不在最后: {}", last);
        // 反斜杠路径（Windows）
        for p in &cp {
            assert!(!p.contains('/'), "路径含正斜杠: {}", p);
        }
        // forge 库在列
        assert!(cp.iter().any(|p| p.contains("forge-1.12.2-14.23.5.2860")));
        // 非 Windows natives 应被跳过（如 natives-linux）
        assert!(!cp.iter().any(|p| p.contains("natives-linux")), "非 Windows natives 混入 classpath");
    }

    #[test]
    fn classpath_lwjgl3_natives_windows() {
        let ctx = test_ctx("vanilla-1.21.4");
        let v = fixture("vanilla-1.21.4.json");
        let cp = build_classpath(&ctx, &v);
        assert_classpath_single_layered(&cp);
        // LWJGL 3.x natives-windows jar 应加入 classpath（类分布在其中）
        assert!(cp.iter().any(|p| p.contains("natives-windows")), "LWJGL natives 缺失");
        // 主 JAR 最后
        assert!(cp.last().unwrap().ends_with("vanilla-1.21.4.jar"));
    }

    #[test]
    fn classpath_modlauncher_path_single_layer() {
        // 回归（NeoForge 整合包启动失败根因）: cpw.mods:modlauncher 的 classpath 路径
        // 必须是 .minecraft\libraries\cpw\mods\modlauncher\...（单层）
        let ctx = test_ctx("neoforge-21.1.60");
        let v = fixture("neoforge-21.1.60.json");
        let cp = build_classpath(&ctx, &v);
        assert_classpath_single_layered(&cp);
        let ml = cp.iter().find(|p| p.contains("modlauncher"))
            .expect("classpath 缺少 modlauncher");
        // 期望: .minecraft\libraries\cpw\mods\modlauncher\<ver>\modlauncher-<ver>.jar
        let expect = format!("C:\\fake\\mc\\libraries\\cpw\\mods\\modlauncher\\11.0.4@jar\\modlauncher-11.0.4@jar.jar");
        assert_eq!(
            ml,
            &expect,
            "modlauncher 路径错误（双层 libraries 会导致 bootstraplauncher 找不到 Consumer 服务）: {}", ml
        );
    }

    #[test]
    fn classpath_deduplicates_duplicate_libraries() {
        // 回归（Duplicate key 崩溃根因）: HMCL 等生成的版本 JSON 含重复 libraries 条目
        // （gson/guava/log4j 等出现两次），classpath 必须去重——
        // 否则 bootstraplauncher UnionFileSystem 构建模块时抛 "Duplicate key ...jar"
        let ctx = test_ctx("neoforge-21.1.60");
        let mut v = fixture("neoforge-21.1.60.json");
        // 注入重复条目（模拟 HMCL 合并产生的重复）
        let gson = serde_json::json!({
            "name": "com.google.code.gson:gson:2.10.1",
            "downloads": {"artifact": {"path": "com/google/code/gson/gson/2.10.1/gson-2.10.1.jar"}}
        });
        if let Some(libs) = v["libraries"].as_array_mut() {
            libs.push(gson);
        }
        let cp = build_classpath(&ctx, &v);
        // gson 路径只出现一次
        let gson_count = cp.iter().filter(|p| p.ends_with("gson-2.10.1.jar")).count();
        assert_eq!(gson_count, 1, "重复库未去重: {:?}", cp);
        // 整体无重复
        let mut seen = std::collections::HashSet::new();
        for p in &cp {
            assert!(seen.insert(p.clone()), "classpath 存在重复路径: {}", p);
        }
    }

    // ── 集成: resolve_version 合并（Fabric 差异式 JSON + 父 vanilla）──

    #[test]
    fn resolve_version_merges_jvm_args() {
        // 构造临时 .minecraft: versions/fabric-loader-.../ + versions/1.21.4/
        let tmp = std::env::temp_dir().join("teas-launch-test-resolve");
        let _ = std::fs::remove_dir_all(&tmp);

        let fabric_name = "fabric-loader-0.16.10-1.21.4";
        let fabric_dir = tmp.join("versions").join(fabric_name);
        std::fs::create_dir_all(&fabric_dir).unwrap();
        let fabric = fixture("fabric-1.21.4-0.16.10.json");
        std::fs::write(
            fabric_dir.join(format!("{}.json", fabric_name)),
            serde_json::to_string_pretty(&fabric).unwrap(),
        ).unwrap();

        let v_dir = tmp.join("versions").join("1.21.4");
        std::fs::create_dir_all(&v_dir).unwrap();
        let mut vanilla = fixture("vanilla-1.21.4.json");
        vanilla["id"] = serde_json::json!("1.21.4");
        std::fs::write(
            v_dir.join("1.21.4.json"),
            serde_json::to_string_pretty(&vanilla).unwrap(),
        ).unwrap();

        let merged = resolve_version(&tmp, fabric_name).expect("resolve_version 失败");
        let jvm = merged["arguments"]["jvm"].as_array().expect("合并后缺少 jvm 参数");
        let joined = serde_json::to_string(jvm).unwrap();

        // 子 JSON 的差异参数
        assert!(joined.contains("-DFabricMcEmu"), "子 jvm 参数缺失: {}", joined);
        // 父 JSON 的 -Djava.library.path 与 -cp（核心修复: 之前不合并 jvm）
        assert!(joined.contains("-Djava.library.path"), "父 jvm 参数缺失: {}", joined);
        assert!(joined.contains("\"-cp\""), "父 jvm 参数缺失 (-cp): {}", joined);
        // 父在前子在后
        let p_lib = joined.find("-Djava.library.path").unwrap();
        let p_emu = joined.find("-DFabricMcEmu").unwrap();
        assert!(p_lib < p_emu, "父 jvm 参数应在子参数之前");
        // mainClass 子优先
        assert_eq!(
            merged["mainClass"],
            serde_json::json!("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_version_cycle_detected() {
        // A inheritsFrom B, B inheritsFrom A → 应报错而非栈溢出
        let tmp = std::env::temp_dir().join("teas-launch-test-cycle");
        let _ = std::fs::remove_dir_all(&tmp);
        for name in ["A", "B"] {
            let dir = tmp.join("versions").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let parent = if name == "A" { "B" } else { "A" };
            let json = serde_json::json!({
                "id": name,
                "inheritsFrom": parent,
                "mainClass": "net.minecraft.client.main.Main"
            });
            std::fs::write(
                dir.join(format!("{}.json", name)),
                serde_json::to_string_pretty(&json).unwrap(),
            ).unwrap();
        }
        let err = resolve_version(&tmp, "A").expect_err("应检测到循环");
        assert!(err.contains("循环"), "错误信息不符: {}", err);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── 集成: build_jvm_args 合并场景（差异式 JSON + 父 JSON 合并）──

    #[test]
    fn jvm_args_merged_no_duplicate_cp() {
        // 模拟 resolve_version 合并后: 父 vanilla 提供 -cp/-Djava.library.path，子 JSON 提供差异参数
        let parent_jvm = serde_json::json!([
            "-Djava.library.path=${natives_directory}",
            "-cp", "${classpath}"
        ]);
        let child_jvm = serde_json::json!([
            "-DignoreList=bootstraplauncher",
            "-p", "${library_directory}/cpw/mods/bootstraplauncher/1.1.2/bootstraplauncher-1.1.2.jar"
        ]);
        let mut v = serde_json::json!({});
        v["arguments"] = serde_json::json!({"jvm": []});
        let mut merged: Vec<serde_json::Value> = Vec::new();
        for a in parent_jvm.as_array().unwrap() { merged.push(a.clone()); }
        for a in child_jvm.as_array().unwrap() { merged.push(a.clone()); }
        v["arguments"]["jvm"] = serde_json::Value::Array(merged);

        let ctx = test_ctx("1.20.1-forge-47.3.0");
        let natives = PathBuf::from("C:\\fake\\mc\\versions\\1.20.1-forge-47.3.0\\1.20.1-forge-47.3.0-natives");
        let map = build_placeholders(&ctx, &v, &natives);
        let jvm = build_jvm_args(&ctx, &v, &map, &natives, 21, true);

        // -cp 只出现一次（受保护，保留父 JSON 的）
        let cp_count = jvm.iter().filter(|a| a.as_str() == "-cp").count();
        assert_eq!(cp_count, 1, "重复 -cp: {:?}", jvm);
        // -p 保留
        assert!(jvm.iter().any(|a| a == "-p"));
        // -Djava.library.path 只一次
        let lp_count = jvm.iter().filter(|a| a.starts_with("-Djava.library.path")).count();
        assert_eq!(lp_count, 1);
        // 无残留占位符
        for a in &jvm {
            assert!(!a.contains("${"), "残留占位符: {}", a);
        }
    }
