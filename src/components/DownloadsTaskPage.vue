<script setup lang="ts">
const emit = defineEmits<{ navigate: [page: string] }>();
import { inject, ref, type Ref, onActivated, onDeactivated } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DownloadTask } from "../App.vue";

const dlTasks = inject<Ref<DownloadTask[]>>("dlTasks")!;
const dlHistory = inject<Ref<DownloadTask[]>>("dlHistory")!;
const finishDlTask = inject<(id: number, error?: string) => void>("finishDlTask", () => { console.error("[DL] finishDlTask 注入失败!"); });

// ── Per-file progress tracking ─────────────────────────────────
interface FileProgress {
  file_name: string;
  file_percent: number;
  file_speed: number;   // B/s
  file_downloaded: number;
  file_total: number;
}

// batch_id → task_id 映射表（同一个 batch_id 可能对应多个 task）
const batchTaskMap = new Map<number, number>();

// taskId → 聚合进度信息
interface AggregatedProgress {
  batch_id: number;
  percent: number;        // 0-100
  total_speed: number;    // B/s (所有活跃文件之和)
  total_bytes: number;
  downloaded_bytes: number;
  files: Record<string, FileProgress>;  // fileName → per-file progress
  batch_done: number;
  batch_total: number;
  /** 当前真正在传输的文件数（后端 total_speed 的样本数） */
  running: number;
  /** 阶段文案（解压 / 安装加载器 …），仅 stage 事件会填 */
  step: string;
}
const progressMap = ref<Record<number, AggregatedProgress>>({});

const expandedTasks = ref<Set<number>>(new Set());
/** 剩余时间估算：总量 - 已下载，除以当前总速度 */
function etaText(a: AggregatedProgress | undefined): string {
  if (!a || a.total_speed <= 0 || a.total_bytes <= 0) return "";
  const remain = a.total_bytes - a.downloaded_bytes;
  if (remain <= 0) return "";
  const sec = remain / a.total_speed;
  if (!isFinite(sec) || sec <= 0) return "";
  if (sec < 60) return `剩余 ${Math.ceil(sec)} 秒`;
  if (sec < 3600) return `剩余 ${Math.ceil(sec / 60)} 分钟`;
  return `剩余 ${(sec / 3600).toFixed(1)} 小时`;
}

function toggleDetails(taskId: number) {
  const next = new Set(expandedTasks.value);
  if (next.has(taskId)) next.delete(taskId); else next.add(taskId);
  expandedTasks.value = next;
}

let unlisten: UnlistenFn | null = null;

/**
 * 后端进度事件（schema 见 src-tauri/src/progress.rs）。
 * 旧字段名 filename / percent 仅作兜底：以前 install/、modpack 等各自 emit
 * 过一套格式，缺 file_name 时下面的字符串匹配会抛 TypeError，监听器直接断掉。
 */
interface ProgressPayload {
  kind?: "stage" | "file";
  batch_id?: number;
  batch_total?: number;
  batch_done?: number;
  file_name?: string;
  file_percent?: number;
  file_speed?: number;
  file_downloaded?: number;
  file_total?: number;
  total_bytes?: number;
  total_downloaded?: number;
  aggregated_percent?: number;
  /** 后端算好的"活跃文件速度之和"，前端不再自己累加 */
  total_speed?: number;
  /** 当前并发下载的文件数 */
  batch_running?: number;
  step?: string;
  /** @deprecated 旧 schema */
  filename?: string;
  /** @deprecated 旧 schema */
  percent?: number;
}

