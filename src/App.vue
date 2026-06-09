<script setup lang="ts">
import { ref, computed, provide, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
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

onMounted(async () => {
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    instances.value = await invoke<Instance[]>("list_instances", { mcDir });
    // 恢复上次选中的实例
    const cfg = await invoke<Record<string, any>>("config_read", { scope: "launcher" });
    const saved = cfg.active_instance as string | undefined;
    if (saved) {
      instances.value = instances.value.map(i =>
        ({ ...i, active: i.name === saved })
      ) as Instance[];
    }
  } catch { /* */ }
  try {
    const cfg = await invoke<Record<string, any>>("config_read", { scope: "user" });
    if (cfg.accounts && Array.isArray(cfg.accounts)) {
      accounts.value = cfg.accounts;
    }
  } catch { /* */ }

  // 下载进度事件由 DownloadsTaskPage 管理（聚合进度 + per-file 详情）
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
      <component :is="pageComponent" :key="currentPage" @select-instance="selectAndGo" @navigate="navigate" />
    </Transition>
  </main>
</template>
