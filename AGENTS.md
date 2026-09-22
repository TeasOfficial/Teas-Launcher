# Teas Launcher — 项目文档

## 项目概述

Rust + Tauri v2 + Vue 3 的 Minecraft 启动器，工业科幻仪表盘风格 UI。

支持 Forge / NeoForge / Fabric / Quilt / Cleanroom 五种加载器，Modrinth + CurseForge 双资源源，`.mrpack` 整合包安装。

## 技术栈

| 层 | 技术 |
|------|------|
| 桌面壳 | Tauri v2 (Rust) |
| 前端 | Vue 3 + TypeScript + Vite |
| 样式 | 全局 CSS (src/styles.css)，无 Tailwind |
| 包管理 | npm + Cargo workspace |
| 工具链 | `x86_64-pc-windows-msvc`（见 `.cargo/config.toml`） |
| Crate 类型 | `["lib", "staticlib"]` — 无 cdylib，由 `src-tauri/src/main.rs` 链接 staticlib 生成可执行文件 |

> 历史遗留：早期文档记载工具链为 `x86_64-pc-windows-gnu`，并据此说明"无 cdylib 是因为 GNU 工具链 DLL 导出符号限制"。当前实际构建走 MSVC，该理由已不成立，但 crate 类型保持不变。

## 启动命令

```bash
npx tauri dev       # 开发（Vite热重载 + Rust自动重编译）
npx tauri build     # 生产构建
npm run build       # 仅构建前端
cargo build         # 仅构建Rust

cargo test --lib    # 运行单元测试（45 个，主要在 launch.rs）
cargo check         # 快速类型检查
```

## 目录结构

```
mcstarter/
├── Cargo.toml                 # Rust workspace root
├── package.json               # 前端依赖
├── index.html                 # Vue 挂载点
├── vite.config.ts
├── .cargo/config.toml         # 工具链 target + USTC 镜像源
├── src/                       # 前端源码
│   ├── main.ts                # Vue createApp 入口
│   ├── App.vue                # 根组件（页面路由 + 全局状态）295 行
│   ├── styles.css             # 全局样式 745 行
│   ├── utils/
│   │   └── version.ts         # MC 版本号解析与比较（UI 侧唯一实现）
│   ├── composables/
│   │   └── useGameInstall.ts  # 游戏安装向导状态机（版本列表/加载器选择/提交安装）
│   └── components/
│       ├── Sidebar.vue        # 侧栏导航 + 网络状态 + 版本显示
│       ├── StatusBar.vue      # 顶部状态条（时钟 + 窗口控件）
│       ├── DashboardPage.vue  # 仪表盘主页（启动按钮 + 玩家身份 + 实例信息 + 遥测条）
│       ├── InstancesPage.vue  # 实例列表（搜索、删除、选择）
│       ├── DownloadsPage.vue  # 下载中心（搜索/详情/安装）648 行
│       ├── DownloadsTaskPage.vue # 下载任务管理（进度+历史）
│       ├── AccountsPage.vue   # 账户管理（Microsoft 入口为占位/离线/Authlib）
│       ├── SettingsPage.vue   # 设置（Java路径/内存/下载源/分辨率/线程数）691 行
│       └── DebugPage.vue      # Dev模式调试面板
└── src-tauri/                 # Tauri Rust 后端 (9126 行)
    ├── Cargo.toml
    ├── tauri.conf.json        # 1100x640 固定窗口
    ├── capabilities/default.json
    ├── tests/fixtures/        # 6 个真实版本 JSON，供 launch.rs 单元测试使用
    └── src/
        ├── main.rs            # 入口 → teas_launcher_lib::run()
        ├── lib.rs             # 模块声明 + BUILD 常量 + 42 个命令注册 (99 行)
        ├── utils.rs           # JAR 校验、目录移动、UUID、加载器检测
        ├── http.rs            # HTTP 客户端、代理、取消下载
        ├── config.rs          # tc.ini 配置读写、.teas 目录
        ├── download/          # 下载引擎（PCL-CE 移植）
        │   ├── model.rs       #   DownloadFile / FileChecker / DownloadState
        │   ├── source.rs      #   URL 镜像源转换（BMCLAPI / MCIMirror）
        │   └── engine.rs      #   多线程下载 + 多源回退 + 进度事件 (655 行)
        ├── version/           # 版本文件管理（PCL-CE 移植）
        │   ├── manifest.rs    #   各加载器版本清单获取 (445 行)
        │   ├── library.rs     #   Libraries 解析与下载 (552 行)
        │   └── assets.rs      #   资源索引与缺失资源检测
        ├── install/           # 加载器安装引擎（PCL-CE 移植）
        │   ├── vanilla.rs     #   原版安装 + 加载器分发入口
        │   ├── forge.rs       #   Forge installer.jar 解析 (353 行)
        │   ├── neoforge.rs    #   NeoForge
        │   ├── fabric.rs      #   Fabric（MergeJson 自包含 JSON）
        │   ├── quilt.rs       #   Quilt
        │   ├── cleanroom.rs   #   Cleanroom（1.12.2）
        │   └── merge.rs       #   libraries 去重合并 + 整合包依赖解析
        ├── instance.rs        # 实例同步、版本解析（inheritsFrom 递归）、列表/删除
        ├── launch/            # Minecraft 启动流程（原单文件 3060 行，已按职责拆分）
        │   ├── mod.rs         #   LaunchContext / AuthInfo / launch_instance / pre_check
        │   ├── natives.rs     #   Natives 解压、子目录重定向、孤儿文件清理
        │   ├── args.rs        #   启动参数构建（纯函数，单测覆盖的主战场）
        │   ├── process.rs     #   预启动处理（log4j2/options.txt）、进程启动与监控
        │   ├── java.rs        #   Java 版本探测、游戏版本比较
        │   └── tests.rs       #   43 个单元测试
        ├── crash.rs           # 崩溃检测（stderr/crash-reports/latest.log）
        ├── system.rs          # 系统命令（版本号、状态、网络、Java扫描、代理、内存）
        ├── modrinth.rs        # Modrinth API（搜索、详情、下载）
        ├── curseforge.rs      # CurseForge API（搜索、详情、文件、下载）
        └── modpack.rs         # 整合包安装（.mrpack 完整流程）671 行
```

