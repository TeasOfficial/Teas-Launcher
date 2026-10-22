<script setup lang="ts">
import { ref, computed, provide, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { readUserConfig, writeUserConfig, writeLauncherConfig } from "./utils/config";
import Sidebar from "./components/Sidebar.vue";
import StatusBar from "./components/StatusBar.vue";
import DashboardPage from "./components/DashboardPage.vue";
import InstancesPage from "./components/InstancesPage.vue";
import DownloadsPage from "./components/DownloadsPage.vue";
import AccountsPage from "./components/AccountsPage.vue";
import SettingsPage from "./components/SettingsPage.vue";
import DebugPage from "./components/DebugPage.vue";
import DownloadsTaskPage from "./components/DownloadsTaskPage.vue";

export interface DownloadTask { id: number; name: string; status: string; error?: string; steps?: string[]; }

const currentPage = ref("dashboard");
const dlTasks = ref<DownloadTask[]>([]);
const dlHistory = ref<DownloadTask[]>([]);
let dlTaskId = 0;
const dlToast = ref("");

const isLaunching = ref(false);
const isRunning = ref(false);
provide("isLaunching", isLaunching);
provide("isRunning", isRunning);

function addDlTask(name: string) {
  const id = ++dlTaskId;
  dlTasks.value.push({ id, name, status: "下载中..." });
  dlToast.value = `已开始下载: ${name}`;
  setTimeout(() => { if (dlToast.value === `已开始下载: ${name}`) dlToast.value = ""; }, 3000);
  return id;
}
function updateDlTask(id: number, status: string) { const t = dlTasks.value.find(t => t.id === id); if (t) t.status = status; }
function finishDlTask(id: number, error?: string) {
  const t = dlTasks.value.find(t => t.id === id);
  console.log(`[DL] finishDlTask id=${id} found=${!!t} error=${error} dlTasks=${dlTasks.value.length} dlHistory=${dlHistory.value.length}`);
  if (t) {
    if (error) t.error = error;
    dlHistory.value = [...dlHistory.value, { ...t }];
    dlTasks.value = dlTasks.value.filter(t => t.id !== id);
    console.log(`[DL] after: dlTasks=${dlTasks.value.length} dlHistory=${dlHistory.value.length}`);
  }
}
provide("dlTasks", dlTasks);
provide("dlHistory", dlHistory);
provide("addDlTask", addDlTask);
provide("updateDlTask", updateDlTask);
provide("finishDlTask", finishDlTask);

export interface Instance {
  name: string;
  version: string;
  mods: number;
  loader: boolean;
  loaderName: string;
  lastPlayed: string;
  active: boolean;
}

export interface Account {
  icon: string;
  nameZh: string;
  nameEn: string;
  statusZh: string;
  statusEn: string;
  active: boolean;
  type: string;
  /** 离线账户的 UUID（MD5("OfflinePlayer:"+name)，由后端 offline_uuid 计算） */
  uuid?: string;
}

const instances = ref<Instance[]>([]);
const accounts = ref<Account[]>([]);

// ── 拖放安装整合包 (PCL/HMCL-style) ──
const dragOver = ref(false);
const dragFileName = ref("");
const dragFilePath = ref("");
const dragInstalling = ref(false);
/** 拖放安装页的返回目标 */
const dropReturnTo = ref("dashboard");
const dropInstanceName = ref("");

async function handleFileDrop(paths: string[]) {
  if (dragInstalling.value) return;
  const file = paths[0] || "";
  const lower = file.toLowerCase();
  dragOver.value = false;
  // 只接受整合包；以前拖 .jar 进来是完全静默的，用户不知道发生了什么
  if (!lower.endsWith(".mrpack") && !lower.endsWith(".zip")) {
    dlToast.value = "只支持拖入 .mrpack / .zip 整合包；单个 Mod 请到「获取内容」安装";
    setTimeout(() => { dlToast.value = ""; }, 4000);
    return;
  }

  dragFilePath.value = file;
  dragFileName.value = file.split("\\").pop() || file;
  // 默认实例名 = 文件名去扩展名
  dropInstanceName.value = dragFileName.value.replace(/\.(mrpack|zip)$/i, "");
  if (currentPage.value !== "dropInstall") dropReturnTo.value = currentPage.value;
  currentPage.value = "dropInstall";
}

async function confirmDropInstall() {
  if (!dropInstanceName.value.trim()) return;
  const name = dropInstanceName.value.trim();
  dragInstalling.value = true;

  const taskId = addDlTask(name);
  updateDlTask(taskId, "准备安装...");
  currentPage.value = "dlTasks";

  // ★ 直接监听进度事件，更新任务状态
  //
  // 事件 schema 见 src-tauri/src/progress.rs；filename 是旧字段，
  // 仅用于兼容尚未迁移的发射点。少了这层兜底，拖放安装会完全收不到阶段文案。
  type ProgressPayload = { kind?: string; file_name?: string; filename?: string; step?: string };
  const unlisten = await listen<ProgressPayload>("download-progress", (event) => {
    const p = event.payload;
    const taskName = p.file_name ?? p.filename ?? "";
    if (taskName === name && p.step) {
      updateDlTask(taskId, p.step);
    }
  });

  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    await invoke<string>("install_local_modpack", {
      mcDir, filePath: dragFilePath.value, packName: name,
    });
    updateDlTask(taskId, "安装完成");
    finishDlTask(taskId);
    instances.value = await invoke<Instance[]>("list_instances", { mcDir });
  } catch (e: any) {
    updateDlTask(taskId, "安装失败");
    finishDlTask(taskId, String(e));
    // 释放期不能用 alert（会卡住 webview），统一走顶部 toast
    dlToast.value = "安装失败: " + String(e);
    setTimeout(() => { dlToast.value = ""; }, 6000);
  }
  unlisten();
  dragInstalling.value = false;
  dragFilePath.value = "";
  dragFileName.value = "";
}

