# Teas Launcher — 项目文档

## 项目概述

Rust + Tauri v2 + Vue 3 的 Minecraft 启动器，工业科幻仪表盘风格 UI。

## 技术栈

| 层 | 技术 |
|------|------|
| 桌面壳 | Tauri v2 (Rust, x86_64-pc-windows-gnu) |
| 前端 | Vue 3 + TypeScript + Vite |
| 样式 | 全局 CSS (src/styles.css)，无 Tailwind |
| 包管理 | npm + Cargo workspace |

## 启动命令

```bash
npx tauri dev       # 开发（Vite热重载 + Rust自动重编译）
npx tauri build     # 生产构建
npm run build       # 仅构建前端
cargo build         # 仅构建Rust
```

## 目录结构

```
mcstarter/
├── Cargo.toml                 # Rust workspace root
├── package.json               # 前端依赖
├── index.html                 # Vue 挂载点
├── vite.config.ts
├── tsconfig.json
├── src/                       # 前端源码
│   ├── main.ts                # Vue createApp 入口
│   ├── App.vue                # 根组件（页面路由 + 全局状态）
│   ├── styles.css             # 全局样式（~1000行）
│   └── components/
│       ├── Sidebar.vue        # 侧栏导航 + 网络状态 + 版本显示
│       ├── StatusBar.vue      # 顶部状态条（时钟 + 窗口控件）
│       ├── DashboardPage.vue  # 仪表盘主页（启动按钮 + 玩家身份 + 实例信息 + 遥测条）
│       ├── InstancesPage.vue  # 实例列表（搜索、删除、选择）
│       ├── DownloadsPage.vue  # 下载中心（Modrinth API 搜索/详情/安装）
│       ├── DownloadsTaskPage.vue # 下载任务管理（进度+历史）
│       ├── AccountsPage.vue   # 账户管理（Microsoft/离线/Authlib）
│       ├── SettingsPage.vue   # 设置（Java路径/内存/下载源/分辨率/线程数）
│       └── DebugPage.vue      # Dev模式调试面板
├── src-tauri/                 # Tauri Rust 后端
│   ├── Cargo.toml             # Rust 依赖
│   ├── tauri.conf.json        # Tauri 配置（1100x640 固定窗口）
│   ├── build.rs
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       ├── main.rs            # 入口 → teas_launcher_lib::run()
│       ├── lib.rs             # 模块声明 + run() 入口（~70行）
│       ├── utils.rs           # JAR 校验、文件操作、UUID、Natives、OS过滤
│       ├── http.rs            # HTTP 客户端、代理、镜像源、流式下载+进度
│       ├── config.rs          # tc.ini 配置读写、.teas 目录
│       ├── forge.rs           # Forge/NeoForge installer.jar 解析
│       ├── instance.rs        # 实例同步、版本解析、列表/删除
│       ├── launch.rs          # Minecraft 启动流程
│       ├── crash.rs           # 崩溃检测（stderr/crash-reports/latest.log）
│       ├── system.rs          # 系统命令（版本号、状态、网络、Java扫描、代理）
│       ├── modrinth.rs        # Modrinth API（搜索、详情、下载）
│       └── modpack.rs         # 整合包安装（完整流程）
├── crates/mcpatch-core/       # McPatch 增量更新核心库（早期集成，现基本搁置）
└── target/
```

## 关键设计决策

- **无边框窗口**: decorations=false, 自定义顶栏拖拽
- **固定尺寸**: 1100x640, resizable=false, maximizable=false
- **无 cdylib**: GNU 工具链 DLL 导出符号限制，crate-type = ["lib", "staticlib"]
- **页面路由**: App.vue 中 v-if 切换页面组件，无 vue-router
- **状态共享**: provide/inject 传递 instances, accounts, download tasks
- **持久化**:
  - `%APPDATA%/TeasLauncher/tc.ini` → 用户设置+账户（多启动器共享）
  - `.teas/tc.ini` → 实例数据
  - `.teas/launcher.log` / `.teas/last_stderr.log` → 日志
