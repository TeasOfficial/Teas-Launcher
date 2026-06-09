<script setup lang="ts">
import { ref, inject, computed, onMounted, onUnmounted, type ComputedRef, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Instance, Account } from "../App.vue";

const activeInstance = inject<ComputedRef<Instance>>("activeInstance")!;
const activeAccount = inject<ComputedRef<Account | undefined>>("activeAccount")!;

const playerName = computed(() => activeAccount?.value?.statusZh ?? "---");
const playerId = computed(() => activeAccount?.value?.nameZh ?? "未登录");

const emit = defineEmits<{ "navigate": [page: string] }>();

const addDlTask = inject<(name: string) => number>("addDlTask", () => 0);
const updateDlTask = inject<(id: number, status: string) => void>("updateDlTask", () => {});
const finishDlTask = inject<(id: number, error?: string) => void>("finishDlTask", () => {});
const isLaunching = inject<Ref<boolean>>("isLaunching", ref(false));
const isRunning = inject<Ref<boolean>>("isRunning", ref(false));

const launchStatus = ref("");
const showNoAccount = ref(false);
const crashInfo = ref<{ reason: string; log_path: string; crash_dir?: string } | null>(null);

const cpuUsage = ref(0);
const memUsage = ref(0);
let statsTimer: number;

const displayMemory = ref("4096 MB");
const displayJava = ref("Java 21");

onMounted(async () => {
  try { isRunning.value = await invoke<boolean>("is_instance_running"); } catch { /* */ }
  // 从配置读取内存和 Java 信息
  try {
    const cfg = await invoke<Record<string, any>>("config_read", { scope: "user" });
    displayMemory.value = cfg.max_memory || "4096 MB";
    // 用 scan_java 获取准确的 Java 版本
    const javaPath: string = cfg.java_path || "javaw";
    const javaList = await invoke<{ path: string; version: string }[]>("scan_java");
    const norm = (p: string) => p.toLowerCase().replace(/\\/g, '/');
    const match = javaList.find(j => norm(j.path) === norm(javaPath));
    if (match) {
      displayJava.value = `Java ${match.version}`;
    } else {
      // 回退：从路径推测
      const pathMatch = javaPath.match(/[Jj]ava[^/\\]*[/\\]?(\d+)/);
      displayJava.value = pathMatch ? `Java ${pathMatch[1]}` : `Java ${javaList[0]?.version || "?"}`;
    }
  } catch { /* */ }

  const poll = async () => {
    try {
      const [cpu, mem] = await invoke<[number, number]>("get_system_stats");
      cpuUsage.value = Math.round(cpu);
      memUsage.value = Math.round(mem);
    } catch (_) { /* ignore */ }
  };
  poll();
  statsTimer = window.setInterval(poll, 2000);
  setInterval(async () => {
    if (isRunning.value) {
      try {
        const still = await invoke<boolean>("is_instance_running");
        if (!still) {
          // 进程退出了，检查崩溃
          isRunning.value = false;
          try {
            const mcDir = await invoke<string>("get_minecraft_dir");
            const crash = await invoke<any>("check_crash", { mcDir, instanceName: activeInstance.value.name });
            if (crash) crashInfo.value = crash;
          } catch { /* */ }
        }
      } catch { /* */ }
    }
  }, 5000);
});

onUnmounted(() => clearInterval(statsTimer));

async function launch() {
  if (!activeAccount?.value) { showNoAccount.value = true; return; }
  isLaunching.value = true;
  launchStatus.value = "准备中...";
  const mcDir = await invoke<string>("get_minecraft_dir");
  const instName = activeInstance.value.name;
  const playerName = activeAccount.value.statusZh;

  const taskId = addDlTask(instName);
  try {
    const cfg = await invoke<Record<string, any>>("config_read", { scope: "user" });
    launchStatus.value = "检查完整性...";
    updateDlTask(taskId, "检查文件完整性");
    await invoke("fix_instance", { mcDir, instanceName: instName, source: "MCIMirror" });

    launchStatus.value = "启动中...";
    updateDlTask(taskId, "正在启动游戏");
    await invoke("launch_instance", {
      mcDir, instanceName: instName, playerName,
      javaPath: cfg.java_path || "javaw",
      maxMemory: String(cfg.max_memory || "4G"),
      jvmArgs: String(cfg.jvm_args || "-XX:+UseG1GC"),
      windowWidth: parseInt(cfg.window_width) || 854,
      windowHeight: parseInt(cfg.window_height) || 480,
    });
    updateDlTask(taskId, "已启动");
  } catch (e) {
    console.error("Launch error:", e);
    launchStatus.value = `启动失败: ${e}`;
    updateDlTask(taskId, "启动失败");
  }
  finishDlTask(taskId);

  // 等待3秒检测进程状态
  await new Promise(r => setTimeout(r, 3000));
  try { isRunning.value = await invoke<boolean>("is_instance_running"); } catch { /* */ }

  // 进程已退出 → 检查崩溃（只有真正崩溃才弹窗）
  if (!isRunning.value) {
    try {
      const crash = await invoke<any>("check_crash", { mcDir, instanceName: instName });
      if (crash) crashInfo.value = crash;
    } catch { /* */ }
  }
  isLaunching.value = false;
  launchStatus.value = "";
}

async function kill() {
  try { await invoke("kill_instance"); isRunning.value = false; } catch (e) { console.error("Kill failed:", e); }
}
</script>