## 关键设计决策

- **无边框窗口**: decorations=false, 自定义顶栏拖拽
- **固定尺寸**: 1100x640, resizable=false, maximizable=false
- **页面路由**: App.vue 中 v-if 切换页面组件，无 vue-router
- **状态共享**: provide/inject 传递 instances, accounts, download tasks
- **持久化**:
  - `%APPDATA%/TeasLauncher/tc.ini` → 用户设置+账户（多启动器共享）
  - `.teas/tc.ini` → 实例数据
  - `.teas/launcher.log` → 运行日志（tauri-plugin-log，单文件上限 5MB）
  - `.teas/last_stderr.log` → 游戏 stderr
- **Dev 模式**: Rust `cfg!(dev)` 控制，Minecraft 目录为 `D:\Minecraft\[000A]`
- **多线程下载**: tokio::spawn + Semaphore，默认 8 线程，设置可调 1-64
- **下载源镜像**: BMCLAPI (游戏), MCIMirror (Modrinth/CurseForge 资源)，默认启用
- **Git**: 仓库存在，主分支 `dev`

## Rust 后端命令清单

共 42 个命令，通过 `#[tauri::command]` 注册，全部集中在 `lib.rs`。

### 实例管理 — `instance.rs`
- `list_instances(mc_dir)` — 扫描 `versions/` 目录
- `delete_instance(mc_dir, instance_name)` — 删除实例目录
- `fix_instance(app, mc_dir, instance_name, source)` — 补全缺失的版本 JAR、资源索引、Libraries

### 启动 — `launch.rs`

`LaunchContext` → `pre_check` → `ensure_parent_version` → 补全 Libraries+Assets → `prepare_natives` → `build_flat_args` → `pre_run_setup` → `spawn_process` → `monitor_process`

- `launch_instance(...)` — 完整启动流程
- `kill_instance()` — taskkill /F /T /PID + 子进程树
- `is_instance_running()` — 检查 RUNNING_PID
- `get_launch_args(...)` — 调试用参数预览

