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

cargo test --lib    # 运行单元测试（52 个，主要在 launch/tests.rs 与 instance.rs）
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
│   │   ├── version.ts         # MC 版本号解析与比较（UI 侧唯一实现）
│   │   ├── config.ts          # tc.ini 读写（readUserConfig/writeUserConfig…）
│   │   ├── instanceSettings.ts # 实例覆盖项 + 全局配置的合并逻辑
│   │   └── source.ts          # 下载源参数映射（gameSourceParam）
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
        ├── progress.rs        # download-progress 事件统一发射（唯一出口）
        ├── config.rs          # tc.ini 配置读写、.teas 目录
        ├── download/          # 下载引擎（PCL-CE 移植）
        │   ├── model.rs       #   DownloadFile / FileChecker / DownloadState
        │   ├── source.rs      #   URL 镜像源转换（仅 BMCLAPI）
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
- **下载源**: 默认**官方源**；游戏可切 BMCLAPI 镜像（设置 → 高级）。Mod / 整合包固定官方源——
  **MCIMirror 已停止运营**（2025），相关分支与 `mod_source` 配置已移除
- **Git**: 仓库存在，主分支 `dev`

## 密钥管理（CurseForge API Key）

- **读取优先级**：环境变量 `TEAS_CURSEFORGE_API_KEY` → `%APPDATA%\TeasLauncher\curseforge.key`
- **写入工具**：`scripts/set-cf-key.ps1`（`Read-Host -AsSecureString`，不回显、不进 shell 历史；
  写完用 `icacls /inheritance:r` 去掉继承，只留当前用户）
- **禁止事项**：硬编码进源码、写进 `log::` 输出、拼进错误串、作为命令返回值回传前端、提交进仓库
  （`.gitignore` 已排除 `*.key`）
- **前端只能拿到状态**：`curseforge_key_status` 返回 `{configured, source, file}`，不含 key 本身
- **发布版内置、源码不内置**：Release 打包时把 key 注入构建产物（构建环节注入，不进仓库），
  用户开箱即用；源码/开发版一律走环境变量或本地 key 文件。**仓库里永远只有占位与工具脚本**。
  注意：二进制里内置的 key 是可被提取的，自带 key 的构建产物不要对外分发（README 已写明）。
- **历史包袱**：`curseforge.rs` 与 `modpack.rs` 曾硬编码 PCL 的公共 key，已移除；
  没有 key 时官方接口必然 403，代码只走镜像并给出可操作提示

## 旧版本 legacy 资源（pre-1.6）

Beta / Alpha 时代的版本启动时会去拉 `http://s3.amazonaws.com/MinecraftResources/`，
那个桶早已下线 → 日志里 `FileNotFoundException`，结果是没有声音、没有 lang。
官方启动器的做法：`assets` 索引里 `map_to_resources: true` 时，把对象**摊平**复制到
`<gameDir>/resources/`，游戏直接读本地文件。

- 一次性抓取脚本：`scripts/fetch-legacy-assets.mjs`
  ```bash
  node scripts/fetch-legacy-assets.mjs "<.minecraft>/versions/b1.6.4/b1.6.4.json"
  # 可选：--out <dir>  --jobs 8  --mirror（优先 BMCLAPI）
  ```
  它会读版本 JSON 的 `assetIndex.url` → 下载索引 → 并发下载 749 个对象（约 47 MB）→ 摊平写入
  `versions/<name>/resources/`。已存在且大小一致的文件会跳过。
- 注意：**游戏仍会在日志里报一次 S3 404**，那是游戏自身代码里的死 URL，无法消除，
  但资源已经在本地，声音正常。
- **PowerShell 坑**：路径里的 `[000A]` 会被当通配符，`Test-Path`/`Get-ChildItem` 必须用
  `-LiteralPath`，否则会误报"目录不存在"（本轮就踩到了）。

## Rust 后端命令清单

共 46 个命令，通过 `#[tauri::command]` 注册，全部集中在 `lib.rs`。

