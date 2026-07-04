## Teas Launcher — 潜在问题清单
## 生成日期: 2026-07-04
## 用途: 新对话中逐个修复的参考索引
## 每个条目包含: 位置 -> 症状 -> 根因 -> 修复方向

---

## P0 — 严重

### 1. ZIP 路径遍历漏洞
- **位置**: `src-tauri/src/modpack.rs:88` (install_modpack Phase 2 解压)
- **症状**: 恶意 `.mrpack` 可通过 `../` 路径将文件写入 `tmp_dir` 外部。
- **根因**: `tmp_dir.join(&name)` 后未验证 `dest` 是否是 `tmp_dir` 的子路径。
- **修复**: 解析前调用 `dest.canonicalize()` 并检查其是否以 `tmp_dir.canonicalize()` 为前缀。
```rust
let dest = tmp_dir.join(&name);
let canon = dest.canonicalize().map_err(|_| "bad path")?;
if !canon.starts_with(tmp_dir.canonicalize()?) {
    return Err(format!("路径遍历: {}", name));
}
```

### 2. 零测试覆盖
- **位置**: 整个项目, Rust + Vue
- **症状**: 任何修改都无法自动验证是否引入回归。
- **根因**: `cargo test` / `vitest` 均未配置。
- **修复方向**:
  1. 先为核心下载引擎 (`download/engine.rs`) 添加单元测试 (Mock HTTP server).
  2. 为 `download/source.rs` (已有 `#[cfg(test)]` 块, 可扩展).
  3. 为 `launch.rs` 的 `build_flat_args` / `deduplicate_args` / `split_args_keep_quoting` 函数添加单元测试.
  4. 前端: 安装 vitest, 为关键逻辑 (如 download task 生命周期) 写组件测试.

### 3. 下载取消是全局的, 不按任务隔离
- **位置**: `src-tauri/src/http.rs:6` (CANCEL_DOWNLOAD AtomicBool)
- **症状**: 同时下载 Mod A 和整合包时, 取消任一任务会取消全部。
- **根因**: `static CANCEL_DOWNLOAD: AtomicBool` 全局唯一, `download_single` 和 `download_single_with_batch_progress` 都检查它。
- **修复方向**:
  1. 将 `CANCEL_DOWNLOAD` 改为 `Mutex<HashSet<u64>>` (用 batch_id/任务 ID 索引).
  2. `cancel_download()` 接收 `batch_id` 参数.
  3. `is_cancelled()` 接收 `batch_id` 并检查对应标识.
  4. 保留全局取消作为"全部取消"的扩展选项.

### 4. Config 写入竞态条件 (读-改-写无锁)
- **位置**: `src-tauri/src/config.rs:40` (config_write)
- **症状**: 两个并发的 `config_write` 调用同时读取后分别写入, 后完成的会覆盖先完成的, 数据静默丢失。
- **根因**:
```rust
let mut cfg = parse(read(path));  // 读取
cfg[key] = value;                  // 修改
write(path, cfg);                  // 覆写 — 无锁
```
- **修复方向**:
  1. 添加全局 `Mutex<()>` 或文件级 `Mutex` 串行化所有 config 写操作.
  2. (更优) 使用 atomic write: 先写 `.tmp` 文件, 再 rename 到目标文件.
  3. 前端也应合并连续的保存调用 (debounce 500ms).

### 5. `resolve_version` 无限递归 (inheritsFrom 环)
- **位置**: `src-tauri/src/instance.rs:9` (resolve_version)
- **症状**: 循环 `inheritsFrom` 链导致栈溢出, 启动器崩溃。
- **根因**: 纯递归, 无 `visited` 集合, 无深度上限。
- **修复方向**:
```rust
fn resolve_version(dot_minecraft, instance_name) -> Result<Value, String> {
    resolve_version_impl(dot_minecraft, instance_name, &mut HashSet::new(), 0)
}
fn resolve_version_impl(..., visited: &mut HashSet<String>, depth: u32) {
    if depth > 10 { return Err("inheritsFrom 链过深"); }
    if !visited.insert(instance_name.to_string()) { return Err("检测到循环"); }
    // ... 原有逻辑
}
```

---

## P1 — 功能正确性/可靠性

### 6. 配置文件写入非原子
- **位置**: `src-tauri/src/config.rs:50` (config_write)
- **症状**: 写入过程中崩溃 -> 配置文件变为空白或截断损坏.
- **根因**: 直接 `fs::write(&path, ...)` 覆写.
- **修复**: 先写 `path.with_extension(".tmp")`, 再 `std::fs::rename(tmp, path)`.