- **Dev 模式**: Rust `#[cfg(dev)]` 控制，Minecraft 目录为 `D:\Minecraft\[000A]`
- **多线程下载**: tokio::spawn + Semaphore，默认 8 线程，设置可调 1-64
- **下载源镜像**: BMCLAPI (游戏), MCIMirror (Modrinth 资源)，默认启用

## Rust 后端命令清单

所有命令通过 `#[tauri::command]` 注册，分布在以下模块中：

### 系统 — `system.rs`
- `check_update` — 占位，调用 mcpatch_core
- `get_version` / `get_full_version` — 版本号（BUILD 常量拼入）
- `is_dev` — cfg!(dev)
- `get_system_stats` — CPU/MEM (sysinfo)
- `check_network` — Modrinth/CurseForge/Minecraft 连通性，返回 JSON {ok, total, failures}
- `test_proxy_connectivity` — 代理连通性测试
- `detect_system_proxy` — 读取 Windows 注册表系统代理
- `scan_java` — 自动搜索 Java 安装，返回 Vec<{path, version}>
- `get_minecraft_dir` — Dev 返回 D:\Minecraft\[000A]，Release 返回 exe 目录
- `get_teas_dir` — 返回 .teas 目录路径

### 配置 — `config.rs`
- `config_read(scope)` — 读取 tc.ini 全部 JSON
- `config_write(scope, key, value)` — 合并写入 key-value
- scope: "launcher" → .teas/tc.ini, "user" → %APPDATA%/TeasLauncher/tc.ini

### 实例管理 — `instance.rs`
- `list_instances(mc_dir)` — 扫描 `versions/` 目录
- `delete_instance(mc_dir, instance_name)` — 删除实例目录
- `fix_instance(app, mc_dir, instance_name, source)` — 补全缺失的版本 JAR、资源索引、Libraries
- `resolve_version(dm, name)` — 递归解析 inheritsFrom 合并父版本数据
- `sync_instance_files(app, dm, name, source, label)` — 核心同步逻辑

### 启动 — `launch.rs` (2026-06-09 重写, BUILD=64)

参照 HMCL/PCL/PCL-CE/PrismLauncher 四大启动器完全重写:

**架构**: `LaunchContext` → `pre_check` → `ensure_parent_version` → 补全 Libraries+Assets → `prepare_natives` (PCL-style 清空+重解压) → `build_arguments` (HMCL/PCL-style 完整参数链) → `pre_run_setup` (log4j2.xml + options.txt) → `spawn_process` (直接 ProcessBuilder, 不用 bat) → `monitor_process` (流监控 + 崩溃检测)

**命令**:
- `launch_instance(...)` — 完整启动流程
- `kill_instance()` — taskkill /F /T /PID + 子进程树
- `is_instance_running()` — 检查 RUNNING_PID
- `get_launch_args(...)` — 调试用参数预览

**关键改进** (vs 旧版):
1. 直接 ProcessBuilder 启动 (不用 BAT), `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`
2. 完整 classpath 构建 (PCL-style 排序, OptiFine 倒数第二)
3. JVM 参数: 编码/GC(G1GC+ZGC)/Log4j RCE防御/Java版本适配
4. Natives: 清空→重解压→删除孤立文件 (PCL-style)
5. 预启动: log4j2.xml提取、options.txt语言修复
6. 进程监控: stdout/stderr pump threads + ExitWaiter + 崩溃检测集成
7. 环境变量: APPDATA, Path(含Java bin), INST_*

### 崩溃检测 — `crash.rs`
- `check_crash(mc_dir, instance_name)` — 检查 stderr.log / crash-reports / latest.log
- 8 种关键词→原因匹配
- 正常退出检测：日志中含 "Stopping!" 返回 None

### HTTP — `http.rs`
- `build_http_client(timeout)` — 读取代理配置创建 reqwest 客户端
- `apply_source(url, source)` — URL 镜像转换
- `download_with_progress(app, srcs, filename, label)` — 流式下载 + 进度发射
- `cancel_download()` — 设置取消标志