### 实例管理 — `instance.rs`
- `list_instances(mc_dir)` — 扫描 `versions/` 目录
- `delete_instance(mc_dir, instance_name)` — 删除实例目录
- `open_instance_folder(mc_dir, instance_name)` — 在资源管理器打开实例目录（版本隔离下是 versions/<name>）
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
- `install_file` / `install_curseforge_file` — 下载安装（`install_project` 因只取 `versions[0].files[0]`、忽略版本与加载器已被删除）
- `curseforge_key_status` / `curseforge_save_key` / `curseforge_clear_key` / `curseforge_test_key` — API Key 状态与本地存储（不回传 key）

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

**所有发射点必须走 `progress.rs`**（`emit_stage` / `emit_file`），不要在别处直接
`app.emit("download-progress", ...)`。历史上 install/、modpack、modrinth、curseforge
各发过一套 `{filename, percent}` 旧格式，与 engine 的 `{file_name, batch_id}` 并存，
前端按新格式解析时 `file_name` 为 undefined 会抛 TypeError，整个监听器断掉、
安装过程完全没有进度。

`kind` 区分两类事件：`stage`（准备/解压/装加载器，只有 `step` + 总百分比）、
`file`（逐文件进度，前端聚合到任务卡）。

Rust 端发射 `download-progress` 事件：

```json
{
  "kind": "file",           // file | stage
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
  "total_speed": 4194304,   // 后台按"活跃文件"求和，前端不再自己累加
  "batch_running": 8,       // 当前真正在传输的文件数（任务卡显示"N 路并发"）
  "step": "下载中 45%"
}
```

前端 `DownloadsTaskPage.vue` 处理：
- 按 `batch_id` 聚合进度 → 进度条平滑递增不横跳
- `stage` 事件只更新百分比与 `step` 文案，不写入 per-file 列表
- 顶栏显示总下载速度（所有活跃文件速度之和）
- 详细信息面板显示每个文件的迷你进度条 + 速度 + 大小
- 对旧字段 `filename` / `percent` 仍做兜底（防御未迁移的发射点）

## 测试

- `cargo test --lib` — 52 个单元测试
- 主要覆盖 `launch/tests.rs` 的**参数构建**（43 个）、`instance.rs` 的版本解析（7 个）与 `download/source.rs` 的镜像转换（2 个）
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
- BUILD 常量: 每次修改核心逻辑后 +1（当前 100，位于 `lib.rs`）
- `get_full_version()` 返回: `v0.1.1.0100`
- 注：`src-tauri/tauri.conf.json` 的 `version` 仍为 `0.1.0`，与 Cargo.toml 未同步

## 开发约定

- 前端修改无需重启，Vite 热重载。**大改 SFC 后如果界面行为和代码对不上**，多半是 HMR 只换了模板没换脚本：
  重写一次 `index.html`（内容不变即可）触发整页刷新，或重启 `tauri dev`
- Rust 修改后 Tauri dev 自动重编译
- 端口 1420 (Vite)，被占用时需手动杀进程
- 后端模块按职责分层：`download/` 管传输，`version/` 管版本文件，`install/` 管加载器安装
- 新增加载器时，安装逻辑放 `install/`，版本清单获取放 `version/manifest.rs`，两处都要在 `lib.rs` 注册
- **日志一律用 `log::` 宏**（`error!` 失败 / `warn!` 可恢复问题 / `info!` 关键流程 / `debug!` 逐文件细节），
  由 tauri-plugin-log 写入 `.teas/launcher.log`。Release 构建没有控制台，用 `println!` 输出等于丢弃
- loader 安装的共享步骤（取原版 JSON、合并 libraries、转自包含 JSON、下载 libs+JAR）统一走
  `install/merge.rs`，不要在各 loader 里重写

## 已知问题（未修复）

7. **`list_instances` 依赖 `<name>.json` 判定实例有效性** — 缺该文件的目录会被跳过（它们是
   残留的游戏目录，启动必然失败）。若将来支持无 JSON 的实例类型需同步改这里。
8. **"地图"分类映射到 Modrinth 的 `categories:worldgen`** — Modrinth 没有 `map` 这个
   project_type（实测返回 0 条），`modrinth.rs::search_bbsmc` 里做了显式映射。这是近似，
   不等于真正的"地图"分类。
