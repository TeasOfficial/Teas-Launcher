# Teas Launcher — 前端界面审查清单

> 生成日期：2026-10-20（BUILD 77，dev 构建）
> 审查方式：真实运行 `npx tauri dev`，用 Computer Use **只截启动器窗口**（1100×640 @125% → 1375×800），未截全屏
> 截图目录：[`.review/`](.review/)（文件名对照见文末）
> 截图工具：[`scripts/capture-window.ps1`](scripts/capture-window.ps1)（DPI 感知 + 只抓窗口矩形）
> 每条都带「状态」勾选框，修完把 `[ ]` 改成 `[x]`。

## 严重度

| 标记 | 含义 |
|---|---|
| 🔴 P0 | 阻断主流程 / 必现错误 |
| 🟠 P1 | 影响可用性，用户会卡住或误操作 |
| 🟡 P2 | 视觉/一致性/打磨 |

---

## 🔴 P0-1 安装游戏流程：弹窗装不下 → 改为独立页面

- [x] **已重做为独立页面**（BUILD 80）。原方案是在弹窗里修滚动，用户否决并改为页面化。

**改动**：
- 删除「安装游戏」弹窗（`DownloadsPage.vue`），新增 `active === "install"` 页面，走与搜索/详情一致的页面切换与返回栈
- 页面布局：左列 = 版本横幅 + 加载器列表（单选行，含推荐/不兼容态）；右列 = 安装摘要（实例名、游戏版本、
  加载器、加载器版本、目标目录）+ 取消/确认安装
- 加载器版本选择作为页内子页（`showLoaderVersions`），`goBack` 会先退回加载器列表再离开
- 顺带修：实例名分隔符改成下划线（`26.3-Forge_66.0.2`，与已有实例 `1.20.1-Forge_47.4.23` 一致）；
  版本网格只给最新版标「推荐」（Forge 每个版本都 stable，全标等于没标）

**验证**：`.review/pg02.png`（空态）、`.review/pg08.png`（选完 Forge 66.0.2，摘要与目录同步）、
`.review/pg07.png`（版本网格，仅最新版带推荐）

### 整合包安装（同样页面化，BUILD 81）

- 删除「安装整合包」弹窗，新增 `active === "modpack-install"` 页面：左栏 = 整合包横幅 + 文件条 + 四步流程 + 简介，右栏 = 实例名/来源/目标目录 + 取消/开始安装
- 返回栈：`modpackReturnTo` 记录来源（详情页），取消或安装后回到原页
- 修：默认实例名去掉 `.zip` 后缀（原来会把 `Souldrend-v8.5.1.zip` 当实例名）
- 验证：`.review/mp10.png`（安装页）、`.review/mp11.png`（取消回到详情）
### 实例设置（同样页面化，BUILD 82）

- `InstancesPage.vue` 增加 `view: "list" | "settings"`，设置不再是弹窗：页头切成「← 返回 + 实例设置」，列表隐藏
- 左栏 = 实例名 + 说明 + 4 组覆盖项（Java/内存/JVM/分辨率），右栏 = **全局当前值对照** + 取消/保存
- 修：右栏「最大内存」曾显示成 `7026 MB MB`（值本身已带单位）
- 验证：`.review/iset03.png`（设置页）、`.review/iset04.png`（取消回到列表）

### 「拖放红叉」的真正原因：工作区被打了 Low 完整性标签（已修复）

`D:\MCStarter\Teas-Launcher` 曾带 `Mandatory Label\Low Mandatory Level`，继承到 `target\...\debug`。
从那里启动的 exe 以 **Low** 令牌运行，于是：`WebView2 0x8000FFFF`、`无法确定数据目录: 拒绝访问`、
`日志目录不可写`、拖放被系统拒绝——全部同源。

修复：`icacls "D:\MCStarter\Teas-Launcher" /setintegritylevel "(OI)(CI)M"`（target 需 `/T`），
修复后 `create .teas = OK`、`write probe = OK`、进程为 Medium。
### 拖放"红叉"根因：UIPI（BUILD 84）