/** 来自「本地整合包」按钮：走与拖放完全相同的流程 */
function onLocalPack(path: string) {
  handleFileDrop([path]);
}

function cancelDropInstall() {
  currentPage.value = dropReturnTo.value;
  dragFilePath.value = "";
  dragFileName.value = "";
}

let unlistenDrop: (() => void) | undefined;

// 拖放 hover 事件
const onDragOverHandler = (e: DragEvent) => {
  e.preventDefault();
  if (e.dataTransfer?.types.includes("Files")) {
    dragOver.value = true;
    dragFileName.value = e.dataTransfer?.files[0]?.name || "";
  }
};
const onDragLeaveHandler = () => { dragOver.value = false; };

/** 列表密度（P2-8）：紧凑 / 舒适 —— 挂在 <html data-density> 上，CSS 用变量切换行高 */
async function applyDensity() {
  try {
    const cfg = await readUserConfig();
    const d = String(cfg.list_density || "comfort");
    document.documentElement.dataset.density = d === "compact" ? "compact" : "comfort";
  } catch {
    document.documentElement.dataset.density = "comfort";
  }
}

onMounted(async () => {
  void applyDensity();

  // 监听 Tauri v2 拖放事件
  // Tauri v2 的拖放事件挂在 webview 上（Window.onDragDropEvent 已废弃）。
  // 注意：dragDropEnabled=true 时原生接管，HTML5 的 dragover 不会触发；
  // 而 Windows 的 UIPI 会拦截「资源管理器(Medium) → 本应用(High/管理员)」的拖放，
  // 表现为光标变成禁止符，此时请用「本地整合包」按钮选择文件。
  try {
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      const p = event.payload;
      if (p.type === "enter") {
        if (dragInstalling.value) return;
        const first = (p as any).paths?.[0] || "";
        dragFileName.value = first.split("\\").pop() || first;
        dragOver.value = true;
      } else if (p.type === "over") {
        if (!dragInstalling.value) dragOver.value = true;
      } else if (p.type === "leave") {
        dragOver.value = false;
      } else if (p.type === "drop") {
        dragOver.value = false;
        const paths: string[] = (p as any).paths || [];
        if (paths.length > 0) handleFileDrop(paths);
      }
    });
    console.log("[drop] webview drag-drop listener registered");
  } catch (e) {
    console.warn("[drop] Failed to register:", e);
  }
  window.addEventListener("dragover", onDragOverHandler);
  window.addEventListener("dragleave", onDragLeaveHandler);

  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const [list, userCfg] = await Promise.all([
      invoke<Instance[]>("list_instances", { mcDir }),
      readUserConfig(),
    ]);
    instances.value = list;
    // active 由后端按 .teas/tc.ini 的 active_instance 计算，这里不再重复映射，
    // 避免两个真相来源（先前客户端的重映射与后端的枚举顺序经常不一致）
    if (userCfg.accounts && Array.isArray(userCfg.accounts)) {
      accounts.value = userCfg.accounts;
      // 迁移: 旧版本创建的离线账户没有 uuid 字段，补算一次并回写，
      // 否则仪表盘的 UUID 一栏永远只显示占位符
      let changed = false;
      for (const a of accounts.value as Account[]) {
        if (a.type === "offline" && !a.uuid && a.statusZh) {
          try {
            a.uuid = await invoke<string>("offline_uuid", { playerName: a.statusZh });
            changed = true;
          } catch { /* 保持无 uuid */ }
        }
      }
      if (changed) {
        await writeUserConfig("accounts", accounts.value);
      }
    }
  } catch { /* */ }

  // 下载进度事件由 DownloadsTaskPage 管理（聚合进度 + per-file 详情）
});