onActivated(async () => {
  // ★ 激活时注册监听器
  if (unlisten) { unlisten(); unlisten = null; } // 安全清理
  unlisten = await listen<ProgressPayload>("download-progress", (event) => {
    const p = event.payload;
    const fileName = p.file_name ?? p.filename ?? "";
    const batchTotal = p.batch_total ?? 0;
    const batchDone = p.batch_done ?? 0;
    const filePercent = p.file_percent ?? p.percent ?? 0;
    const aggregated = p.aggregated_percent ?? p.percent ?? filePercent;
    // 阶段事件只有一句文案和总百分比，没有文件粒度
    const isStage = p.kind === "stage" || (p.kind === undefined && p.batch_total === undefined);

    // ── 找到事件对应的任务卡：batch_id 优先，其次按名称，最后兜底最后一张卡 ──
    let task: DownloadTask | undefined;
    if (p.batch_id) {
      const tid = batchTaskMap.get(p.batch_id);
      if (tid !== undefined) task = dlTasks.value.find(t => t.id === tid);
    }
    if (!task && fileName) {
      task = dlTasks.value.find(t => t.name === fileName)
        ?? dlTasks.value.find(t => t.name.includes(fileName) || fileName.includes(t.name));
    }
    if (!task && dlTasks.value.length > 0) task = dlTasks.value[dlTasks.value.length - 1];
    if (!task) return;
    if (p.batch_id) batchTaskMap.set(p.batch_id, task.id);

    // ── 更新聚合进度 ──
    const batchId = p.batch_id ?? 0;
    let agg = progressMap.value[task.id];
    if (!agg || agg.batch_id !== batchId) {
      // 新批次开始
      agg = {
        batch_id: batchId,
        percent: 0,
        total_speed: 0,
        total_bytes: p.total_bytes ?? 0,
        downloaded_bytes: p.total_downloaded ?? 0,
        files: {},
        batch_done: batchDone,
        batch_total: batchTotal,
        running: 0,
        step: "",
      };
    }

    agg.batch_id = batchId;
    agg.batch_done = batchDone;
    agg.batch_total = batchTotal;
    if (p.total_bytes) agg.total_bytes = p.total_bytes;
    if (p.total_downloaded) agg.downloaded_bytes = p.total_downloaded;
    // 阶段进度和文件进度交替上报，百分比只增不减，避免进度条倒退
    agg.percent = Math.max(agg.percent, Math.min(aggregated, 100));
    // 阶段文案只来自 stage 事件；文件事件的状态由下面的百分比描述表达
    agg.step = isStage ? (p.step || "") : "";

    // 阶段事件不是文件，不进 per-file 列表
    if (!isStage && fileName) {
      agg.files[fileName] = {
        file_name: fileName,
        file_percent: filePercent,
        file_speed: p.file_speed ?? 0,
        file_downloaded: p.file_downloaded ?? 0,
        file_total: p.file_total ?? 0,
      };
    }

    // ── 计算总速度 ──
    // 后端已经按"活跃文件"记账给出 total_speed；只有老事件（没有该字段）才回退到前端累加
    if (typeof p.total_speed === "number") {
      agg.total_speed = p.total_speed;
    } else {
      let totalSpeed = 0;
      for (const key of Object.keys(agg.files)) {
        const f = agg.files[key];
        if (f.file_speed > 0 && f.file_percent < 100) totalSpeed += f.file_speed;
      }
      agg.total_speed = totalSpeed;
    }
    if (typeof p.batch_running === "number") agg.running = p.batch_running;

    progressMap.value = { ...progressMap.value, [task.id]: agg };

    // ── 更新任务状态显示 ──
    if (isStage) {
      // 阶段文案（准备 / 解压 / 安装加载器 …）
      task.status = p.step || `处理中 ${agg.percent}%`;
    } else if (batchTotal > 1) {
      task.status = `下载中 ${agg.percent}% (${batchDone}/${batchTotal})`;
    } else if (filePercent < 100) {
      task.status = `下载中 ${filePercent}% — ${fileName || task.name}`;
    } else {
      task.status = p.step || `处理中 — ${fileName || task.name}`;
    }

    // ── 更新步骤列表（详细面板用） ──
    if (!isStage && fileName) {
      if (!task.steps) task.steps = [];
      const stepText = batchTotal > 1
        ? `${fileName} ${filePercent}%`
        : `下载中 ${filePercent}%`;
      // 找到同名文件的旧步骤并替换
      const key = fileName.substring(0, Math.min(20, fileName.length));
      const existingIdx = task.steps.findIndex(s => s.startsWith(key));
      if (existingIdx >= 0) {
        task.steps[existingIdx] = stepText;
      } else {
        task.steps.push(stepText);
        if (task.steps.length > 50) task.steps.shift();
      }
    }
  });
});

onDeactivated(() => {
  // ★ 离开页面时卸载监听器
  if (unlisten) { unlisten(); unlisten = null; }
});

async function cancelTask(task: DownloadTask) {
  await invoke("cancel_download");
  task.status = "已取消";
  finishDlTask(task.id);
}

function removeHistory(task: DownloadTask) {
  dlHistory.value = dlHistory.value.filter(t => t.id !== task.id);
}

function clearHistory() {
  dlHistory.value = [];
}

// ── 格式化工具 ──
function fmtSpeed(bps: number): string {
  if (bps <= 0) return "";
  if (bps > 1048576) return `${(bps / 1048576).toFixed(1)} MB/s`;
  if (bps > 1024) return `${(bps / 1024).toFixed(0)} KB/s`;
  return `${bps.toFixed(0)} B/s`;
}