实测完整性级别：`explorer.exe` = **Medium**，`teas-launcher.exe` = **High（管理员）**。
Windows 的 UIPI 禁止跨完整性级别拖放，所以光标显示禁止符——**不是代码问题，是系统拦截**。

同时修了监听方式：`Window.onDragDropEvent` 在 Tauri v2 已废弃，改为 `getCurrentWebview().onDragDropEvent`，
并补上 `enter` / `over` / `leave`，这样悬停时浮层能正常出现（原来依赖 HTML5 `dragover`，
在 `dragDropEnabled: true` 下永远不会触发）。

**新增不依赖拖放的入口**：获取内容 → 整合包 → 搜索栏右侧「本地整合包」按钮（系统文件对话框）。
验证：`.review/drop03.png`。文件对话框本身未自动化测试（会弹出系统级模态框）。
### 拖放导入（页面化 + 3 个行为修复，BUILD 83）

- 拖放确认弹窗 → `currentPage = "dropInstall"` 独立页面（左侧 LOCAL PACK 文件信息 + 四步流程，右侧实例名/来源/目录 + 取消/开始安装）
- **拖入非整合包文件不再静默**：以前拖 `.jar` 进来完全没反应，现在弹 toast 说明只支持 `.mrpack/.zip`
- **`alert()` 换成 toast**：安装失败原来用 `alert()`，会阻塞 webview；且原代码在 `finally` 之前抛错时 `dragInstalling` 永远是 false
- `dragInstalling` 现在真正在安装期间置位（防重复拖入触发两次安装），结束时复位；取消会回到拖放前的页面
- 验证：`.review/drop01.png`（页面布局，用临时初始状态渲染后已还原）
**仍在用弹窗的**：账户新增（AccountsPage）、删除失败提示（InstancesPage）、拖放导入（已页面化）。
**遗留**：`useGameInstall` 里 `showGameInstall` 已不再驱动 UI（仅 `doInstallGame` 内部重置），
可后续清理；「安装整合包」仍是弹窗，如需统一可同样页面化。

## 🔴 P0-2 `.vue` 不参与类型检查，运行时错误只能靠肉眼发现

- [x] **已修复**（BUILD 79）：接入 `vue-tsc`，`npm run build` = `vue-tsc --noEmit && vite build`，
  另加 `npm run typecheck`。首次运行查出 **19 个错误**，全部修完（详见下方"P0 修复明细"）

**现象**：搜索页显示 `搜索失败: ReferenceError: readUserConfig is not defined`（`.review/p21.png`）。原因是重构时改了 `DownloadsPage.vue` 的调用却漏了 import，而 `npm run build` 里的 `tsc` **不解析 .vue 文件**，构建全绿。

**位置**：`package.json` 的 `build` 脚本（`tsc && vite build`）。

**建议**：引入 `vue-tsc`，把构建脚本改成 `vue-tsc --noEmit && vite build`；同时给 SFC 加一层 `npm run typecheck`。这类错误本可以在 CI 阶段拦住。

---

## 🟠 P1-3 实例行的「设置 / 删除」几乎点不到

- [x] **已修复**（BUILD 87）：改成行尾常驻 `⋯` 菜单

**修复前（实测）**：

| 操作 | 结果 |
|---|---|
| 打开列表 | 除鼠标所在行外，**任何一行都看不到按钮**（`.review/p13b.png`） |
| 点行中间 / 偏 25px | 选中实例并**跳到仪表盘**（`.review/p13h.png`、`p13i.png`） |
| 点准按钮 | 才能进设置（`.review/p13g.png`） |

整行约 1290px 都是"选中并跳走"，两个按钮只藏在右侧 80px 且默认 `opacity:0`。

**修复内容**：

- 行尾固定一列 **`⋯` 按钮，常驻可见**（`InstancesPage.vue` / `.inst-more-btn`）
- 菜单项：**启动 / 设置 / 打开目录 / 删除**，点菜单外部自动关闭（`document` 级监听）
- **行单击改为只选中，不再跳页**；进仪表盘改为：菜单「启动」或**双击行**
- 新增后端命令 `open_instance_folder`（`instance.rs`），复用 `utils::instance_game_dir` 处理版本隔离
- 删除仍是二次确认，只是从行内按钮挪进菜单（`确认删除` 红底态）