onUnmounted(() => {
  if (unlistenDrop) unlistenDrop();
  window.removeEventListener("dragover", onDragOverHandler);
  window.removeEventListener("dragleave", onDragLeaveHandler);
});


const activeInstance = computed(() =>
  instances.value.find((i) => i.active)
  ?? instances.value[0]
  ?? { name: "无实例", version: "---", mods: 0, loader: false, loaderName: "Vanilla", lastPlayed: "从未", active: true }
);

const activeAccount = computed(() =>
  accounts.value.find((a) => a.active)
);

provide("instances", instances);
provide("activeInstance", activeInstance);
provide("accounts", accounts);
provide("activeAccount", activeAccount);  // ComputedRef — 在模板中自动解包，script 中用 .value

/** 只设为当前实例，不跳页（实例行单击） */
async function selectInstanceOnly(inst: Instance) {
  instances.value = instances.value.map(i => ({ ...i, active: i.name === inst.name })) as Instance[];
  await writeLauncherConfig("active_instance", inst.name);
}

/** 设为当前实例并跳到仪表盘（菜单「启动」/ 双击行） */
async function selectAndGo(inst: Instance) {
  await selectInstanceOnly(inst);
  currentPage.value = "dashboard";
}

async function setActiveAccount(index: number) {
  const updated = accounts.value.map((a, i) => ({ ...a, active: i === index }));
  accounts.value = updated;
  await writeUserConfig("accounts", accounts.value);
}

provide("setActiveAccount", setActiveAccount);

const pageComponent = computed(() => {
  const map: Record<string, any> = {
    dashboard: DashboardPage,
    instances: InstancesPage,
    downloads: DownloadsPage,
    accounts: AccountsPage,
    settings: SettingsPage,
    debug: DebugPage,
    dlTasks: DownloadsTaskPage,
  };
  return map[currentPage.value];
});

function navigate(page: string) {
  currentPage.value = page;
}
</script>