### Forge — `forge.rs`
- `read_forge_installer(dm, name, archive, mc_ver, loader_ver)` — HMCL-style 解析 installer.jar

### Modrinth API — `modrinth.rs`
- `search_bbsmc(...)` — 搜索 Modrinth
- `get_project(project_id)` — 项目详情
- `get_project_versions(project_id)` — 版本列表
- `install_file(app, mc_dir, url, filename, project_type, source)` — 下载文件+进度
- `install_project(mc_dir, project_id, project_type)` — 旧版安装

### 整合包 — `modpack.rs`
- `install_modpack(...)` — 完整流程：下载 .mrpack→解压→下载 Mod→安装加载器→同步文件

### 工具 — `utils.rs`
- `is_jar_valid(path)` — EOCD 签名校验
- `scan_and_fix_jars(dir, deleted)` — 递归扫描删除损坏 JAR
- `move_dir_contents(src, dst)` — 递归移动目录内容
- `detect_loader(path)` — 检测加载器类型
- `read_json_field(path, name, field)` — 快速读取 JSON 字段
- `offline_uuid(name)` — MD5 生成离线 UUID
- `extract_natives(dm, ver, name)` — 从 libraries JAR 提取 native DLL
- `arg_matches_current_os(obj)` — OS 规则过滤

## 前端全局状态 (App.vue provide)

| Key | 类型 | 说明 |
|-----|------|------|
| `instances` | `Ref<Instance[]>` | 实例列表（从 list_instances 加载） |
| `activeInstance` | `ComputedRef<Instance>` | 当前活跃实例 |
| `accounts` | `Ref<Account[]>` | 账户列表（从 user config 加载） |
| `activeAccount` | `ComputedRef<Account>` | 当前活跃账户 |
| `dlTasks` | `Ref<DownloadTask[]>` | 下载中任务 |
| `dlHistory` | `Ref<DownloadTask[]>` | 下载历史（会话内） |
| `addDlTask(name) → id` | function | 创建下载任务 |
| `updateDlTask(id, status)` | function | 更新任务状态 |
| `finishDlTask(id, error?)` | function | 完成任务→移入历史 |
| `setActiveAccount(index)` | function | 切换活跃账户并保存 |

## 下载任务生命周期

1. `addDlTask("xxx.jar")` → dlTasks.push({id, name, status:"下载中..."})
2. 下载进行中 → 进度事件更新 status 字段
3. 成功: `updateDlTask(id, "已完成")` → `finishDlTask(id)` → 移入 dlHistory
4. 失败: `updateDlTask(id, "失败")` → `finishDlTask(id, error)` → 移入 dlHistory
5. 取消: 用户点击 ✕ → `cancel_download()` → `finishDlTask(id)` → 移入 dlHistory

## 下载进度事件 (2026-06-10 优化)

Rust 端发射 `download-progress` 事件，格式:
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

前端 DownloadsTaskPage.vue 处理:
- 按 `batch_id` 聚合进度 → 进度条平滑递增不横跳
- 顶栏显示总下载速度（所有活跃文件速度之和）
- 详细信息面板显示每个文件的迷你进度条+速度+大小

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
- BUILD 常量: 每次修改核心逻辑后 +1（当前 64）
- `get_full_version()` 返回: `v0.1.1.0063`

## 开发约定

- 前端修改无需重启，Vite 热重载
- Rust 修改后 Tauri dev 自动重编译
- 端口 1420 (Vite), 被占用时需手动杀进程
- 项目没有 Git 仓库 - 谨慎修改 lib.rs

## 已知问题 (2026-06-09)

1. **启动逻辑重写 ✅** — BUILD=64, 参照 HMCL/PCL/PCL-CE/PrismLauncher 完全重写
2. **整合包安装后缺版本文件** — Forge/Fabric/NeoForge 整合包安装后需要自动下载对应加载器的版本 JSON 和 Libraries。
3. **Microsoft 正版登录** — 未实现。
4. **原版游戏下载** — 下载中心的"游戏下载"入口尚未接入实际下载流程。