**验证**：

| 验证点 | 证据 |
|---|---|
| 每行都有 `⋯` | `.review/menu01.png` |
| 菜单展开（启动/设置/打开目录/删除） | `.review/menu05.png` |
| 点空白处菜单关闭 | `.review/menu03.png` |
| 单击行**留在列表**、只切换 active | `.review/menu04.png` |
| 菜单「设置」进设置页 | `.review/menu06.png` |
| 菜单「启动」进仪表盘且实例正确 | `.review/menu07.png` |
| 「打开目录」 | 未自动化验证（会弹资源管理器窗口），代码走 `open_instance_folder` |

**取舍**：单击不再跳页，可能有人觉得"点了没反应"。缓解手段：行有 `title="单击选中 · 双击进入仪表盘"`，
且选中状态是可见的（行高亮 + 左侧色条）。如果后续仍有人困惑，可在行尾加一个常驻的「▶ 启动」按钮。
## 🟠 P1-4 无账户时没有任何引导

- [x] **已修复**（BUILD 92）

**改法**：不再"点了才报错"，而是把主按钮直接变成下一步动作。

| 状态 | 主按钮 | 干员身份区 |
|---|---|---|
| 有账户 + 有实例 | 初始化序列（原样） | 切换干员 |
| **无账户** | **＋ 添加干员** → 账户页（虚线 + warn 色） | **添加干员** + 一行提示「任意名字即可创建离线账户」 |
| 有账户 + **无实例** | **＋ 获取游戏** → 获取内容页 | 切换干员 |

「无账户」状态下按钮不再是可点但会失败的启动键，而是明示的入口；`launch()` 里的
`if (!activeAccount) { showNoAccount = true; return; }` 保留为兜底（正常点不到）。

**证据**：`.review/p14_01.png`（临时清空 `tc.ini` 的 accounts 复现，截完已还原）。

**未验证**：「无实例」分支（`获取游戏`）没有清空实例目录去复现，逻辑同上但没截图为证。

**现象**：`.review/p01.png`。干员身份区只有一个空头像和 `---`，UUID 行是"未登录 · ——"，而右侧"初始化序列"按钮照常可点；点了才弹"请先创建账户"。

