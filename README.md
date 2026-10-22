# Teas Launcher

<div align="center">

**[ English ]** | **[ 中文 ]**

一个基于 **Tauri v2 + Vue 3 + Rust** 的现代 Minecraft 启动器，采用工业科幻仪表盘风格 UI。

A modern Minecraft launcher built with **Tauri v2 + Vue 3 + Rust**, featuring an industrial sci-fi dashboard UI.

![Platform](https://img.shields.io/badge/platform-Windows%20x64-blue)
![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)
![Vue](https://img.shields.io/badge/vue-3.5-brightgreen)
![Tauri](https://img.shields.io/badge/tauri-v2-ff69b4)
![Build](https://img.shields.io/badge/build-92-yellowgreen)

</div>

---

> [!WARNING]
> **⚠️ 测试阶段 / Alpha Stage**
>
> 本项目目前仍处于早期测试阶段，部分功能可能无法正常使用或存在 Bug。
> 请勿将其用于日常游戏，生产环境建议使用 [HMCL](https://github.com/HMCL-dev/HMCL)、[PCL2](https://github.com/Hex-Dragon/PCL2) 或 [PrismLauncher](https://github.com/PrismLauncher/PrismLauncher) 等成熟启动器。
>
> This project is in early alpha. Some features may be broken or missing. Do NOT use it as your daily driver — use established launchers like HMCL, PCL2, or PrismLauncher instead.

---

## 功能特性 / Features

- 🎮 **游戏安装** — 原版 + Forge / NeoForge / Fabric / Quilt / Cleanroom 五种加载器，整页安装流程
- 📦 **资源获取** — Modrinth 与 CurseForge 双源，搜索 / 详情 / 版本选择 / 安装
- 🗜️ **整合包安装** — `.mrpack` 整合包，支持拖放与本地文件导入
- 📁 **多实例管理** — 版本隔离、加载器检测、Mod 计数；行尾 `⋯` 菜单（启动 / 设置 / 打开目录 / 删除）
- 🧵 **下载引擎** — 多线程并发 + 多源回退 + **停滞/过慢自动换源** + **断点续传**
- 🔍 **JAR 校验** — EOCD 签名 + SHA1/MD5 哈希校验，下载后自动验证
- 🩺 **崩溃检测** — 自动分析 stderr / crash-reports / latest.log 定位问题
- 🧩 **实例级覆盖** — Java 路径 / 内存 / JVM 参数 / 窗口尺寸可按实例单独设置
- 🎨 **工业科幻 UI** — 无边框暗色主题、自定义窗口控件、双语显示

## 下载源与镜像 / Sources & Mirrors

| 类型 | 来源 | 说明 |
|------|------|------|
| 游戏本体 / 加载器 / Libraries | **官方源优先**，可选 BMCLAPI 镜像 | 设置 → 高级 →「游戏下载源」 |
| Mod / 整合包 | 官方源（Modrinth / CurseForge） | 不经第三方镜像 |
| Maven 第三方库（Forge / Fabric / NeoForge） | 官方 maven → 阿里云 Maven Central → BMCLAPI | 自动按速度切换 |

几个已知的处理：

- **MCIMirror 已停止运营（2025）**，相关的镜像分支与「资源下载源」设置已全仓移除。
- 镜像（BMCLAPI）是社区服务，**并发一高会返回 429/403**。引擎对镜像单独限流（3 并发），
  遇到限流走指数退避重试，不会把整批文件判死。
- 请求统一走 **HTTP/1.1**：Cloudflare 对 h2 协商会直接回 403（同一地址 curl 是 200）。
- 单个文件如果长时间无数据（10s）或平均速度低于 32 KB/s（15s），会**自动换下一个源**，
  已下载部分通过 `Range` 续传，不从头再来。

## CurseForge API Key

CurseForge 的官方 API 需要 Key。本项目的处理方式：

| 场景 | Key 从哪来 |
|------|-----------|
| **发布版（Release 构建）** | **打包时内置**，开箱即用，用户无需自行申请 |
| **源码 / 开发版** | **不内置**，需自行提供（见下） |

> [!IMPORTANT]
> **仓库与源码中永远不含明文 Key。** 内置只发生在打包环节（构建时注入），
> 不会提交进 git，也不会出现在日志、错误信息或前端返回值里。
> 自带 Key 的构建产物请勿对外分发 —— 二进制里的 Key 是可以被提取的。

### 源码 / 开发版如何提供 Key

按以下优先级读取：

1. 环境变量 `TEAS_CURSEFORGE_API_KEY`
2. `%APPDATA%\TeasLauncher\curseforge.key` —— 用仓库脚本写入，不回显、不进 shell 历史：

   ```powershell
   ./scripts/set-cf-key.ps1
   ```

没有 Key 时，CurseForge 功能会**直接返回可操作的错误提示**，而不是静默失败或走不可靠的第三方镜像。

无论哪种方式，前端只能拿到 `{configured, source, file}` 这样的状态，**Key 本身永远不会回传**。

## 旧版本资源（pre-1.6）

Beta / Alpha 时代的版本启动时会去拉 `http://s3.amazonaws.com/MinecraftResources/`，
那个桶早已下线，结果是**没有声音、没有 lang**。官方启动器的做法是把 `assets` 索引里
`map_to_resources: true` 的对象摊平复制到 `<gameDir>/resources/`：

```bash
node scripts/fetch-legacy-assets.mjs "<.minecraft>/versions/b1.6.4/b1.6.4.json"
```

> 游戏自身仍会在日志里打一次 S3 404，那是它代码里的死 URL，无法消除；资源在本地即可正常发声。

## Java 版本要求

- 现代版本：Java 21+（启动器会自动选择已安装的 Java）
- **Java 8 时代（launchwrapper / LWJGL 2）的实例必须用 Java 8**：这类实例在 Java 9+ 上会直接
  `ClassCastException: AppClassLoader cannot be cast to URLClassLoader`。启动器会在**启动前拦截**
  并说明原因，而不是让游戏闪退。
- 需要时可在「实例列表 → ⋯ → 设置 → Java 路径」为单个实例指定 Java，不影响全局设置。

## 技术栈 / Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop Shell | **Tauri v2** (Rust, `x86_64-pc-windows-msvc`) |
| Frontend | **Vue 3** + TypeScript + Vite 6 |
| Styling | Global CSS (no Tailwind), industrial sci-fi theme |
| HTTP Client | reqwest (native-tls, stream, socks) |
| Archiving | zip-rs, md5, sha1 |
| System Info | sysinfo |
| Package Management | npm + Cargo workspace |

> Crate 类型为 `["lib", "staticlib"]`：`src-tauri/src/main.rs` 链接 staticlib 生成可执行文件。

## 快速开始 / Quick Start

### 前置要求 / Prerequisites

- **Rust** 1.80+（Windows 目标 `x86_64-pc-windows-msvc`）
- **MSVC 生成工具**（Visual Studio Build Tools：MSVC v143 + Windows SDK）
  - 没装过可以用仓库里的脚本：`./scripts/install-msvc.ps1`
- **Node.js** 18+、**npm**

### 开发 / Development

```bash
npm install              # 安装前端依赖
npx tauri dev            # Vite 热重载 + Rust 自动重编译
```

> Vite 固定用 1420 端口；被占用时先杀掉旧进程再启动。

### 测试 / Test

```bash
cargo test --lib         # Rust 单元测试（57 个）
npm run typecheck        # vue-tsc 类型检查（npm run build 也会跑）
```

### 构建 / Build

```bash
npx tauri build
```

产物位于 `target/x86_64-pc-windows-msvc/release/`（Cargo workspace 在仓库根目录）。

### 版本号 / Version

- Cargo semver：`0.1.1`
- BUILD 常量在 `src-tauri/src/lib.rs`，每次改动核心逻辑 +1；`get_full_version()` 返回 `v0.1.1.0092`

## 项目结构 / Project Structure

```
Teas-Launcher/
├── Cargo.toml                 # Rust workspace root
├── package.json               # 前端依赖
├── index.html                 # Vue 挂载点
├── vite.config.ts             # Vite 配置（端口 1420）
├── AGENTS.md                  # 开发文档（架构、约定、踩坑记录）
├── UI-REVIEW.md               # 前端界面审查清单（P0/P1/P2）
│
├── scripts/                   # 开发辅助脚本
│   ├── set-cf-key.ps1         #   写入 CurseForge Key（不回显）
│   ├── install-msvc.ps1       #   安装 MSVC 生成工具到非系统盘
│   ├── capture-window.ps1     #   窗口截图（CopyFromScreen）
│   └── capture-hwnd.ps1       #   窗口截图（PrintWindow，不抢焦点）
│
├── src/                       # Vue 3 前端
│   ├── main.ts
│   ├── App.vue                # 页面路由（v-if 切换）+ 全局状态 provide
│   ├── styles.css             # 全局样式
│   ├── utils/                 # 版本号比较 / tc.ini 读写 / 实例设置合并 / 源映射
│   ├── composables/
│   │   └── useGameInstall.ts  # 游戏安装向导状态机
│   └── components/
│       ├── Sidebar.vue / StatusBar.vue
│       ├── DashboardPage.vue  # 仪表盘（启动按钮 + 干员身份 + 实例卡片）
│       ├── InstancesPage.vue  # 实例列表（⋯ 菜单 + 实例设置页）
│       ├── DownloadsPage.vue  # 获取内容（游戏安装 / 整合包 / Mod 搜索详情）
│       ├── DownloadsTaskPage.vue # 下载任务（聚合进度 + ETA + 并发数）
│       ├── AccountsPage.vue   # 账户管理
│       ├── SettingsPage.vue   # 全局设置
│       └── DebugPage.vue      # 调试面板（Dev）
│
└── src-tauri/                 # Rust 后端
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── tests/fixtures/        # 6 份真实版本 JSON（launch 单测用）
    └── src/
        ├── main.rs            # 入口 → teas_launcher_lib::run()
        ├── lib.rs             # 模块声明 + BUILD + 命令注册
        ├── download/          # 下载引擎（多源回退、续传、进度事件）
        ├── version/           # 版本清单 / Libraries / Assets
        ├── install/           # 各加载器安装（Forge/NeoForge/Fabric/Quilt/Cleanroom）
        ├── launch/            # 启动流程（mod/args/process/natives/java + tests）
        ├── progress.rs        # download-progress 事件唯一出口
        ├── instance.rs        # 实例扫描 / 删除 / 版本解析
        ├── config.rs          # tc.ini 读写
        ├── crash.rs           # 崩溃检测
        ├── system.rs          # Java 扫描 / 系统信息 / 网络检测
        ├── http.rs            # HTTP 客户端（代理、UA、HTTP/1.1）
        ├── modrinth.rs / curseforge.rs / modpack.rs
        └── utils.rs           # JAR 校验、路径归一、UUID
```

## 启动引擎 / Launch Engine

启动逻辑参照 **HMCL** / **PCL** / **PCL-CE** / **PrismLauncher** 的设计：

- **参数构建** — HMCL/PCL 风格，完整 JVM + Game 参数链，支持 OS/Feature rules 过滤；
  参数以 `Vec<String>` 直接交给 `Command::args()`，不做「拼接后再切分」
- **classpath 去重** — 按 Maven 坐标（`group:artifact[:classifier]`，不含版本）去重并保留高版本，
  避免同一库出现两个版本（如 asm 9.6 / 9.8）导致 Fabric Loader 拒绝启动
- **Natives 管理** — PCL 风格，清空 → 重解压 → 删除孤立文件
- **GC 优化** — Java 21+ 分代 ZGC / 15-20 ZGC / 8-14 G1GC 自动选择
- **安全防御** — Log4j RCE 漏洞防御参数
- **编码处理** — HMCL 风格 `-Dfile.encoding` / `-Dstdout.encoding` 跨 Java 版本兼容
- **进程监控** — stdout/stderr stream pump + ExitWaiter，退出后自动触发崩溃检测

## 文档 / Docs

| 文件 | 内容 |
|------|------|
| [AGENTS.md](AGENTS.md) | 架构、模块职责、开发约定、**已修复问题的根因记录**（避免回退） |
| [UI-REVIEW.md](UI-REVIEW.md) | 前端界面审查清单：P0/P1/P2 问题、CurseForge 遗留项、验证证据 |

## TODO

- [x] **游戏安装入口** — 原版 + 五种加载器，整页安装流程 ✅
- [x] **Assets 资源自动补全** — 缺失资源文件自动下载 ✅
- [x] **下载源偏好持久化** — 官方源 / BMCLAPI 设置正确保存 ✅
- [x] **Fabric 整合包兼容** — 空 `arguments.game` 自动补充默认参数 ✅
- [x] **模组安装路径** — 版本隔离下自动定位 `versions/<name>/mods/` ✅
- [x] **实例 MC 版本检测** — PCL 风格多源回退 ✅
- [x] **Quilt / Cleanroom 安装入口** ✅
- [x] **CurseForge 接入**（需要用户自己的 API Key）✅
- [x] **下载引擎健壮性** — 停滞后换源、断点续传、限流退避 ✅
- [ ] **Microsoft 正版登录** — 目前仅支持离线模式
- [ ] **Authlib-Injector 外置登录** — 第三方验证服支持
- [ ] **CurseForge 整合包安装** — 目前整合包仅支持 Modrinth `.mrpack`
- [ ] **OptiFine / LiteLoader / LegacyFabric / LabyMod** — 当前加载器只有 5 种，这几个未接入
- [ ] **整合包安装后自动补全** — 加载器版本 JSON / Libraries 自动下载
- [ ] **启动后窗口行为** — 启动游戏后自动隐藏 / 最小化启动器
- [ ] **多语言支持** — 目前中文 UI（部分双语）
- [ ] **自动更新** — 启动器自身更新机制
- [ ] **Linux / macOS 支持** — 目前仅 Windows x64

## 已知限制 / Known Limitations

- **启动偶发停在「校验实例JAR」之后** — 已在 AGENTS.md 记录为待定位问题（现象：无日志、CPU 归零、
  前端静默回到初始态）。
- **整合包安装后可能缺版本文件** — Forge/Fabric/NeoForge 整合包需要额外补齐加载器 JSON 与 Libraries。
- **错误类型仍是 `String`** — 后端尚未引入 `thiserror`，前端无法按错误类别分支处理。
- **以管理员身份运行时拖放会被系统拦截**（Windows UIPI）：请用非管理员终端启动。

## 配置 / Configuration

配置使用 JSON 格式的 `tc.ini`：

| Scope | Path | Content |
|-------|------|---------|
| `launcher` | `.teas/tc.ini` | 实例数据 |
| `user` | `%APPDATA%/TeasLauncher/tc.ini` | 用户设置、账户信息 |

日志：`.teas/launcher.log`（单文件上限 5MB）、游戏 stderr 落在 `.teas/last_stderr.log`。

## 设计系统 / Design System

```
--bg-deep:   #0A0A0A    --bg-mid:    #121212    --bg-panel:  #161618
--bg-raised: #1C1C1E    --border:    #252528    --border-lit:#333338
--accent:    #C8FF3D    --accent-dim:rgba(200,255,61,.12)
--secondary: #FFFFFF    --text:      #D0D0D4    --text-dim:  #6A6A72
--warn:      #FF8A3D    --danger:    #FF4D4D
```

## 许可 / License

MIT License

---

<div align="center">

**Teas Launcher** — 让每一次启动都充满仪式感 ✨

</div>