<template>
  <section class="page active">
    <div class="dashboard-grid">

      <!-- 干员身份 -->
      <div class="panel panel-player" style="grid-area:player">
        <div class="panel-tech-corner"></div>
        <div class="panel-head">
          <span class="panel-head-label">
            <span class="bl-zh">干员身份</span>
            <span class="bl-en">OPERATOR IDENTITY</span>
          </span>
          <span class="panel-head-line"></span>
        </div>
        <div class="panel-body">
          <div class="player-avatar">
            <svg viewBox="0 0 40 40" width="40" height="40">
              <circle cx="20" cy="15" r="6" fill="none" stroke="currentColor" stroke-width="1.2"/>
              <path d="M6 36c0-9 6.3-14 14-14s14 5 14 14" fill="none" stroke="currentColor" stroke-width="1.2"/>
            </svg>
          </div>
          <div class="player-name">{{ playerName }}</div>
          <div>
            <span class="pi-label">UUID</span>
            <span class="pi-val">
              <span class="bl-zh">{{ playerId }}</span>
              <span class="bl-en">{{ playerId === '未登录' ? 'NOT AUTHENTICATED' : 'AUTHENTICATED' }}</span>
            </span>
          </div>
          <button class="switch-operator-btn" @click="emit('navigate', 'accounts')">
            <span class="bl-zh">切换干员</span>
            <span class="bl-en">SWITCH OPERATOR</span>
          </button>
        </div>
      </div>

      <!-- 右侧主区域 -->
      <div style="grid-area:main;display:flex;flex-direction:column;gap:10px">

        <!-- 当前实例 -->
        <div class="panel" style="flex:1">
          <div class="panel-tech-corner"></div>
          <div class="panel-head">
            <span class="panel-head-label">
              <span class="bl-zh">当前实例</span>
              <span class="bl-en">ACTIVE INSTANCE</span>
            </span>
            <span class="panel-head-line"></span>
          </div>
          <div class="panel-body" style="align-items:center;justify-content:center">
            <div class="instance-name-display">{{ activeInstance.name }}</div>
            <div class="instance-ver-sub">{{ activeInstance.version }}</div>
          </div>
        </div>

        <!-- 启动区 -->
        <div class="panel panel-launch" style="flex:2">
          <div class="panel-tech-corner"></div>
          <div class="panel-tech-corner tr"></div>
          <div class="launch-content">
            <div class="launch-version-row">
              <span class="lv-dot"></span>
              <span class="lv-text">MINECRAFT {{ activeInstance.version }}</span>
            </div>
            <button
              class="launch-btn-main"
              :class="{ launching: isLaunching, running: isRunning }"
              :disabled="isLaunching"
              @click="isRunning ? kill() : launch()"
            >
              <span class="lbm-icon">{{ isRunning ? '■' : '▶' }}</span>
              <span>
                <span class="bl-zh">{{ isLaunching ? launchStatus || '启动中...' : isRunning ? '序列运行中' : '初始化序列' }}</span>
                <span class="bl-en">{{ isLaunching ? 'LAUNCHING...' : isRunning ? 'SEQUENCE ACTIVE' : 'INITIALIZE SEQUENCE' }}</span>
              </span>
            </button>
            <div class="launch-info-row">
              <span class="li-item">{{ activeInstance.loaderName.toUpperCase() }}</span>
              <span class="li-sep">//</span>
              <span class="li-item">{{ displayMemory }}</span>
              <span class="li-sep">//</span>
              <span class="li-item">{{ displayJava }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 遥测条 -->
      <div class="panel" style="grid-area:telemetry">
        <div class="telemetry-row">
          <div class="tm-item">
            <span class="tm-label">CPU</span>
            <div class="tm-bar"><div class="tm-fill" :style="{width:cpuUsage+'%'}"></div></div>
            <span class="tm-val">{{ cpuUsage }}%</span>
          </div>
          <div class="tm-item">
            <span class="tm-label">MEM</span>
            <div class="tm-bar"><div class="tm-fill" :style="{width:memUsage+'%'}"></div></div>
            <span class="tm-val">{{ memUsage }}%</span>
          </div>
        </div>
      </div>

    </div>

    <!-- 未选择账户提示 -->
    <Transition name="modal">
      <div v-if="showNoAccount" class="modal-overlay" @click.self="showNoAccount = false">
        <div class="modal-panel" style="width:360px">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">未选择账户</span>
              <span class="bl-en">NO ACCOUNT</span>
            </span>
            <button class="modal-close" @click="showNoAccount = false">✕</button>
          </div>
          <div class="modal-body">
            <p class="modal-desc">请先在账户管理中创建或选择一个账户。</p>
            <p class="modal-desc-sub">Please create or select an account first.</p>
          </div>
        </div>
      </div>
    </Transition>

    <!-- 崩溃提示 -->
    <Transition name="modal">
      <div v-if="crashInfo" class="modal-overlay" @click.self="crashInfo = null">
        <div class="modal-panel" style="width:500px">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">游戏崩溃</span>
              <span class="bl-en">CRASH DETECTED</span>
            </span>
            <button class="modal-close" @click="crashInfo = null">✕</button>
          </div>
          <div class="modal-body">
            <p class="modal-desc" style="color:var(--danger);white-space:pre-line;max-height:250px;overflow-y:auto;font-size:12px;line-height:1.5">{{ crashInfo.reason }}</p>
            <div class="form-field" v-if="crashInfo.log_path">
              <span class="form-label">日志路径 / Log Path</span>
              <span class="debug-card-val" style="font-size:10px;word-break:break-all;white-space:pre-line">{{ crashInfo.log_path }}</span>
            </div>
            <div class="form-field" v-if="crashInfo.crash_dir">
              <span class="form-label">崩溃报告目录</span>
              <span class="debug-card-val" style="font-size:10px;word-break:break-all">{{ crashInfo.crash_dir }}</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </section>
</template>