<template>
  <Sidebar :current-page="currentPage" @navigate="navigate" />
  <main class="main-area" style="position:relative">
    <Transition name="dl-view">
      <div v-if="dlToast" class="dl-toast">{{ dlToast }}</div>
    </Transition>
    <StatusBar />
    <Transition name="page" mode="out-in">
      <!-- 拖放安装：独立页面（不再是弹窗） -->
      <section v-if="currentPage === 'dropInstall'" key="dropInstall" class="page active">
        <div class="page-header">
          <button class="dl-back-btn" @click="cancelDropInstall">
            ← <span class="bl-zh">返回</span><span class="bl-en">BACK</span>
          </button>
          <h2 class="page-title">
            <span class="bl-zh">安装整合包</span>
            <span class="bl-en">INSTALL MODPACK</span>
          </h2>
          <span class="page-line"></span>
        </div>

        <div class="dl-install-view">
          <div class="dl-install-grid">
            <div class="dl-install-main">
              <div class="dl-install-hero">
                <span class="dl-install-hero-label">LOCAL PACK</span>
                <span class="dl-install-hero-ver">{{ dragFileName || '未选择文件' }}</span>
              </div>

              <div class="dl-modpack-file">
                <span class="dl-modpack-file-icon">◫</span>
                <span class="dl-modpack-file-name">{{ dragFilePath }}</span>
              </div>

              <p class="dl-install-hint">
                这是本地文件，安装过程与在线整合包一致；进度可在「任务列表」查看。
              </p>
              <div class="dl-modpack-steps">
                <div class="dl-modpack-step"><b>01</b><span>解压整合包</span></div>
                <div class="dl-modpack-step"><b>02</b><span>同步 overrides</span></div>
                <div class="dl-modpack-step"><b>03</b><span>按清单拉取模组</span></div>
                <div class="dl-modpack-step"><b>04</b><span>安装对应加载器</span></div>
              </div>
            </div>

            <aside class="dl-install-side">
              <div class="dl-side-title">安装摘要<span>SUMMARY</span></div>
              <label class="dl-side-field">
                <span class="dl-side-label">实例名称</span>
                <input class="form-input" v-model="dropInstanceName"
                  @keyup.enter="dropInstanceName.trim() && confirmDropInstall()"
                  placeholder="输入实例名称" />
              </label>
              <div class="dl-side-row"><span>来源文件</span><b class="dl-side-path">{{ dragFileName }}</b></div>
              <div class="dl-side-row"><span>目标目录</span><b class="dl-side-path">versions/{{ dropInstanceName.trim() || '…' }}</b></div>
              <div class="iset-page-actions">
                <button class="acct-new-btn iset-cancel" @click="cancelDropInstall">取消</button>
                <button class="acct-new-btn" :disabled="!dropInstanceName.trim() || dragInstalling" @click="confirmDropInstall">
                  开始安装
                </button>
              </div>
            </aside>
          </div>
        </div>
      </section>
      <KeepAlive v-else>
        <component :is="pageComponent" :key="currentPage" @select-instance="selectInstanceOnly" @start-instance="selectAndGo" @navigate="navigate" @local-pack="onLocalPack" />
      </KeepAlive>
    </Transition>

    <!-- 拖放整合包浮层提示 -->
    <Transition name="modal">
      <div v-if="dragOver && !dragInstalling" class="drop-hint">
        <span style="font-size:28px">📦</span>
        <span class="bl-zh" style="font-size:14px;font-weight:600;color:var(--accent)">松开以安装整合包</span>
        <span class="bl-en" style="font-size:11px;color:var(--text-dim)">{{ dragFileName }}</span>
      </div>
    </Transition>

  </main>
</template>

<style scoped>
.drop-hint{position:fixed;left:50%;bottom:40px;transform:translateX(-50%);display:flex;flex-direction:column;align-items:center;gap:6px;background:var(--bg-panel);border:2px dashed var(--accent);border-radius:10px;padding:16px 32px;text-align:center;z-index:100;pointer-events:none}
</style>
