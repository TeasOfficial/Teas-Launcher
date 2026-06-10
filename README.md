# Teas Launcher

<div align="center">

**[ English ]** | **[ 中文 ]**

一个基于 **Tauri v2 + Vue 3 + Rust** 的现代 Minecraft 启动器，采用工业科幻仪表盘风格 UI。

A modern Minecraft launcher built with **Tauri v2 + Vue 3 + Rust**, featuring an industrial sci-fi dashboard UI.

![Platform](https://img.shields.io/badge/platform-Windows%20x64-blue)
![Rust](https://img.shields.io/badge/rust-1.80%2B-orange)
![Vue](https://img.shields.io/badge/vue-3.5-brightgreen)
![Tauri](https://img.shields.io/badge/tauri-v2-ff69b4)

</div>

---

> [!WARNING]
> **⚠️ 测试阶段 / Alpha Stage**
>
> 本项目目前仍处于早期测试阶段，绝大多数功能可能无法正常使用或存在严重 Bug。
> 请勿将其用于日常游戏，生产环境建议使用 [HMCL](https://github.com/HMCL-dev/HMCL)、[PCL2](https://github.com/Hex-Dragon/PCL2) 或 [PrismLauncher](https://github.com/PrismLauncher/PrismLauncher) 等成熟启动器。
>
> This project is in early alpha. Most features may be broken or missing. Do NOT use it as your daily driver — use established launchers like HMCL, PCL2, or PrismLauncher instead.

---

## TODO

> 以下为当前已知的待办事项，按优先级排列。

- [ ] **Microsoft 正版登录** — 目前仅支持离线模式
- [ ] **Authlib-Injector 外置登录** — 第三方验证服支持
- [ ] **CurseForge 整合包安装** — 目前仅支持 Modrinth (.mrpack) 格式
- [ ] **原版游戏下载** — 下载中心入口尚未接入安装流程
- [ ] **整合包安装后自动补全** — Forge/Fabric/NeoForge 加载器的版本 JSON / Libraries 自动下载
- [ ] **非 ASCII 路径兼容** — Windows GBK 编码下路径含中文时的 JLW 支持
- [ ] **启动后窗口隐藏** — 游戏启动后自动隐藏/恢复启动器窗口
- [ ] **多语言支持** — 目前仅支持中文
- [ ] **自动更新** — 启动器自身更新机制
- [ ] **Linux / macOS 支持** — 目前仅支持 Windows x64
- [ ] **测试覆盖** — 缺乏自动化测试

---

## 功能特性 / Features

- 🎮 **原版安装** — 支持 Mojang 官方源和 BMCLAPI 镜像，一键安装任意版本
- ⚙️ **加载器安装** — Forge / NeoForge / Fabric / Quilt / OptiFine / LiteLoader / Cleanroom / LegacyFabric / LabyMod
- 📦 **Modrinth 集成** — 搜索、浏览、下载 Mod / 资源包 / 光影 / 数据包
- 🔥 **CurseForge 集成** — 搜索、浏览、下载 CurseForge 项目
- 🗜️ **整合包安装** — 支持 Modrinth (.mrpack) 格式整合包
- 📁 **多实例管理** — 版本隔离、加载器检测、Mod 计数
- 🧵 **多线程下载** — 8 线程并发 + BMCLAPI 镜像加速 + 聚合进度追踪
- 🔍 **JAR 校验** — EOCD 签名 + SHA1/MD5 哈希校验，下载后自动验证
- 🩺 **崩溃检测** — 自动分析 stderr / crash-reports / latest.log 定位问题
- 🎨 **工业科幻 UI** — 无边框暗色主题、自定义窗口控件、双语显示

## 技术栈 / Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop Shell | **Tauri v2** (Rust, `x86_64-pc-windows-gnu`) |
| Frontend | **Vue 3** + TypeScript + Vite |
| Styling | Global CSS (no Tailwind), industrial sci-fi theme |
| HTTP Client | reqwest (native-tls, stream, socks) |
| Archiving | zip-rs, md5, sha1 |
| System Info | sysinfo |
| Package Management | npm + Cargo workspace |

## 截图 / Screenshots

> 开发中，即将添加

## 快速开始 / Quick Start

### 前置要求 / Prerequisites

- **Rust** 1.80+ (GNU toolchain: `stable-x86_64-pc-windows-gnu`)
- **Node.js** 18+
- **pnpm** (推荐) 或 npm

### 开发 / Development

```bash
# 安装前端依赖
npm install

# 启动开发模式（Vite 热重载 + Rust 自动重编译）
npx tauri dev
```

### 构建 / Build

```bash
# 生产构建
npx tauri build
```

构建产物位于 `src-tauri/target/release/`。

## 项目结构 / Project Structure

```
Teas Launcher/
├── Cargo.toml                # Rust workspace root
├── package.json              # 前端依赖
├── index.html                # Vue 挂载点
├── vite.config.ts            # Vite 配置
├── tsconfig.json             # TypeScript 配置
│
├── src/                      # Vue 3 前端源码
│   ├── main.ts               # 应用入口
│   ├── App.vue               # 根组件（页面路由 + 全局状态）
│   ├── styles.css            # 全局样式 (~1200 行)
│   └── components/
│       ├── Sidebar.vue       # 侧栏导航
│       ├── StatusBar.vue     # 顶部状态条
│       ├── DashboardPage.vue # 仪表盘主页（启动按钮 + 实例信息）
│       ├── InstancesPage.vue # 实例列表
│       ├── DownloadsPage.vue # 下载中心（Modrinth/CurseForge）
│       ├── DownloadsTaskPage.vue # 下载任务管理
│       ├── AccountsPage.vue  # 账户管理
│       ├── SettingsPage.vue  # 全局设置
│       └── DebugPage.vue     # 调试面板（Dev 模式）
│
├── src-tauri/                # Tauri Rust 后端
│   ├── Cargo.toml            # Rust 依赖
│   ├── tauri.conf.json       # Tauri 配置
│   ├── icons/                # 应用图标
│   └── src/
│       ├── main.rs           # 入口点
│       ├── lib.rs            # 模块声明 + Tauri Builder
│       ├── launch.rs         # 游戏启动引擎
│       ├── instance.rs       # 实例管理
│       ├── install/          # 安装器（Vanilla/Forge/Fabric/Quilt...）
│       ├── download/         # 下载引擎（多源回退 + 并行 + 进度）
│       ├── version/          # 版本/Library/Assets 管理
│       ├── config.rs         # 配置持久化（tc.ini）
│       ├── crash.rs          # 崩溃检测
│       ├── system.rs         # 系统命令（Java 扫描、代理、网络检测）
│       ├── http.rs           # HTTP 客户端 + 镜像源
│       ├── modrinth.rs       # Modrinth API
│       ├── curseforge.rs     # CurseForge API
│       ├── modpack.rs        # 整合包安装
│       └── utils.rs          # JAR 校验、UUID、Natives 等工具
│
└── crates/
    └── mcpatch-core/         # McPatch 增量更新核心库
```

## 启动引擎 / Launch Engine

游戏的启动逻辑参照 **HMCL** / **PCL** / **PCL-CE** / **PrismLauncher** 四大启动器的源码设计，取其精华：

- **参数构建** — HMCL/PCL 风格，完整 JVM + Game 参数链，支持 OS/Feature rules 过滤
- **Natives 管理** — PCL 风格，清空→重解压→删除孤立文件
- **进程启动** — 直接 `ProcessBuilder` + `CREATE_NO_WINDOW`，不通过 BAT 脚本
- **GC 优化** — Java 21+ 分代 ZGC / 15-20 ZGC / 8-14 G1GC 自动选择
- **安全防御** — Log4j RCE 漏洞防御参数
- **编码处理** — HMCL 风格 `-Dfile.encoding` / `-Dstdout.encoding` 跨 Java 版本兼容
- **进程监控** — stdout/stderr stream pump + ExitWaiter，退出后自动触发崩溃检测

## 配置 / Configuration

配置文件使用 JSON 格式的 `tc.ini`，存储位置：

| Scope | Path | Content |
|-------|------|---------|
| `launcher` | `.teas/tc.ini` | 实例数据、下载镜像偏好 |
| `user` | `%APPDATA%/TeasLauncher/tc.ini` | 用户设置、账户信息 |

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