9. **游戏 stdout/stderr 只在 debug 级写入启动器日志** — dev 构建能看全，release 只保留
   Info 级（不再把三千行游戏输出写进 launcher.log）。崩溃诊断改看两处：游戏自己的
   `logs/latest.log`，以及启动器退出时落盘的 `.teas/last_stderr.log`。
10. **`last_stderr.log` 仅在一次启动结束时写入** — 若启动器自身被强杀，该文件仍是上一次的内容。
11. **以管理员身份运行时，系统会拦截拖放与部分合成输入** — Windows UIPI 不允许跨完整性级别投递：
    资源管理器（Medium）无法把文件拖进以管理员（High）运行的启动器，光标显示禁止符；
    同理外部工具也无法向该窗口注入键盘事件。拖放安装因此提供了不依赖拖放的入口：
    获取内容 → 整合包 → 「本地整合包」按钮。要以拖放方式安装，请用**非管理员**终端启动 `npx tauri dev`。

12. **工作区目录被打上 Low 完整性标签会让整个 app 失效** — 若 `D:\MCStarter\Teas-Launcher`（或其 `target/`）
    被设置了 `Mandatory Label\Low Mandatory Level`，从该目录启动的 exe 会以 **Low** 令牌运行，
    表现为：`WebView2 0x8000FFFF 灾难性故障`、`无法确定数据目录: 拒绝访问 (os error 5)`、
    `日志目录不可写`、以及拖放被系统拒绝。检查与修复（命令需在**已打开**的 shell 里由 owner 执行）：
    `icacls "D:\MCStarter\Teas-Launcher"` 看是否有 `Mandatory Label\Low`；
    有的话 `icacls "D:\MCStarter\Teas-Launcher" /setintegritylevel "(OI)(CI)M"`，
    `target` 需 `/T` 递归。验证：正常应为 `Medium Mandatory Level (S-1-16-8192)`。

13. **启动可能停在"校验实例JAR"之后（未定位）** — 2026-10-21 复现：StellarLightTech 1.12.2 的
    库文件已全部下载完成后，`launch_instance` 打完 `[scan] 策略B: PCL-CE风格 — 仅校验实例JAR`
    就再无任何日志，进程 CPU 归零（在 await 上挂着），前端按钮回到初始态但没弹错误。
    可疑点：`download/engine.rs` 里新加的 `MIRROR_SEM`（镜像 3 并发）若许可泄漏会让后续下载永久排队；
    也可能是资产索引请求落在 600 秒超时上。下次复现时先看 `.teas/launcher.log` 有无新的
    `starting new connection`，并考虑给 `MIRROR_SEM.acquire()` 加超时。

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
- **"版本号 → 版本 JSON URL"只有一处实现** — `version/manifest.rs::resolve_version_url`。
  install/vanilla、install/merge、launch 曾经各自请求一遍同一份 Mojang 清单。
- **「选了官方源却走镜像」的根因是硬编码** — `install/forge.rs`、`install/neoforge.rs`、
  `modpack.rs` 的把 `bmclapi2` 写在了 URL 列表第一位，`download/source.rs::source_library` 的
  特殊 maven 分支（forge/fabric/neoforged）则**只返回镜像**，完全不看 `prefer_official`。
  现在这些分支都按设置排序，两边都保留；`install_game` 还会打印一行
  `[install] 加载器 X 下载源: 官方源（prefer_official=true）` 便于核对。
- **MCIMirror 已停运，全仓移除** — `http.rs::apply_source`、`download/source.rs::source_mod_download`、
  `curseforge.rs::cf_mirror`、`modpack.rs` 的镜像回退全部删除；前端设置页的「资源下载源」选择器一并移除。
  现在 CurseForge 没有 API Key 会**直接返回可操作错误**，而不是先走两次必然 403 的请求。