**位置**：
- [`src/components/DashboardPage.vue:114`](src/components/DashboardPage.vue#L114) —— `if (!activeAccount?.value) { showNoAccount.value = true; return; }`
- [`src/components/DashboardPage.vue:282`](src/components/DashboardPage.vue#L282) —— 「未选择账户」提示弹窗

**建议**：无账户时把主按钮换成「添加账户」并直达账户页（而不是点了才报错）；干员身份区加一行 CTA。首启场景（没有任何实例时）再加一个"去获取内容"的入口。

---

## 🟠 P1-5 设置-启动：内存滑块读不出"当前值"

- [x] **已修复**（BUILD 93）

**改法**：

| 问题 | 现在 |
|---|---|
| 自动模式下滑块 `disabled`、灰得看不出位置 | **不再禁用**：保持可交互，拖动即切手动（描边提示是推荐值） |
| 当前值只靠一行文字 | 滑块 thumb 就是当前值 + 数值框同排显示（78px，等宽数字） |
| "推荐 9180 MB" 是独立文字，和滑块无关 | 推荐值变成**轨道上的虚线刻度**（顶部三角），图例只做说明「▲ 推荐 N MB」 |
| 0.5 GB / 推荐 / 自动 / 最大 四个元素挤一行 | 统一为 `.mem-legend`：左端最小、中间图例、自动开关、右端最大 |


**首版翻车与修正**：最初在轨道上额外画了一条白色「当前值刻度针」（`.mem-mark-cur`），
但 thumb 本身就在那个位置 —— 纯属重复；而且实测它和 thumb **错开 8px**，看起来像贴在圆点右侧的异物。
根因：`.mem-track-wrap` 有 `padding:0 8px`，`.mem-slider` 因此整体内缩 8px，
而 rail 只内缩了 8px（应该 16px = 容器 padding 8 + 半个 thumb 8），所以刻度和 thumb 位置不重合。
最终做法：**删掉刻度针**（thumb 就是当前值），rail 改 `left/right:16px`，推荐刻度现在精确压在 thumb 中心。

**证据**：`.review/p15_06.png`（自动模式，推荐刻度在轨道上）、`.review/p15_07.png`（手动模式，
推荐刻度仍保留作参照，thumb 移到 17664 MB）。

**顺带修的坑**：改 CSS 后 Vite 一度返回 `__vite__css = ""`（页面完全无样式），重启 dev server 才恢复；
文件本身没问题（postcss 能解析）。遇到"整页没样式"先看这一点。

**现象**：`.review/p07.png`。自动内存开启时滑块被 `disabled`（opacity .35），thumb 停在中间偏右，而数值是 9180 MB（上限 23.8 GB）；下方"0.5 GB / 推荐 9180 MB / 自动 / 最大 23.8 GB"四个元素挤在同一行，无法判断当前分配位置。

**位置**：
- [`src/components/SettingsPage.vue:367`](src/components/SettingsPage.vue#L367) —— 滑块行
- [`src/components/SettingsPage.vue:379`](src/components/SettingsPage.vue#L379) —— 刻度/说明行
- [`src/styles.css:822`](src/styles.css#L822) —— `.mem-slider`（禁用态 opacity .35）

**建议**：自动模式下滑块保持可见（只读样式），并在当前值位置画一个标记点/刻度高亮；把"推荐值"做成滑块上的一个刻度，而不是独立文字；数值输入框与滑块同一行、单位对齐。

---

## 🟡 P2-6 表单控件宽度没有档位

- [x] **已修复**（BUILD 94）

**现象**：同是 `.set-item`，分辨率下拉宽 780px、"是/否"切换只有 120px、"JVM 参数"输入 780px、"下载线程数"整行只有一个居中的 8（`.review/p06.png`、`p09.png`）。

**根因**：`.set-input{flex:1}`——控件吃掉整行剩余空间，跟内容无关；
少数行又手写 `width:70/80px`，`.mem-num` 再来个 78px。等于五档宽度并存。

**改法**：

```css
:root{ --field-sm:88px; --field-md:220px; }
.set-input{ flex:0 0 auto; width:var(--field-md); }        /* 默认 md */
.set-input.is-num{ text-align:right; font-variant-numeric:tabular-nums; }  /* 宽度仍是 md */
.set-input.is-num-sm{ width:var(--field-sm); ... }             /* 只给成对的小框 */
.set-input.is-wide{ flex:1 1 auto; width:auto }            /* 只有长文本撑满 */
.set-item .custom-select{ width:var(--field-md) }
```

套用结果：代理端口 → **is-num**（220px、数字右对齐）；窗口宽高 / 内存值 → **is-num-sm**（88px，旁边总有配对元素）；

**下载线程数是个例外**：它是 1..64 的单个小整数，塞进 220px 空框只放一个 `8`，怎么调都像没做完。
最终改成**步进器** `− [8] + 线程`（宽约 170px），空间成了控件的一部分，不再是空洞。
JVM 参数、Java 路径 → **is-wide**；其余输入与下拉 → 默认 md。8 处内联 `width` 全部移除。


**第二版修正**：第一版把数值框做成 88px 宽（`flex-basis` 档），结果 220px 的列里杵着一个小框，
右边空一大片 —— 只对齐了左边缘，右边缘更散了。现在数值框**跟其他控件同宽（md 220）**，
只靠 `text-align:right` + `tabular-nums` 表达"这是数字"；88px 只留给成对出现的小框。

**坑**：一开始用 `flex:0 0 var(--field-md)` 实现，结果分辨率那行的下拉变成 **220px 高**——
因为它的父容器是 `flex-direction:column`，`flex-basis` 在那里等于高度。改成 `width` 才对。

**证据**：`.review/p26_02.png`（高级：线程数 88 右对齐、日志级别 220）、`p26_05.png`（通用：分辨率 220）、
`p26_07.png`（启动：内存 is-num、Java 路径/JVM 参数 is-wide）。

---

## 🟡 P2-7 三套列表形态并存，视觉语言不统一

- [x] **已修复**（BUILD 96）

- `.review/p04.png`：获取内容首页是**竖长卡片**（470×560，内容垂直居中，大量空白）
- `.review/p20.png`：资源浏览是**横向行 + 右箭头**（66px 行高）
- `.review/p02.png`：实例是**表格式行**（30px 行高）

**实际改法**（与原建议略有出入）：

| 位置 | 改前 | 改后 |
|---|---|---|
| 获取内容首页 | 470×560 卡片，内容垂直居中，上下各空 200px | 卡片改为**内容驱动**：图标 / 标题 / 说明 / 3 条能力清单 / 底部「进入 ›」，整组垂直居中、三张等高 |
| 资源浏览 | 行高 118px，无"可点"提示 | 行高 **~75px**，右侧补 `›`（hover 变亮并右移），描述单行省略 |
| 实例列表 | 36px | **44px**（表格结构保留） |

**踩坑**：`.dl-result-card` 放在 `flex-direction:column` 的 `.dl-results` 里，把 `min-height` 去掉后
行被压缩到叠字；必须写 `flex:0 0 auto` 锁住。另外 `.dl-result-*` 当时存在**两套重复定义**
（后一套覆盖前一套，`flex-wrap:wrap` 让行高又涨回去），已合并为一套。

**未做**：资源行的 `⋯` 动作菜单（快捷安装/打开详情）、首页卡片下方的最近更新——需要新数据源。

**证据**：`.review/p27_final_home.png`、`p27_final_browse3.png`、`p27_final_inst.png`。

---

## 🟡 P2-8 行高不一致

- [x] **已修复**（BUILD 97）

实例 30px、版本列表 56px、资源浏览 66px。版本列表一屏只能看 9 条（`.review/p17.png`），40 个实例也只能看 18 行（`.review/p02.png`）。建议统一并提供紧凑/舒适两档。

**改法**：

- 新增 `--row-h` token：舒适 44px / 紧凑 34px，挂在 `<html data-density>`
- 表格型列表（实例行、版本行）统一 `height/min-height: var(--row-h)`
- 资源行按密度收缩：舒适 ~75px，紧凑 52px（隐藏描述行、图标 32px）
- 设置 → 通用 → **列表密度：紧凑 / 舒适**，即时生效并写入 `list_density`

**效果**：实例列表 17 行（舒适）/ 19 行（紧凑）；资源列表 6 条 / 8 条。

**证据**：`.review/p28_general.png`（开关）、`p28_compact_inst.png`、`p28_compact_browse.png`。

---

## 🟡 P2-9 空状态太素

- [x] **已修复**（BUILD 98）

`.review/p03.png`（任务列表）整页只有"暂无任务 / NO TASKS"一行小字；`.review/p05.png`（账户）同理。建议：图标 + 一句话引导 + 一个 CTA（"去获取内容"/"新建账户"）。

**改法**：抽一套共享 `.empty-state`（圆形图标框 + 标题 + 英文副标 + 一句说明 + 描边 CTA），
三处空状态统一：

| 页面 | 图标 | CTA |
|---|---|---|
| 任务列表 | ◫ | 去获取内容 → `navigate("downloads")` |
| 账户管理 | ◉ | ＋ 新建账户 → 打开新建对话框 |
| 实例列表 | ▤ | 前往获取内容（搜不到时不给 CTA） |

**证据**：`.review/p29_tasks.png`（任务列表空状态）、`p29_nav.png`（点 CTA 已跳到获取内容）。
账户空状态**未实测**（本机已有 1 个账户，无法触发），只做了编译校验。

---

## 🟡 P2-10 留白失衡

- [ ] **未采纳（已回退）**

设置页每个 tab 下方约 40% 是空的（p06–p09）；调试面板只占上半屏（p13）。

**试过两版，都撤了**：

1. **底部加「说明 NOTES + 环境信息」卡片** —— 被否。用填充文案去堵空白，等于承认这块本来没内容，
   比留白更像没做完。
2. **内容列收窄到 760px 居中** —— 也被否。整页变成窄条，反而不如原来的通栏。

**结论**：这不是缺陷。640px 固定高度 + 一个 tab 只有 3 行设置，下面空着是正常的；
强行"填满"只会让页面更廉价。要让这里不空，需要的是**真内容**（更多设置项、或者把设置拆得更细），
而不是布局技巧。**保持原样。**

唯一保留的改动：调试面板补了 ENVIRONMENT 区块（Data Dir / Java Path / Launcher Log /
Instances / Accounts / Log Level）—— 那里是真实状态，不是说明文字，并未被否。

---

## 🟡 P2-11 关于页有重复且离题的文案

`.review/p10.png` 中"开发辅助: Claude Code · DeepSeek"与"BUILT WITH: CLAUDE CODE · DEEPSEEK"重复出现两次，且与启动器功能无关。建议删掉或并进一行"技术栈"小字。

---

## 🟡 P2-12 版本列表的 ⚒ 图标没有说明

`.review/p17.png` 每行右侧的锤子图标语义不明（代码里是 `title="Forge 可用"`，只悬停可见），容易被当成错误或警告标记。建议换成带文字的标签（Forge 可用）或加图例。

---

## 🟡 P2-13 加载态只有文字

各列表加载时是"加载中..."纯文本，没有骨架屏，切页会有明显跳动。建议给列表/卡片加骨架。

---

## 🟡 P2-14 次要文字对比度偏低

[`src/styles.css:18`](src/styles.css#L18) `--text-dim: #6A6A72` 在 `--bg-deep: #0A0A0A` 上约 **3.7:1**，低于 WCAG AA 的 4.5:1。而它承载了大量 9–10px 的英文副标题（每个导航项、每个字段标签都有）。建议提到 #8A8A92 左右（≈5:1）或只在小字号处提亮。

---

## 🟡 P2-15 双语堆叠带来的信息噪声

全局每个可点项都是"中文 + 英文大写小字"。英文不提供额外信息，在 1100px 宽下显得碎。建议英文只在关键位置保留，或加一个"显示英文"开关（设置项）。

---

## 🟡 P2-16 枚举 / 布尔设置被渲染成自由文本框

- [x] **已修复**（BUILD 95）

**现象**：高级设置里「版本隔离」（是/否）和「日志级别」（枚举）都是普通输入框，
可以随便输入任意字符串，保存后也不会报错。

**根因**：`advancedSettings` 只声明了 label 与默认值，模板对没有类型的项一律兜底成
`<input class="set-input">`。

**连带的两个真问题**：

| 设置 | 问题 | 修法 |
|---|---|---|
| 版本隔离 | Rust 判据是 `s != "否"`，打成 `否 `（带空格）/ `no` / `false` 会被**静默当成开启** | 改 `toggle` 控件，Rust 侧同时容忍 `否/no/false/0/off/关闭` |
| 日志级别 | **没有任何代码读它**，改了不生效 | 接进 `build_logger()`：启动时读 `log_level` 映射到 `LevelFilter`，UI 标注「重启生效」 |

**证据**：`.review/adv05.png`（下拉展开，5 档都在）、`.review/adv07.png`（选中 ERROR）；
重启验证：ERROR 级别下启动后日志 **0 行新增**，改回 INFO 重启后 `[app] Teas Launcher … (build 94)` 恢复输出。

---
## 本轮已修复（供回溯，不在此清单重复）

| 问题 | 位置 |
|---|---|
| Vite 监视 `target/` 导致 `EBUSY` 崩溃（workspace 布局） | [`vite.config.ts`](vite.config.ts) |
| panic 只写日志、不写 stderr，日志插件未就绪时"静默退出 101" | [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) |
| 日志目录不可写时直接 panic（现在降级为仅控制台） | [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) |
| 搜索页 `readUserConfig is not defined`（漏 import） | [`src/components/DownloadsPage.vue`](src/components/DownloadsPage.vue) |

---

## 环境备注（下次复现用）

1. **cargo 不在 PATH**：`cmd` 里 `cargo` 找不到，需 `export PATH="$HOME/.cargo/bin:$PATH"`（或写进用户环境变量）。
2. **dev 数据目录硬编码** `D:\Minecraft\[000A]`（见 `BUG.md` #14）：只能用真实环境跑，或被沙箱挡住写权限。
3. **WebView2 用户数据目录与完整性级别绑定**：管理员终端（High）启动的实例，Medium 进程复用同一 UDF 会报 `0x800700AA`。切换调试方式时要先清 `%LOCALAPPDATA%\com.teaslauncher.desktop`。
4. **Computer Use 截图**：`get_window_state` 在本机返回的是屏幕区域且有缓存，改用 [`scripts/capture-window.ps1`](scripts/capture-window.ps1)（DPI 感知 + `CopyFromScreen` 只抓窗口矩形）。窗口内坐标系与 1375×800 截图一一对应。

---

## 截图对照

| 文件 | 内容 |
|---|---|
| p01 | 仪表盘（无账户） |
| p02 | 实例列表（40 个实例） |
| p03 | 任务列表（空态） |
| p04 | 获取内容（三张入口卡片） |
| p05 | 账户管理（空态） |
| p06 | 设置-通用 |
| p07 | 设置-启动（内存滑块问题） |
| p08 | 设置-网络 |
| p09 | 设置-高级 |
| p10 | 设置-关于 |
| p11 | 原版游戏（三个分类折叠态） |
| p12 | 新建账户弹窗 |
| p13 | 调试面板 |
| p14 | 实例列表（删除确认态，按钮露出） |
| p15 | 旧版列表展开 |
| p17 | 正式版列表展开（⚒ 图标） |
| p18 | **安装游戏弹窗（P0-1 证据）** |
| p19 / p19a / p20 / p20a | 资源浏览 / 返回过程 |
| p21 | **搜索页报错（P0-2 证据，已修）** |

---

## CurseForge 接入遗留（2026-10-20 密钥接入验证后新增）

验证结果：`classId=4471`（整合包）与 `classId=6`（模组）均返回 HTTP 200，走的是官方
`api.curseforge.com`（不是镜像），详情/文件列表正常。以下是在验证过程中暴露的遗留项。

### 🟠 CF-1 搜索结果全部显示「版本未知」
- [ ] 未修复

CF 搜索响应里每条带 `latestFilesIndexes[].gameVersion`，前端没映射，导致兼容性徽章恒为
「版本未知」。证据：`.review/cf08.png`（Maps3D / Hackers 'n Slashers 等全部如此）。
位置：`src/components/DownloadsPage.vue` 的 `openDetail`/搜索结果映射处。

### 🟡 CF-2 详情页 followers 恒为 0
- [ ] 未修复

`src/components/DownloadsPage.vue` 里 CF 分支写死 `followers: 0`（API 实际返回 `followers` 字段）。

### 🟡 CF-3 版本卡片标题重复
- [ ] 未修复

CF 的 `fileName` 同时被填进 `name` 和 `version_number`，于是每行出现两遍同样的文件名
（见 `.review/cf04.png`：`Souldrend-v8.5.1.zip` ×2）。

### 🟠 CF-4 文件下载仍拼 fileId + 强制走镜像
- [ ] 未修复

`src-tauri/src/curseforge.rs::install_curseforge_file` 用 `fileId/1000`、`fileId%1000` 拼
`edge.forgecdn.net` 再套镜像；有了合法 key 之后应该改用 API 返回的 `downloadUrl`（官方 CDN），
镜像只作兜底。

### 🟡 CF-5 整合包「安装」文案未区分
- [ ] 未修复

CurseForge 的整合包同样是 `.zip`，点「安装」会走 `openModpackInstall`，但按钮文案和普通
文件一样，用户不知道会触发整合包安装向导（会先下 500 MB+）。

### 下载任务表现力（BUILD 85）

- 后端 `BatchProgress` 的速度表从 `Vec<u64>`（按数值去重，实际会失真）改成 `HashMap<文件名, 速度>`，
  `total_speed` 由后端按"当前真正在传的文件"求和后下发，前端不再自己累加
- 事件新增 `batch_running`（当前并发文件数），任务卡显示"N 路并发"
- 任务一拿到信号量许可就上报 0% 事件：并发文件**立刻**出现在详情里，不用等各自首字节（大文件要等几秒）
- 进度节流 100ms → 50ms；卡片新增**剩余时间**估算与"已下载 / 总量"
