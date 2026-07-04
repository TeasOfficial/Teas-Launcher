<script setup lang="ts">
import { ref, computed, provide, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
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
}

const instances = ref<Instance[]>([]);
const accounts = ref<Account[]>([]);

// ── 拖放安装整合包 (PCL/HMCL-style) ──
const dragOver = ref(false);
const dragFileName = ref("");
const dragFilePath = ref("");
const dragInstalling = ref(false);
const showDropDialog = ref(false);
const dropInstanceName = ref("");

async function handleFileDrop(paths: string[]) {
  if (dragInstalling.value) return;
  const file = paths[0] || "";
  const ext = file.toLowerCase();
  if (!ext.endsWith(".mrpack") && !ext.endsWith(".zip")) return;

  dragOver.value = false;
  dragFilePath.value = file;
  dragFileName.value = file.split("\\").pop() || file;
  // 默认实例名 = 文件名去扩展名
  dropInstanceName.value = dragFileName.value.replace(/\.(mrpack|zip)$/i, "");
  showDropDialog.value = true;
}

async function confirmDropInstall() {
  if (!dropInstanceName.value.trim()) return;
  const name = dropInstanceName.value.trim();
  showDropDialog.value = false;

  const taskId = addDlTask(name);
  updateDlTask(taskId, "准备安装...");
  currentPage.value = "dlTasks";

  // ★ 直接监听进度事件，更新任务状态
  type ProgressPayload = { filename: string; step?: string; percent?: number };
  const unlisten = await listen<ProgressPayload>("download-progress", (event) => {
    const p = event.payload;
    if (p.filename === name && p.step) {
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
    alert("安装失败: " + String(e));
  }
  unlisten();
  dragFilePath.value = "";
  dragFileName.value = "";
}

let unlistenDrop: (() => void) | undefined;
let unlistenDragOver: (() => void) | undefined;

// 拖放 hover 事件
const onDragOverHandler = (e: DragEvent) => {
  e.preventDefault();
  if (e.dataTransfer?.types.includes("Files")) {
    dragOver.value = true;
    dragFileName.value = e.dataTransfer?.files[0]?.name || "";
  }
};
const onDragLeaveHandler = () => { dragOver.value = false; };

onMounted(async () => {
  // 监听 Tauri v2 拖放事件
  try {
    const win = getCurrentWindow();
    unlistenDrop = await win.onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        const paths: string[] = (event.payload as any).paths || [];
        if (paths.length > 0) handleFileDrop(paths);
      }
    });
    console.log("[drop] drag-drop listener registered");
  } catch (e) {
    console.warn("[drop] Failed to register:", e);
  }
  window.addEventListener("dragover", onDragOverHandler);
  window.addEventListener("dragleave", onDragLeaveHandler);

  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const [list, launcherCfg, userCfg] = await Promise.all([
      invoke<Instance[]>("list_instances", { mcDir }),
      invoke<Record<string, any>>("config_read", { scope: "launcher" }),
      invoke<Record<string, any>>("config_read", { scope: "user" }),
    ]);
    instances.value = list;
    const saved = launcherCfg.active_instance as string | undefined;
    if (saved) {
      instances.value = instances.value.map(i =>
        ({ ...i, active: i.name === saved })
      ) as Instance[];
    }
    if (userCfg.accounts && Array.isArray(userCfg.accounts)) {
      accounts.value = userCfg.accounts;
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

async function selectAndGo(inst: Instance) {
  instances.value = instances.value.map(i => ({ ...i, active: i.name === inst.name })) as Instance[];
  currentPage.value = "dashboard";
  try { await invoke("config_write", { scope: "launcher", key: "active_instance", value: inst.name }); } catch { /* */ }
}

async function setActiveAccount(index: number) {
  const updated = accounts.value.map((a, i) => ({ ...a, active: i === index }));
  accounts.value = updated;
  try {
    await invoke("config_write", { scope: "user", key: "accounts", value: accounts.value });
  } catch { /* */ }
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
      <KeepAlive>
        <component :is="pageComponent" :key="currentPage" @select-instance="selectAndGo" @navigate="navigate" />
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

    <!-- 安装确认对话框 -->
    <Transition name="modal">
      <div v-if="showDropDialog" class="modal-overlay" @click.self="showDropDialog = false">
        <div class="modal-panel" style="width:420px;padding:24px">
          <div class="modal-header">
            <span class="bl-zh">安装整合包</span>
            <span class="bl-en">INSTALL MODPACK</span>
          </div>
          <div style="display:flex;flex-direction:column;gap:12px;margin-top:16px">
            <div>
              <span class="bl-zh" style="font-size:12px;color:var(--text-dim)">文件名</span>
              <div style="font-size:13px;color:var(--secondary);word-break:break-all">{{ dragFileName }}</div>
            </div>
            <div>
              <span class="bl-zh" style="font-size:12px;color:var(--text-dim)">实例名称</span>
              <input class="form-input" v-model="dropInstanceName" @keyup.enter="confirmDropInstall"
                style="width:100%;margin-top:4px;font-size:14px" placeholder="输入实例名称" />
            </div>
          </div>
          <div class="modal-actions" style="margin-top:20px;justify-content:flex-end">
            <button class="acct-new-btn" @click="showDropDialog = false" style="color:var(--text-dim);border-color:var(--border)">取消</button>
            <button class="acct-new-btn" @click="confirmDropInstall" :disabled="!dropInstanceName.trim()">确认安装</button>
          </div>
        </div>
      </div>
    </Transition>
  </main>
</template>

<style scoped>
.drop-hint{position:fixed;left:50%;bottom:40px;transform:translateX(-50%);display:flex;flex-direction:column;align-items:center;gap:6px;background:var(--bg-panel);border:2px dashed var(--accent);border-radius:10px;padding:16px 32px;text-align:center;z-index:100;pointer-events:none}
</style>
