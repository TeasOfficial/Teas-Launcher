<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

const mcDir = ref("");
const version = ref("");
const arch = ref("x86_64");
const cpuUsage = ref(0);
const memUsage = ref(0);
const launchArgs = ref<Record<string, any> | null>(null);

// 环境信息（P2-10：下半屏原本是空的）
const teasDir = ref("");
const javaPath = ref("");
const logPath = ref("");
const instances = ref(0);
const accounts = ref(0);
const logLevel = ref("");

onMounted(async () => {
  try { mcDir.value = await invoke<string>("get_minecraft_dir"); } catch { /* */ }
  try { version.value = await invoke<string>("get_full_version"); } catch { /* */ }
  try {
    const [cpu, mem] = await invoke<[number, number]>("get_system_stats");
    cpuUsage.value = Math.round(cpu);
    memUsage.value = Math.round(mem);
  } catch { /* */ }
  // 环境信息
  try { teasDir.value = await invoke<string>("get_teas_dir"); } catch { /* */ }
  try {
    const cfg = await invoke<Record<string, any>>("config_read", { scope: "user" });
    javaPath.value = String(cfg.java_path || "");
    accounts.value = Array.isArray(cfg.accounts) ? cfg.accounts.length : 0;
    logLevel.value = String(cfg.log_level || "INFO");
  } catch { /* */ }
  try {
    const list = await invoke<any[]>("list_instances", { mcDir: mcDir.value || "." });
    instances.value = list.length;
  } catch { /* */ }
  logPath.value = teasDir.value ? `${teasDir.value}\\launcher.log` : "";

  // 启动参数预览
  try {
    launchArgs.value = await invoke<Record<string, any>>("get_launch_args", {
      mcDir: mcDir.value || ".",
      instanceName: "_preview_",
      playerName: "Steve",
      javaPath: "javaw",
      maxMemory: "4G",
      jvmArgs: "-XX:+UseG1GC",
      windowWidth: 854,
      windowHeight: 480,
    });
  } catch { /* */ }
});
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <h2 class="page-title">
        <span class="bl-zh">调试面板</span>
        <span class="bl-en">DEBUG PANEL</span>
      </h2>
      <span class="page-line"></span>
    </div>

    <div class="debug-grid">
      <div class="debug-card">
        <span class="debug-card-label">Minecraft Dir</span>
        <span class="debug-card-val">{{ mcDir }}</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Version</span>
        <span class="debug-card-val">{{ version }}</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">CPU</span>
        <span class="debug-card-val">{{ cpuUsage }}%</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Memory</span>
        <span class="debug-card-val">{{ memUsage }}%</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Dev Mode</span>
        <span class="debug-card-val" style="color:var(--warn)">TRUE</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Arch</span>
        <span class="debug-card-val">{{ arch }}</span>
      </div>
    </div>

    <!-- 环境信息 -->
    <h3 class="debug-section-title">环境信息 / ENVIRONMENT</h3>
    <div class="debug-grid">
      <div class="debug-card debug-card-wide">
        <span class="debug-card-label">Data Dir</span>
        <span class="debug-card-val">{{ teasDir || "—" }}</span>
      </div>
      <div class="debug-card debug-card-wide">
        <span class="debug-card-label">Java Path</span>
        <span class="debug-card-val">{{ javaPath || "—" }}</span>
      </div>
      <div class="debug-card debug-card-wide">
        <span class="debug-card-label">Launcher Log</span>
        <span class="debug-card-val">{{ logPath || "—" }}</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Instances</span>
        <span class="debug-card-val">{{ instances }}</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Accounts</span>
        <span class="debug-card-val">{{ accounts }}</span>
      </div>
      <div class="debug-card">
        <span class="debug-card-label">Log Level</span>
        <span class="debug-card-val">{{ logLevel || "INFO" }}</span>
      </div>
    </div>

    <!-- 启动参数 -->
    <div v-if="launchArgs" class="debug-launch-section">
      <h3 class="debug-section-title">启动参数 / LAUNCH ARGS</h3>
      <div class="debug-args-grid">
        <div class="debug-card" v-for="(val, key) in launchArgs" :key="key">
          <span class="debug-card-label">{{ key }}</span>
          <span class="debug-card-val">{{ val }}</span>
        </div>
      </div>
    </div>
  </section>
</template>