- **classpath 必须按 Maven 坐标去重，而不是按路径** — 整合包/加载器 profile 常同时带
  `org.ow2.asm:asm:9.6` 与 `:9.8`，旧逻辑只删"完全相同的路径"，两个版本都进了 classpath，
  于是 Fabric Loader 直接崩：`duplicate ASM classes found on classpath`。
  现在 `launch/args.rs::build_classpath`、`version/library.rs::dedupe_tokens`、
  `install/merge.rs::merge_libraries` 三处统一用 `maven_key()`（group:artifact[:classifier]，**不含版本**）
  去重并用 `version_gt()` 保留较高版本（数字段按数值比，`9.10 > 9.9`）。
  注意 NeoForge 坐标带 `@jar` 后缀，比较版本前要剥掉。
  回归测试：`classpath_keeps_single_version_per_coordinate`、`asm_duplicate_versions_collapse_to_highest`、
  `natives_classifiers_stay_separate`。
- **Java 8 时代的实例必须用 Java 8 跑，启动前就拦** — `launchwrapper 1.5` 会把系统类加载器强转成
  `URLClassLoader`，Java 9 起必然 `ClassCastException`；LWJGL 2 的本地库同样在 Java 9+ 加载失败。
  以前只打一条 WARN 就放行，用户看到的是"点了没反应"。现在 `launch/java.rs::check_java_compatibility`
  会在启动前阻断，并说明原因与解决办法（判据是 mainClass 含 launchwrapper，或 libraries 里有
  `net.minecraft:launchwrapper` / `org.lwjgl.lwjgl` / `org.lwjgl:lwjgl:2.x`，而不是只看版本号，
  避免把 1.13~1.16 这种「required=8 但 LWJGL3 能跑 Java 17」的情况误拦）。
- **下载引擎：停滞/过慢自动换源 + 断点续传** — 以前只有 600 秒总超时，一条 9 KB/s 但不断的
  连接会一直挂着（用户看到"卡在 8%、剩余 55 分钟"），而且换源等于从头重下，所以也不敢换。
  现在：
  - 停滞检测 `STALL_TIMEOUT=10s`（`tokio::time::timeout` 包住 `stream.next()`）
  - 过慢检测 `SLOW_WINDOW=15s` + `SLOW_MIN_BPS=32 KB/s`（只算**本轮**字节，续传的历史进度不算）
  - 两者都返回 `__STALL__:` / `__SLOW__:` 前缀，调用方看到就不再重试同一 URL，直接换下一个源
  - 换源**保留 .dlpart**，`download_single` / `download_single_with_batch_progress` 都发
    `Range: bytes=<已有字节>-`，206 时追加写；服务器不支持（200）则从头来
  - 校验失败或非停滞类错误仍然清理临时文件
- **限流（403/429/503）必须退避重试，不能当永久失败** — BMCLAPI 是社区镜像，8 并发里会有 3 个
  被回 429/403；旧逻辑直接判死 → 整批文件同时失败。现在 `__THROTTLE__:` 前缀 → 指数退避
  （1s/2s/4s/8s）重试同一个源，并且镜像单独限流 3 并发（`MIRROR_SEM`）
- **reqwest 必须 `.http1_only()`** — 走 Cloudflare 的镜像（BMCLAPI / mirrors.cernet.edu.cn）
  对 h2 协商直接回 403，而同样的地址 curl/HTTP1.1 是 200。加上浏览器 UA 一起，实测下载立刻通了
- **最后一个源不做"过慢中断"** — 否则所有源都慢的大文件（如 Forge universal 4.4 MB）会
  被反复打断、永远下不完。`allow_slow_abort` 由调用方按下标传（`idx + 1 < urls.len()`）
- **第三方 maven 不再加 `libraries.minecraft.net` 兜底** — forge/fabric/neoforged 的 jline、scala、
  modlauncher 在 Mojang CDN 上根本不存在，加了只会 404 浪费 4 次重试；
  改为加**阿里云 Maven Central 镜像**（`maven.aliyun.com/repository/public`，限流友好）
- **Forge universal jar 要自己拼 URL** — 版本 JSON 里这一条通常没有 `url`（只有 path），
  文件实际在 `maven.minecraftforge.net/.../forge-<ver>-universal.jar`；不拼就只能去
  `libraries.minecraft.net` → 404（1.12.2 整合包启动失败的常见原因）
