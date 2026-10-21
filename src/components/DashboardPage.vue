<script setup lang="ts">
import { ref, inject, computed, onActivated, onDeactivated, type ComputedRef, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Instance, Account } from "../App.vue";
import { gameSourceParam } from "../utils/source";
import { readUserConfig } from "../utils/config";
import { readLaunchSettings } from "../utils/instanceSettings";

const activeInstance = inject<ComputedRef<Instance>>("activeInstance")!;
const activeAccount = inject<ComputedRef<Account | undefined>>("activeAccount")!;
const instances = inject<Ref<Instance[]>>("instances", ref([]));

/** P1-4：没有账户/没有实例时，主按钮要直接给出下一步动作，而不是点了才报错 */
const hasAccount = computed(() => !!activeAccount?.value);
const hasInstance = computed(() => instances.value.length > 0);

const playerName = computed(() => activeAccount?.value?.statusZh ?? "---");
/**
 * 账户 UUID。Account 的 nameZh 存的是账户**类型**名（"离线账户"/"外置验证"），
 * 不是 UUID —— 之前这里直接用了 nameZh，于是"UUID"一栏显示的其实是账户类型。
 */
const playerUuid = computed(() => activeAccount?.value?.uuid ?? "");
/** 认证状态：离线账户只是本地标识，不该标成"已认证" */
const authState = computed(() => {
  const a = activeAccount?.value;
  if (!a) return { zh: "未登录", en: "NOT AUTHENTICATED" };
  if (a.type === "offline") return { zh: "离线模式", en: "OFFLINE MODE" };
  return { zh: "已认证", en: "AUTHENTICATED" };
});

const emit = defineEmits<{ "navigate": [page: string] }>();

const addDlTask = inject<(name: string) => number>("addDlTask", () => 0);
const updateDlTask = inject<(id: number, status: string) => void>("updateDlTask", () => {});
const finishDlTask = inject<(id: number, error?: string) => void>("finishDlTask", () => {});
const isLaunching = inject<Ref<boolean>>("isLaunching", ref(false));
const isRunning = inject<Ref<boolean>>("isRunning", ref(false));

const launchStatus = ref("");
/// 启动失败的具体原因。**不要**塞进 launchStatus——那个值会直接显示在启动按钮里，
/// 长错误会把按钮撑成一堵字墙（实测触发过一次，非常难看）。
const launchError = ref("");
/// 详情弹窗。卡片里只放一行摘要，完整说明在弹窗里看——长文本塞进仪表盘会把布局顶坏。
const launchErrorDetail = ref(false);

function errorSummary(msg: string) {
  const line = msg.split("\n").find((l) => l.trim()) || "";
  return line.length > 44 ? line.slice(0, 44) + "…" : line;
}
const showNoAccount = ref(false);
const crashInfo = ref<{ reason: string; log_path: string; crash_dir?: string } | null>(null);

const cpuUsage = ref(0);
const memUsage = ref(0);
let statsTimer: number | null = null;
let runningTimer: number | null = null;

const displayMemory = ref("4096 MB");
/** 内存被该实例的实例设置覆写了（UI 上标记出来，免得用户以为改了全局没生效） */
const memoryOverridden = ref(false);

onActivated(async () => {
  // 每次激活都刷新：内存可能刚在「设置」页或实例设置里改过，运行状态也可能
  // 在其他页面发生变化。此前只在首次挂载读取一次，导致改了内存后仪表盘仍显示旧值。
  try {
    const mcDir = await invoke<string>("get_minecraft_dir").catch(() => "");
    const [running, settings] = await Promise.all([
      invoke<boolean>("is_instance_running").catch(() => false),
      mcDir
        ? readLaunchSettings(mcDir, activeInstance.value.name)
        : Promise.resolve(null),
    ]);
    isRunning.value = running;
    if (settings) {
      // 显示生效值而不是全局值：实例覆写后仪表盘必须反映真正会用的内存
      displayMemory.value = settings.maxMemory;
      memoryOverridden.value = settings.memoryOverridden;
    }
  } catch { /* */ }

  // ★ 启动轮询（每次激活都启动）
  startPolling();
});

onDeactivated(() => {
  // ★ 离开页面时停止所有轮询
  stopPolling();
});

function startPolling() {
  stopPolling(); // 防止重复启动
  pollStats();
  pollRunning();
}

function stopPolling() {
  if (statsTimer !== null) { clearTimeout(statsTimer); statsTimer = null; }
  if (runningTimer !== null) { clearTimeout(runningTimer); runningTimer = null; }
}

async function pollStats() {
  try {
    const [cpu, mem] = await invoke<[number, number]>("get_system_stats");
    cpuUsage.value = Math.round(cpu);
    memUsage.value = Math.round(mem);
  } catch (_) { /* ignore */ }
  // ★ 递归 setTimeout: 等当前调用完成后再等 2 秒
  statsTimer = window.setTimeout(pollStats, 2000);
}

