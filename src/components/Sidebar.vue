<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, inject, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DownloadTask } from "../App.vue";

defineProps<{ currentPage: string }>();
const emit = defineEmits<{ navigate: [page: string] }>();

// 下载任务数（用于角标）
const dlTasks = inject<Ref<DownloadTask[]>>("dlTasks", ref([]) as any);
const dlCount = computed(() => dlTasks.value.length);

const navItems = [
  { id: "dashboard", zh: "仪表盘", en: "DASHBOARD" },
  { id: "instances", zh: "实例列表", en: "INSTANCES" },
  { id: "dlTasks", zh: "下载任务", en: "DOWNLOADS", badge: dlCount },
  { id: "downloads", zh: "获取内容", en: "BROWSE" },
  { id: "accounts", zh: "账户管理", en: "ACCOUNTS" },
  { id: "settings", zh: "设置", en: "SETTINGS" },
];

const netLevel = ref<"ok" | "warn" | "err">("ok");
const netZh = ref("系统正常");
const netEn = ref("SYS OK");
const netDetail = ref("");
let netTimer: number;

async function checkNet() {
  try {
    const result = await invoke<{ ok: number; total: number; failures: string[] }>("check_network");
    if (result.failures.length === 0) {
      netLevel.value = "ok"; netZh.value = "系统正常"; netEn.value = "SYS OK"; netDetail.value = "";
    } else if (result.ok === 0) {
      netLevel.value = "err"; netZh.value = "无网络连接"; netEn.value = "NO NETWORK";
      netDetail.value = result.failures.join(" / ");
    } else {
      netLevel.value = "warn"; netZh.value = "部分服务异常"; netEn.value = "PARTIAL OUTAGE";
      netDetail.value = result.failures.join(" / ");
    }
  } catch {
    netLevel.value = "err"; netZh.value = "检测失败"; netEn.value = "CHECK FAILED";
  }
}

const devMode = ref(false);
const appVersion = ref("v0.1.1");

onMounted(async () => {
  checkNet();
  netTimer = window.setInterval(checkNet, 30000);
  try { devMode.value = await invoke<boolean>("is_dev"); } catch { /* */ }
  try { appVersion.value = await invoke<string>("get_full_version"); } catch { /* */ }
});
onUnmounted(() => clearInterval(netTimer));
</script>

<template>
  <nav class="sidebar">
    <div class="sidebar-brand" data-tauri-drag-region>
      <svg class="logo-hex" viewBox="0 0 24 24" width="24" height="24">
        <path d="M12 2L3 7v10l9 5 9-5V7L12 2z" fill="none" stroke="currentColor" stroke-width="1.2"/>
      </svg>
      <span class="sidebar-title">TEAS</span>
      <div class="brand-ver-group">
        <span class="brand-version">{{ appVersion }}</span>
        <span class="brand-sub">Launcher</span>
      </div>
    </div>

    <div class="nav-group">
      <button v-for="item in navItems" :key="item.id"
        :class="['nav-item', { active: currentPage === item.id, 'has-badge': item.badge && item.badge > 0 }]"
        @click="emit('navigate', item.id)">
        <span>
          <span class="bl-zh">{{ item.zh }}</span>
          <span class="bl-en">{{ item.en }}</span>
        </span>
        <span v-if="item.badge && item.badge > 0" class="nav-badge">{{ item.badge }}</span>
      </button>

      <button v-if="devMode" :class="['nav-item', { active: currentPage === 'debug' }]" @click="emit('navigate', 'debug')">
        <span><span class="bl-zh">调试面板</span><span class="bl-en">DEBUG</span></span>
      </button>
    </div>

    <div class="sidebar-footer" data-tauri-drag-region>
      <div class="sys-status" :class="netLevel">
        <span class="sys-dot" :class="netLevel"></span>
        <div class="sys-info">
          <span class="sys-label">
            <span class="bl-zh">{{ netZh }}</span>
            <span class="bl-en">{{ netEn }}</span>
          </span>
          <span class="sys-detail" v-if="netDetail">{{ netDetail }}</span>
        </div>
      </div>
    </div>
  </nav>
</template>