### 7. `scan_parallel` 线程爆炸
- **位置**: `src-tauri/src/utils.rs:95` (scan_parallel)
- **症状**: `libraries/` 有数百个子目录 -> 同时 spawn 数百个线程 -> 可能 OOM.
- **根因**: 每个子目录无条件 `std::thread::spawn`.
- **修复方向**:
  1. 最佳: 使用 `rayon` 库的并行迭代器.
  2. 替代: 使用 `std::sync::Arc<Semaphore>` 限制最大并行为 CPU 核数.
  3. 或直接用方案 A (QuickCheck) 作为默认策略, 不使用 Parallel.

### 8. `build_http_client` 每次调用都重新读取配置
- **位置**: `src-tauri/src/http.rs:42` (build_http_client)
- **症状**: 8 线程并发下载 50 个文件 -> 配置文件被读取+解析 ~50 次.
- **根因**: 无缓存.
- **修复方向**:
  1. 使用 `once_cell::sync::Lazy<Mutex<CachedConfig>>` 缓存配置, 设置 TTL (如 5 秒).
  2. 或者启动时读取一次, 通过 `Arc<Config>` 传递.

### 9. `DownloadsPage.vue` 搜索无 debounce
- **位置**: `src/components/DownloadsPage.vue` (search 输入框)
- **症状**: 快速打字时每秒发送多次 Modrinth API 请求, 超过限速 (300/min).
- **修复**: 搜索输入上添加 `debounce` (300-500ms). VueUse 的 `useDebounceFn` 或手写 `setTimeout` 清空.

### 10. 所有页面通过 `<KeepAlive>` 保持存活
- **位置**: `src/App.vue` (template 底部)
- **症状**: 9 个组件全部常驻内存, 搜索缓存/API 响应/下载进度从不释放。使用时间越长内存占用越高.
- **修复方向**:
  1. 对 DownloadsPage 和 InstancesPage 使用 `:include` 限制.
  2. 或者大型页面使用 `onActivated`/`onDeactivated` 清理缓存.

### 11. `system.rs` 互斥锁争用
- **位置**: `src-tauri/src/system.rs` (get_system_stats / get_memory_info)
- **症状**: 前端每秒轮询两个命令, 它们争夺同一个 `Mutex<System>`, 都需要 `refresh_all()` (较耗时).
- **根因**: 两个 Tauri 命令共享同一个 `Mutex<System>` 状态.
- **修复方向**:
  1. 合并为单个命令一次返回 CPU + MEM.
  2. 或者 frontend 只调用 `get_system_stats`, 从中同时取内存数据.
  3. 对 refresh 做节流 (500ms).

### 12. 每个文件下载都创建新 `reqwest::Client`
- **位置**: `src-tauri/src/download/engine.rs:180` (download_single) 和 `:220` (download_single_with_batch_progress)
- **症状**: 连接池、TLS 配置、代理每次下载重新初始化.
- **修复**: 启动时创建 `reqwest::Client` 单例, 通过 `Arc<Client>` 或 Tauri State 共享.

### 13. Rust 端 `LaunchContext` 有大量死代码
- **位置**: `src-tauri/src/launch.rs:85` (LaunchContext struct)
- **症状**: `#[allow(dead_code)]` 标注了大量字段 (`min_memory`, `metaspace`, `game_args`, `env_vars`, `process_priority`), 实际 launch 命令未使用.
- **根因**: `launch_instance` 命令直接从头构建 `Vec<String>` 参数, 不经过 `LaunchContext`.
- **修复**: 决定 `LaunchContext` 的去留: 要么让 `launch_instance` 使用它填充, 要么删除它.

---

## P2 — UX/性能

### 14. Dev 模式硬编码路径
- **位置**: `src-tauri/src/system.rs:27` (get_minecraft_dir) 和 `src-tauri/src/config.rs:6` (teas_dir)
- **症状**: `#[cfg(dev)]` 下所有路径指向 `D:\Minecraft\[000A]` — 仅在这台机器上可用.
- **修复**: 改为读取环境变量 `TEAS_DEV_MC_DIR` 或 `CARGO_MANIFEST_DIR/../test_minecraft`.

### 15. `dlTasks` 响应式数组直接修改
- **位置**: `src/App.vue:35` (updateDlTask)
- **症状**: `dlTasks.value.find(...).status = x` 直接修改数组元素属性.
- **根因**: Vue `ref` 默认深度响应式, 碰巧能工作, 但依赖这个行为很脆弱.
- **修复**: 改为
```ts
const idx = dlTasks.value.findIndex(t => t.id === id);
if (idx !== -1) dlTasks.value[idx] = { ...dlTasks.value[idx], status };
```