function fmtSize(bytes: number): string {
  if (bytes <= 0) return "? MB";
  if (bytes > 1073741824) return `${(bytes / 1073741824).toFixed(1)} GB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

// 将 files Record 转为可迭代的 entries 数组
function fileEntries(files: Record<string, FileProgress> | undefined): [string, FileProgress][] {
  if (!files) return [];
  return Object.entries(files);
}
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <h2 class="page-title"><span class="bl-zh">任务列表</span><span class="bl-en">TASKS</span></h2>
      <span class="page-line"></span>
      <button v-if="dlHistory.length > 0" class="acct-new-btn" @click="clearHistory" style="color:var(--danger);border-color:var(--danger)"><span>清空历史</span></button>
    </div>

    <!-- ══════════ 进行中任务 ══════════ -->
    <div v-if="dlTasks.length > 0" class="dl-task-section">
      <h3 class="dl-section-title">进行中 / ACTIVE</h3>
      <div class="dl-task-grid">
        <div class="dl-task-card active" v-for="t in dlTasks" :key="t.id">
          <div class="dtc-body">
            <!-- 顶栏: 名称 + 总速度 -->
            <div class="dtc-header">
              <div class="dtc-name">{{ t.name }}</div>
              <div class="dtc-total-speed" v-if="progressMap[t.id]?.total_speed">
                {{ fmtSpeed(progressMap[t.id].total_speed) }}
              </div>
            </div>

            <!-- 进度条 -->
            <div class="dtc-progress" v-if="progressMap[t.id]">
              <div class="dtc-bar"><div class="dtc-fill" :style="{width: Math.max(progressMap[t.id].percent, 1)+'%'}"></div></div>
              <div class="dtc-progress-info">
                <span class="dtc-pct">{{ progressMap[t.id].percent }}%</span>
                <span class="dtc-size" v-if="progressMap[t.id].total_bytes > 0">
                  {{ fmtSize(progressMap[t.id].downloaded_bytes) }} / {{ fmtSize(progressMap[t.id].total_bytes) }}
                </span>
                <span class="dtc-eta" v-if="etaText(progressMap[t.id])">{{ etaText(progressMap[t.id]) }}</span>
                <span class="dtc-count" v-if="progressMap[t.id].batch_total > 1">
                  {{ progressMap[t.id].batch_done }}/{{ progressMap[t.id].batch_total }} 文件
                </span>
                <span class="dtc-running" v-if="progressMap[t.id].running > 0">
                  {{ progressMap[t.id].running }} 路并发
                </span>
              </div>
            </div>

            <!-- 阶段文案（下载/解压/安装加载器 等没有文件粒度时的说明） -->
            <div class="dtc-step-text" v-if="progressMap[t.id]?.step">{{ progressMap[t.id].step }}</div>

            <!-- 还没有任何进度事件时的兜底状态文本 -->
            <div class="dtc-status" v-if="!progressMap[t.id]">{{ t.status }}</div>

            <!-- 详情开关 -->
            <button
              v-if="progressMap[t.id] && Object.keys(progressMap[t.id].files).length > 0"
              class="dtc-detail-toggle"
              @click="toggleDetails(t.id)"
              :title="expandedTasks.has(t.id) ? '收起详情' : '展开详情'"
            >
              {{ expandedTasks.has(t.id) ? '▲ 收起详情' : '▼ 详细信息' }}
              <span class="dtc-toggle-badge">{{ Object.keys(progressMap[t.id].files).length }}</span>
            </button>

            <!-- ══════════ 详细信息面板 ══════════ -->
            <Transition name="drop">
              <div v-if="expandedTasks.has(t.id) && progressMap[t.id]" class="dtc-detail-panel">
                <!-- 每个文件独立一行 + 迷你进度条 -->
                <div
                  class="dtc-file-row"
                  v-for="[fname, fp] in fileEntries(progressMap[t.id]?.files)"
                  :key="fname"
                  :class="{ done: fp.file_percent >= 100 }"
                >
                  <div class="dtc-file-name" :title="fname">{{ fname }}</div>
                  <div class="dtc-file-progress">
                    <div class="dtc-mini-bar">
                      <div class="dtc-mini-fill" :style="{width: fp.file_percent+'%'}"></div>
                    </div>
                    <span class="dtc-file-pct">{{ fp.file_percent }}%</span>
                    <span class="dtc-file-speed" v-if="fp.file_speed > 0 && fp.file_percent < 100">
                      {{ fmtSpeed(fp.file_speed) }}
                    </span>
                    <span class="dtc-file-size" v-if="fp.file_total > 0">
                      {{ fmtSize(fp.file_downloaded) }} / {{ fmtSize(fp.file_total) }}
                    </span>
                  </div>
                </div>
              </div>
            </Transition>
          </div>
          <button class="dtc-cancel" @click="cancelTask(t)" title="取消下载">✕</button>
        </div>
      </div>
    </div>

    <!-- ══════════ 历史记录 ══════════ -->
    <div v-if="dlHistory.length > 0" class="dl-task-section">
      <h3 class="dl-section-title">历史记录 / HISTORY</h3>
      <div class="dl-task-grid">
        <div class="dl-task-card" :class="{ fail: t.status === '失败' || t.status === '已取消' }" v-for="t in dlHistory" :key="t.id">
          <div class="dtc-body">
            <div class="dtc-name">{{ t.name }}</div>
            <div v-if="t.error" class="dtc-error">{{ t.error }}</div>
            <div class="dtc-status" :class="{ fail: t.status === '失败' || t.status === '已取消' }">{{ t.status }}</div>
          </div>
          <button class="dtc-remove" @click="removeHistory(t)" title="移除记录">✕</button>
        </div>
      </div>
    </div>

    <div v-if="dlTasks.length === 0 && dlHistory.length === 0" class="empty-state">
      <span class="empty-icon">◫</span>
      <span class="empty-title">还没有下载任务</span>
      <span class="empty-en">NO TASKS</span>
      <span class="empty-hint">安装游戏、整合包或模组时，进度和速度会显示在这里。</span>
      <button class="empty-cta" @click="emit('navigate', 'downloads')">去获取内容</button>
    </div>
  </section>
</template>