- **长错误不能写进按钮文案，也不该铺在仪表盘里** — `launchStatus` 直接显示在启动按钮里
  （字号 18px、字距 .26em、居中），把"该实例需要 Java 8…"这种多行说明塞进去，按钮就变成
  一堵字墙；改成卡片内大面板后，长文本又会把按钮和标题顶走，怎么调都像布局坏了。
  **最终形态**：卡片里只留一行摘要 `.launch-error-bar`（红点 + 一句话 + 「详情」），
  完整说明放进弹窗（复用 `.modal-overlay` / `.modal-panel`，与崩溃提示一致）。
  按钮文案只允许四个字：准备中 / 检查完整性 / 启动中 / 启动失败。
  踩过的两个坑：**(1) 不要给正文加 `max-height` + `overflow`** —— 形成嵌套滚动后，
  最后一行卡在框口被切一半，滚动条只是内层小框的，用户以为"下面还有字显示不出来"；
  **(2) flex 子项默认 `flex-shrink:1`** —— 容器不够高时会被压缩，`overflow:hidden` 再把内容裁掉，
  连滚动条都没有，需要显式 `flex-shrink:0`。
- **没有账户/实例时，主按钮变成下一步动作（P1-4）** — 以前仪表盘照常显示可点的启动键，点了才弹
  「请先创建账户」。现在 `hasAccount` / `hasInstance` 决定按钮形态：无账户 → `＋ 添加干员`
  （虚线 + warn 色，直达账户页），无实例 → `＋ 获取游戏`（直达获取内容页），两者都有才是启动键。
  干员身份区同步把「切换干员」换成「添加干员」并加一行提示。
- **内存滑块：自动模式不再禁用，推荐值变成轨道刻度（P1-5）** — 以前 `memAuto` 时滑块被
  `disabled`（opacity .35），thumb 位置读不出当前值，推荐值只是一行与滑块无关的文字。
  现在滑块永远可交互（拖动即切手动，靠描边和 shadow 提示"这是推荐值"），当前值就是 thumb
  本身，推荐值画成轨道上的虚线刻度 + 三角，图例只负责说明。
  **别再加"当前值刻度针"**：第一版加了一条白色竖线画在同一个位置，既重复又因为
  `.mem-track-wrap` 的 padding 8px 与 rail 只内缩 8px（应为 16px）而偏移 8px，看起来像异物。
- **表单控件宽度只有三档（P2-6）** — 以前 `.set-input{flex:1}` 让控件跟着剩余空间走，
  同一页出现 70/78/80/780/内容撑开五种宽度，右边缘对不齐。现在 `--field-sm/md` 两档：
  默认 md（220px），`.is-num` 只是右对齐 + tabular-nums（**宽度仍是 md**），
  `.is-num-sm`（88px）只给成对出现的小框（窗口宽高、内存值），`.is-wide` 给长文本。
  **别把数值框做窄**：88px 的框放在 220px 的列里，右边空一大片，看着像没做完；
  反过来，单个小整数（下载线程数 1..64）塞进 220px 空框也一样难看 —— 这类改用
  `.num-stepper`（`− [8] + 线程`，宽约 170px），让控件本身就是一个完整对象。
  **坑**：别用 `flex-basis` 实现宽度——控件可能处在 `flex-direction:column` 容器里，
  那时 flex-basis 是**高度**（分辨率下拉被撑成 220px 高就是这原因）。用 `width` 才安全。
- **枚举/布尔设置不能兜底成文本框（P2-16）** — `advancedSettings` 以前只有 label + def，
  模板一律渲染 `<input>`，于是「版本隔离(是/否)」「日志级别(枚举)」都能输入任意字符串。
  现在数据表带 `type: "toggle" | "select"`（+ `options`、`needsRestart`），模板按类型渲染，
  未声明类型的项**不应再默默变成输入框**（新增设置时必须给类型）。
  连带修掉两处真问题：**(1)** `version_isolation` 的判据从 `s != "否"` 改成容忍
  `否/no/false/0/off/关闭`（以前拼错等于静默开启）；**(2)** `log_level` 之前**没有任何代码读取**，
  现在 `build_logger()` 启动时读它并映射到 `LevelFilter`，UI 标「重启生效」。
  验证：设 ERROR 重启后日志 0 行新增；改回 INFO 重启后 INFO 行恢复。