核心约定：
1. **参数不拼接后分割** — `build_flat_args` 返回 `Vec<String>` 直接传给 `Command::args()`，消除引号丢失 bug
2. Classpath: OptiFine 插入到 `len-1`（倒数第二），主 JAR 最后
3. JVM 参数优先级 (HMCL-style): 用户覆盖→内存→Metaspace→编码→GC→安全→版本JSON→classpath→Main class→游戏参数
4. Natives: 非 ASCII 路径回退 (`%APPDATA%\.minecraft\bin\natives\` → ProgramData)
5. 参数去重: PCL-style（JVM 去重同 key，游戏参数覆盖同 key，除 `--tweakClass`）
6. 正确设置 `APPDATA` 环境变量（指向 `.minecraft` 目录本身，非父目录）
7. GC 策略: 可配置(G1GC/ZGC/自定义)，默认 Java 21+ ZGC、15-20 ZGC、8-14 G1GC
8. `resolve_version` 递归合并 `inheritsFrom` 父版本数据，带循环检测

### 下载 — `download/engine.rs`
- `download_files_parallel(app, files, max_threads)` — 批量并行下载，内部发进度事件
- `download_file(app, ...)` — 单文件下载（含多源回退）

### 安装 — `install/`
- `install::vanilla::install_game(...)` — 原版 + 加载器分发入口（唯一对外命令）
- 其余 `install/*` 为内部模块，由 `vanilla.rs` 与 `modpack.rs` 调用

### 版本清单 — `version/manifest.rs`
- `fetch_version_manifest` — Minecraft 版本清单 (Mojang + BMCLAPI + UVMC 合并)
- `fetch_forge_mc_versions` / `fetch_forge_versions`
- `fetch_neoforge_versions`
- `fetch_fabric_versions` / `fetch_quilt_versions`
- `fetch_cleanroom_versions`

### 崩溃检测 — `crash.rs`
- `check_crash(mc_dir, instance_name)` — 检查 stderr.log / crash-reports / latest.log
- 8 种关键词→原因匹配；日志中含 "Stopping!" 视为正常退出返回 None

### HTTP — `http.rs`
- `cancel_download()` — 设置取消标志

### 资源 API — `modrinth.rs` / `curseforge.rs`
- `search_bbsmc(...)` / `search_curseforge(...)` — 搜索
- `get_project` / `get_project_versions` / `get_curseforge_project` / `get_curseforge_files` — 详情
- `install_file` / `install_project` / `install_curseforge_file` — 下载安装

### 整合包 — `modpack.rs`
- `install_modpack(...)` — 下载 .mrpack → 解压 → 下载 Mod → 安装加载器 → 同步 overrides
- `install_local_modpack(...)` — 本地文件入口

### 系统 — `system.rs`
- `get_version` / `get_full_version` — 版本号（BUILD 常量拼入）
- `is_dev` — cfg!(dev)
- `get_system_stats` — CPU/MEM (sysinfo)
- `get_memory_info` / `calc_auto_memory` — 内存信息与自动分配建议
- `check_network` — Modrinth/CurseForge/Minecraft 连通性，返回 JSON {ok, total, failures}
- `test_proxy_connectivity` / `detect_system_proxy` — 代理检测（读 Windows 注册表）
- `scan_java` — 搜索 Java 安装，返回 Vec<{path, version}>
- `get_minecraft_dir` — Dev 返回 `D:\Minecraft\[000A]`，Release 返回 exe 目录
- `get_teas_dir` — 返回 `.teas` 目录路径

### 配置 — `config.rs`
- `config_read(scope)` — 读取 tc.ini 全部 JSON
- `config_write(scope, key, value)` — 合并写入 key-value
- scope: `"launcher"` → `.teas/tc.ini`, `"user"` → `%APPDATA%/TeasLauncher/tc.ini`

## 前端全局状态 (App.vue provide)

| Key | 类型 | 说明 |
|-----|------|------|
| `instances` | `Ref<Instance[]>` | 实例列表 |
| `activeInstance` | `ComputedRef<Instance>` | 当前活跃实例 |
| `accounts` | `Ref<Account[]>` | 账户列表 |
| `activeAccount` | `ComputedRef<Account>` | 当前活跃账户（模板中自动解包，script 中用 `.value`） |
| `setActiveAccount(index)` | function | 切换活跃账户并保存 |
| `isLaunching` | `Ref<boolean>` | 启动流程进行中 |
| `isRunning` | `Ref<boolean>` | 游戏进程运行中 |
| `dlTasks` | `Ref<DownloadTask[]>` | 下载中任务 |
| `dlHistory` | `Ref<DownloadTask[]>` | 下载历史（会话内） |
| `addDlTask(name) → id` | function | 创建下载任务 |
| `updateDlTask(id, status)` | function | 更新任务状态 |
| `finishDlTask(id, error?)` | function | 完成任务→移入历史 |

## 下载任务生命周期

1. `addDlTask("xxx.jar")` → dlTasks.push({id, name, status:"下载中..."})
2. 下载进行中 → 进度事件更新 status 字段
3. 成功: `updateDlTask(id, "已完成")` → `finishDlTask(id)` → 移入 dlHistory
4. 失败: `updateDlTask(id, "失败")` → `finishDlTask(id, error)` → 移入 dlHistory
5. 取消: 用户点击 ✕ → `cancel_download()` → `finishDlTask(id)` → 移入 dlHistory

## 下载进度事件

Rust 端发射 `download-progress` 事件：

```json
{
  "batch_id": 1,            // 批量任务唯一ID，前端按此聚合
  "batch_total": 50,        // 批量任务总文件数
  "batch_done": 5,          // 已完成文件数
  "file_name": "xxx.jar",   // 当前文件名
  "file_percent": 45,       // 当前文件进度 0-100
  "file_speed": 1048576,    // 当前文件速度 B/s
  "file_downloaded": 65536, // 当前文件已下载字节
  "file_total": 131072,     // 当前文件总字节
  "total_bytes": 10000000,  // 批量任务总字节
  "total_downloaded": 5000000, // 批量任务已下载总字节
  "aggregated_percent": 50, // 聚合进度 (不会横跳的关键)
  "step": "下载中 45%"
}
```

前端 `DownloadsTaskPage.vue` 处理：
- 按 `batch_id` 聚合进度 → 进度条平滑递增不横跳
- 顶栏显示总下载速度（所有活跃文件速度之和）
- 详细信息面板显示每个文件的迷你进度条 + 速度 + 大小

## 测试

- `cargo test --lib` — 45 个单元测试，全部通过
- 主要覆盖 `launch.rs` 的**参数构建**（43 个）与 `download/source.rs` 的镜像转换（2 个）
- `src-tauri/tests/fixtures/` 存放 6 个真实版本 JSON：vanilla 1.21.4、fabric 1.21.4、forge 1.12.2 / 1.16.5 / 1.20.1、neoforge 21.1.60
- 测试通过 `env!("CARGO_MANIFEST_DIR")` 定位 fixtures，覆盖：inheritsFrom 合并、classpath 排序与去重、OS 规则过滤、参数替换、循环引用检测

## CSS 设计系统

```css
--bg-deep: #0A0A0A    --bg-mid: #121212    --bg-panel: #161618
--bg-raised: #1C1C1E  --border: #252528    --border-lit: #333338
--accent: #C8FF3D     --accent-dim: rgba(200,255,61,.12)
--secondary: #FFFFFF  --text: #D0D0D4      --text-dim: #6A6A72
--warn: #FF8A3D       --danger: #FF4D4D
```

字体: Inter/Bahnschrift/DIN (sans), JetBrains Mono/Cascadia Code/Consolas (mono), Orbitron/Eurostile (tech)

关键 CSS 类:
- `.bl-zh` / `.bl-en` → 双语（中文在上，英文在下）
- `.panel` + `.panel-tech-corner` → 方块面板（左上角切角装饰）
- `.modal-overlay` + `.modal-panel` → 弹窗 (position: absolute，不遮挡状态栏)
- `.page` → 页面容器，position: relative，overflow-y: auto
- `.custom-select` → 自定义下拉选择器
- Vue Transition: page, modal, dl-view, tab-view, drop, acct-list

## 版本号规则

- Cargo semver: `0.1.1`
- BUILD 常量: 每次修改核心逻辑后 +1（当前 75，位于 `lib.rs`）
- `get_full_version()` 返回: `v0.1.1.0075`
- 注：`src-tauri/tauri.conf.json` 的 `version` 仍为 `0.1.0`，与 Cargo.toml 未同步

## 开发约定

- 前端修改无需重启，Vite 热重载
- Rust 修改后 Tauri dev 自动重编译
- 端口 1420 (Vite)，被占用时需手动杀进程
- 后端模块按职责分层：`download/` 管传输，`version/` 管版本文件，`install/` 管加载器安装
- 新增加载器时，安装逻辑放 `install/`，版本清单获取放 `version/manifest.rs`，两处都要在 `lib.rs` 注册
- **日志一律用 `log::` 宏**（`error!` 失败 / `warn!` 可恢复问题 / `info!` 关键流程 / `debug!` 逐文件细节），
  由 tauri-plugin-log 写入 `.teas/launcher.log`。Release 构建没有控制台，用 `println!` 输出等于丢弃
- loader 安装的共享步骤（取原版 JSON、合并 libraries、转自包含 JSON、下载 libs+JAR）统一走
  `install/merge.rs`，不要在各 loader 里重写

## 已知问题（未修复）

6. **`download/engine.rs` 与 `install/vanilla.rs` 各自解析 Mojang 版本清单** — 两处都是
   "从 manifest 找版本 URL" 的相同逻辑，可抽成 `resolve_version_url(client, mc_ver)`。
7. **`list_instances` 依赖 `<name>.json` 判定实例有效性** — 缺该文件的目录会被跳过（它们是
   残留的游戏目录，启动必然失败）。若将来支持无 JSON 的实例类型需同步改这里。
8. **"地图"分类映射到 Modrinth 的 `categories:worldgen`** — Modrinth 没有 `map` 这个
   project_type（实测返回 0 条），`modrinth.rs::search_bbsmc` 里做了显式映射。这是近似，
   不等于真正的"地图"分类。
9. **游戏 stdout/stderr 只在 debug 级写入启动器日志** — dev 构建能看全，release 只保留
   Info 级（不再把三千行游戏输出写进 launcher.log）。崩溃诊断改看两处：游戏自己的
   `logs/latest.log`，以及启动器退出时落盘的 `.teas/last_stderr.log`。
10. **`last_stderr.log` 仅在一次启动结束时写入** — 若启动器自身被强杀，该文件仍是上一次的内容。

## 已修复的历史问题（避免回退）

- **`instance_index.json` 只存扫盘得到的字段**（`name`/`version`/`mods`/`loaderName`/`_mtime`）。
  `lastPlayed`/`active`/`loader` 是派生值，存进缓存会随缓存僵化（相对时间每天变、active 随
  用户选择变、loader 曾是反的）。三个字段一律在 `list_instances` 返回时即时计算。
- **`${path}` 占位符不再全局映射** — 它只用于 `logging.client.argument`，官方语义是"日志配置
  文件路径"。`build_placeholders` 故意不含它，由 `build_jvm_args` 指向版本目录下的 `log4j2.xml`。
- **下载源参数必须过 `src/utils/source.ts::gameSourceParam`** — 后端判据是 `source == "Mojang"`，
  而配置里存的是 `"官方源"`。直接把配置值传过去（或写死 "MCIMirror"）会导致用户选了官方源
  仍走镜像。曾出现在 `DashboardPage.launch()` 与 `useGameInstall.doInstallGame()` 两处。
- **用户主动停止不再算崩溃** — `kill_instance` 置 `launch::USER_KILLED`，`monitor_process` 据此
  上报 `exitType: "STOPPED"` 并跳过崩溃检测；`crash::check_crash` 也会直接返回 None
  （taskkill 的退出码 1 与真崩溃无法从退出码区分）。
- **日志 target 需先 `clear_targets()`** — `tauri_plugin_log::Builder::new()` 自带 Stdout +
  系统日志目录两个默认 target，叠加自定义 target 会让 dev 每行打印两次、release 双写两个文件。

## 已知问题

1. **Microsoft 正版登录未实现** — `AccountsPage.vue` 的入口已如实标注为不可用（按钮 disabled），
   未接入 OAuth 流程。接入时替换 `AuthInfo` 的 access_token/uuid 并放开该按钮即可。
2. **整合包安装后可能缺版本文件** — Forge/Fabric/NeoForge 整合包安装后需自动下载对应加载器的版本 JSON 和 Libraries。
3. **原版游戏下载** — 下载中心的"游戏下载"入口尚未接入实际下载流程。
4. **`FileChecker` 的 JSON 校验分支未启用** — `is_json` 只会由 `with_json()` 置为 true，而该方法当前无调用方。
5. **错误类型仍为 `String`** — 全后端 63 处 `Result<_, String>`、100 处 `map_err(|e| e.to_string())`。
   内部错误类型信息在传递中被压平成字符串，前端无法按类别处理。若要引入 `thiserror`，
   需同时改造数百处 `Err("...".to_string())` 构造点，属于独立的重构任务。