async function pollRunning() {
  if (isRunning.value) {
    try {
      const still = await invoke<boolean>("is_instance_running");
      if (!still) {
        isRunning.value = false;
        try {
          const mcDir = await invoke<string>("get_minecraft_dir");
          const crash = await invoke<any>("check_crash", { mcDir, instanceName: activeInstance.value.name });
          if (crash) crashInfo.value = crash;
        } catch { /* */ }
      }
    } catch { /* */ }
  }
  runningTimer = window.setTimeout(pollRunning, 5000);
}

async function launch() {
  if (!activeAccount?.value) { showNoAccount.value = true; return; }
  isLaunching.value = true;
  launchError.value = "";
  launchErrorDetail.value = false;
  launchStatus.value = "准备中...";
  const mcDir = await invoke<string>("get_minecraft_dir");
  const instName = activeInstance.value.name;
  const playerName = activeAccount.value.statusZh;

  const taskId = addDlTask(instName);
  try {
    const cfg = await readUserConfig();
    // 实例设置里的覆盖项在这里生效（留空的字段自动回退到全局值）
    const s = await readLaunchSettings(mcDir, instName);
    launchStatus.value = "检查完整性...";
    updateDlTask(taskId, "检查文件完整性");
    // 用用户配置的游戏源，不要把 mod 源或写死的值传进来
    await invoke("fix_instance", { mcDir, instanceName: instName, source: gameSourceParam(cfg) });

    launchStatus.value = "启动中...";
    updateDlTask(taskId, "正在启动游戏");
    await invoke("launch_instance", {
      mcDir, instanceName: instName, playerName,
      javaPath: s.javaPath,
      maxMemory: s.maxMemory,
      jvmArgs: s.jvmArgs,
      windowWidth: s.windowWidth,
      windowHeight: s.windowHeight,
    });
    updateDlTask(taskId, "已启动");
  } catch (e) {
    console.error("Launch error:", e);
    // 按钮只显示四个字的短状态，完整原因进下面的错误面板
    launchStatus.value = "启动失败";
    launchError.value = String(e);
    updateDlTask(taskId, String(e));
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
              <span class="bl-zh">{{ authState.zh }} · {{ playerUuid || '——' }}</span>
              <span class="bl-en">{{ authState.en }}</span>
            </span>
          </div>
          <button
            class="switch-operator-btn"
            :class="{ 'is-cta': !hasAccount }"
            @click="emit('navigate', 'accounts')"
          >
            <span class="bl-zh">{{ hasAccount ? '切换干员' : '添加干员' }}</span>
            <span class="bl-en">{{ hasAccount ? 'SWITCH OPERATOR' : 'ADD OPERATOR' }}</span>
          </button>
          <div v-if="!hasAccount" class="player-cta-hint">
            任意名字即可创建离线账户
          </div>
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
            <!-- 没有账户 / 没有实例时，按钮直接变成"下一步"，而不是点了才弹提示 -->
            <button
              v-if="!hasAccount"
              class="launch-btn-main cta"
              @click="emit('navigate', 'accounts')"
            >
              <span class="lbm-icon">＋</span>
              <span>
                <span class="bl-zh">添加干员</span>
                <span class="bl-en">ADD OPERATOR</span>
              </span>
            </button>
            <button
              v-else-if="!hasInstance"
              class="launch-btn-main cta"
              @click="emit('navigate', 'downloads')"
            >
              <span class="lbm-icon">＋</span>
              <span>
                <span class="bl-zh">获取游戏</span>
                <span class="bl-en">GET A GAME</span>
              </span>
            </button>
            <button
              v-else
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
            <button v-if="launchError" class="launch-error-bar" @click="launchErrorDetail = true">
              <span class="leb-dot"></span>
              <span class="leb-text">{{ errorSummary(launchError) }}</span>
              <span class="leb-more">详情</span>
            </button>
            <div class="launch-info-row">
              <span class="li-item">{{ activeInstance.loaderName.toUpperCase() }}</span>
              <span class="li-sep">//</span>
              <span class="li-item">{{ displayMemory }}</span>
              <span v-if="memoryOverridden" class="li-inst-tag" title="该值来自此实例的实例设置，不是全局设置">实例</span>
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

    <!-- 启动失败详情 -->
    <Transition name="modal">
      <div v-if="launchErrorDetail && launchError" class="modal-overlay" @click.self="launchErrorDetail = false">
        <div class="modal-panel" style="width:520px">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">启动失败</span>
              <span class="bl-en">LAUNCH FAILED</span>
            </span>
            <button class="modal-close" @click="launchErrorDetail = false">✕</button>
          </div>
          <div class="modal-body">
            <pre class="le-body">{{ launchError }}</pre>
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