### 16. 前端 `inject` 无类型保护
- **位置**: `src/components/DashboardPage.vue:8` 及其他组件
- **症状**: `inject<Ref<boolean>>("isLaunching", ref(false))` — 如果 App.vue 未正确 provide, fallback 是一个独立 ref, 永远不会更新.
- **修复方向**:
  1. 在 `App.vue` 中开一个 composable (`useAppState()`) 统一管理 provide/inject.
  2. 或使用简单的 global state store (不用 Pinia, 仅 reactive singleton).

### 17. `SettingsPage.vue` 保存失败仅 console.error
- **位置**: `src/components/SettingsPage.vue` (save 函数)
- **症状**: 用户保存设置若失败, UI 无任何反馈.
- **修复**: 使用 toast 提示 "保存失败" 或添加短暂红色闪烁.

---

## P3 — 维护性/健壮性

### 18. `read_json_field` 用字符串查找解析 JSON
- **位置**: `src-tauri/src/utils.rs:200` (read_json_field)
- **症状**: 在 JSON 文本中搜索 `"fieldName": "value"` 子串, 遇到转义引号、字段名在其他位置出现等情况会误解析.
- **修复**: 替换为 `serde_json::from_str::<Value>` 后 `["field"].as_str()`.

### 19. 崩溃检测只匹配 8 种模式
- **位置**: `src-tauri/src/crash.rs` (check_crash 底部的模式表)
- **症状**: 缺少大量常见崩溃: `IndexOutOfBoundsException`, `NullPointerException`, `GLFW error`, `Pixel format not accelerated`, `UnsatisfiedLinkError`, `IllegalStateException`, `java.io.IOException` 等.
- **修复**: 参考 HMCL/PCL 的崩溃检测类, 扩展为 30+ 模式.

### 20. `kill_instance` 存在 PID 重用竞争
- **位置**: `src-tauri/src/launch.rs:50` (kill_instance)
- **症状**: 两次 `taskkill` 调用之间, PID 可能被 OS 回收并分配给新进程 -> 误杀.
- **根因**:
```rust
taskkill /F /PID 1234;  // 第一次: 杀掉进程
taskkill /F /T /PID 1234;  // 第二次: 如果 PID 已重用, 误杀
```
- **修复**: 只执行 `taskkill /F /T /PID` 一次 (带 `/T` 会杀进程树, 无需先不带 `/T`).

### 21. `RUNNING_PID` 不持久化
- **位置**: `src-tauri/src/launch.rs:44` (RUNNING_PID static)
- **症状**: 启动器崩溃重启后, `RUNNING_PID = None`, UI 显示"未运行"但 MC 仍在运行.
- **修复**: 启动时用 `tasklist` 或读取 PID 文件检查孤儿 MC 进程.

### 22. `styles.css` 46KB 单文件
- **位置**: `src/styles.css` (46KB, ~1100 行)
- **症状**: 无设计 token 层、组件样式和基础样式混合.
- **修复方向**: 在合适时机拆分为:
  - `src/styles/tokens.css` (CSS 变量、palette、字体)
  - `src/styles/base.css` (reset、utilities、transitions)
  - 组件内 `<style scoped>` 保留组件特化样式.

### 23. TypeScript 无 strict 模式
- **位置**: `tsconfig.json`
- **症状**: 未启用 `strict: true`, 隐式 any、未检查 null 值大量存在.
- **修复**: 逐步启用 `strictNullChecks` -> `noImplicitAny` -> `strict: true`.

### 24. `include_bytes!` 始终嵌入 LUA.jar
- **位置**: `src-tauri/src/launch.rs:36` (LUA_JAR_BYTES)
- **症状**: `include_bytes!("../LUA.jar")` 始终嵌入 ~10KB 到二进制中, 即使不使用.
- **修复方向**:
  1. 如果确定不再需要 LUA.jar, 删除该常量及 `LUA.jar` 文件.
  2. 否则加 feature gate: `#[cfg(feature = "lua_compat")]`.

---

## 快速修复参考 (按优先级排序)

1. modpack.rs: 添加路径遍历检查 — 1 行代码, 安全关键
2. launch.rs: taskkill 只执行带 `/T` 的一次 — 删除 3 行
3. config.rs: 读写加 Mutex 锁 — ~5 行
4. http.rs: CANCEL_DOWNLOAD 改为 per-task — ~15 行
5. instance.rs: resolve_version 添加 visited 集合 — ~10 行
6. config.rs: atomic write (tmp -> rename) — ~5 行
7. engine.rs: reqwest::Client 改为单例 — ~10 行
8. DownloadsPage.vue: 搜索加 debounce — ~5 行
9. DashboardPage.vue: 合并 CPU/MEM 为单次调用 — ~10 行前端
10. crash.rs: 扩展崩溃模式表 — 添加 15+ 模式

---

*文件结束。在新对话中可用 `@BUG.md` 引用此文件。*