- **三套列表形态统一（P2-7）** — 以前"获取内容"首页是 470×560 的巨型卡片（内容只有图标+两行字，
  上下各空 200px）、资源浏览 118px 行且没有"可点"提示、实例列表 36px 表格行，密度差 15 倍。
  现在：首页卡片**按内容撑高**（图标/标题/说明/3 条能力清单/底部「进入 ›」），整组用
  `align-content:center` 居中、`align-items:stretch` 保证三张等高；资源行 ~75px + 右侧 `›`；
  实例行 44px（表格结构保留）。
  **坑 (1)** 放在 `flex-direction:column` 容器里的行必须写 `flex:0 0 auto`，否则去掉 min-height 后
  会被压缩到文字重叠；**坑 (2)** `.dl-result-*` 曾有两套重复定义，后一套静默覆盖前一套——
  改样式前先 grep 确认这个类只定义一次。
- **列表行高只有两档（P2-8）** — 以前实例 36 / 版本行 ~40 / 子分类 58 / 资源行 118 四种高度并存。
  现在 `--row-h`（舒适 44 / 紧凑 34）挂在 `<html data-density>`，表格型列表统一用它；
  资源行按密度收缩（紧凑时隐藏描述行）。开关在 设置 → 通用 → 列表密度，写 `list_density` 并即时生效。
  新增列表样式时**必须**用 `var(--row-h)`，不要再写死像素。
- **空状态要有图标 + 引导 + CTA（P2-9）** — 以前任务列表、账户页空了只有一行「暂无任务 / NO TASKS」，
  页面像加载失败。现在统一用 `.empty-state`（`.empty-icon` / `.empty-title` / `.empty-en` / `.empty-hint` /
  `.empty-cta`），三处（任务、账户、实例）共用；CTA 分别指向获取内容、新建账户对话框。
  新增空状态时套这套类，别再写一行小字了事。
- **设置页的空白不要去填（P2-10，未采纳）** — 640px 高度下，设置每个 tab 下面约 40% 是空的。
  试过两版都被否：① 底部加「说明 + 环境信息」卡片（用文案堵空白，比留白更像没做完）；
  ② 内容列收窄到 760px 居中（整页变窄条，不如通栏）。**保持原样**。
  真要填满，需要的是更多真实设置项，不是布局技巧。
  （调试面板的 ENVIRONMENT 区块保留 —— 那是真实状态，不是文案。）
- **路径归一只有一处实现** — `utils::dot_minecraft(mc_dir)` 与 `utils::instance_game_dir(dot, name)`。
  此前 9 个模块各写一份 `if base.join(".minecraft").exists()`，instance.rs 里甚至还有第三份
  `dot_minecraft_of`。

## 已知问题

1. **Microsoft 正版登录未实现** — `AccountsPage.vue` 的入口已如实标注为不可用（按钮 disabled），
   未接入 OAuth 流程。接入时替换 `AuthInfo` 的 access_token/uuid 并放开该按钮即可。
2. **整合包安装后可能缺版本文件** — Forge/Fabric/NeoForge 整合包安装后需自动下载对应加载器的版本 JSON 和 Libraries。
3. **原版游戏下载** — 下载中心的"游戏下载"入口尚未接入实际下载流程。
4. **`FileChecker` 的 JSON 校验分支未启用** — `is_json` 只会由 `with_json()` 置为 true，而该方法当前无调用方。
5. **错误类型仍为 `String`** — 全后端 63 处 `Result<_, String>`、100 处 `map_err(|e| e.to_string())`。
   内部错误类型信息在传递中被压平成字符串，前端无法按类别处理。若要引入 `thiserror`，
   需同时改造数百处 `Err("...".to_string())` 构造点，属于独立的重构任务。
