<script setup lang="ts">
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
}
const progressMap = ref<Record<number, AggregatedProgress>>({});

const expandedTasks = ref<Set<number>>(new Set());
function toggleDetails(taskId: number) {
  const next = new Set(expandedTasks.value);
  if (next.has(taskId)) next.delete(taskId); else next.add(taskId);
  expandedTasks.value = next;
}

let unlisten: UnlistenFn | null = null;

onActivated(async () => {
  // ★ 激活时注册监听器
  if (unlisten) { unlisten(); unlisten = null; } // 安全清理
  unlisten = await listen<{
    batch_id: number;
    batch_total: number;
    batch_done: number;
    file_name: string;
    file_percent: number;
    file_speed: number;
    file_downloaded: number;
    file_total: number;
    total_bytes: number;
    total_downloaded: number;
    aggregated_percent: number;
    step: string;
  }>("download-progress", (event) => {
    const p = event.payload;

    // ── 建立 batch_id → task 映射 ──
    let task: DownloadTask | undefined;
    if (p.batch_id > 0 && batchTaskMap.has(p.batch_id)) {
      const tid = batchTaskMap.get(p.batch_id)!;
      task = dlTasks.value.find(t => t.id === tid);
    }
    if (!task) {
      // 按文件名匹配任务
      task = dlTasks.value.find(t => t.name === p.file_name);
      if (!task) task = dlTasks.value.find(t => p.file_name.includes(t.name));
      if (!task) task = dlTasks.value.find(t => t.name.includes(p.file_name));
      if (!task && dlTasks.value.length > 0) task = dlTasks.value[dlTasks.value.length - 1];
      if (!task) return;

      // 记录 mapping
      if (p.batch_id > 0) {
        batchTaskMap.set(p.batch_id, task.id);
      }
    }

    // ── 更新聚合进度 ──
    let agg = progressMap.value[task.id];
    if (!agg || agg.batch_id !== p.batch_id) {
      // 新批次开始
      agg = {
        batch_id: p.batch_id,
        percent: 0,
        total_speed: 0,
        total_bytes: p.total_bytes,
        downloaded_bytes: 0,
        files: {},
        batch_done: 0,
        batch_total: p.batch_total,
      };
    }

    // 更新 per-file 进度
    agg.batch_id = p.batch_id;
    agg.batch_done = p.batch_done;
    agg.batch_total = p.batch_total;
    agg.total_bytes = p.total_bytes;

    // 用 aggregated_percent 作为进度条的进度（不会横跳）
    agg.percent = p.aggregated_percent;

    // 更新或插入 per-file 条目
    agg.files[p.file_name] = {
      file_name: p.file_name,
      file_percent: p.file_percent,
      file_speed: p.file_speed,
      file_downloaded: p.file_downloaded,
      file_total: p.file_total,
    };

    // ── 计算总速度：所有活跃文件速度之和 ──
    let totalSpeed = 0;
    for (const key of Object.keys(agg.files)) {
      const f = agg.files[key];
      if (f.file_speed > 0 && f.file_percent < 100) {
        totalSpeed += f.file_speed;
      }
    }
    agg.total_speed = totalSpeed;

    progressMap.value = { ...progressMap.value, [task.id]: agg };

    // ── 更新任务状态显示 ──
    if (!p.batch_total && !p.file_percent && p.step) {
      // 自定义阶段描述（如整合包安装的"解压中..."，没有批量/文件进度时使用）
      task.status = p.step;
    } else if (p.batch_total > 1) {
      task.status = `下载中 ${p.aggregated_percent || 0}% (${p.batch_done}/${p.batch_total})`;
    } else if (p.file_percent < 100) {
      task.status = `下载中 ${p.file_percent}% — ${p.file_name}`;
    } else {
      task.status = `处理中 — ${p.file_name}`;
    }

    // ── 更新步骤列表（详细面板用） ──
    if (!task.steps) task.steps = [];
    const stepText = p.batch_total > 1
      ? `${p.file_name} ${p.file_percent}%`
      : `下载中 ${p.file_percent}%`;
    // 找到同名文件的旧步骤并替换
    const existingIdx = task.steps.findIndex(s =>
      s.startsWith(p.file_name.substring(0, Math.min(20, p.file_name.length)))
    );
    if (existingIdx >= 0) {
      task.steps[existingIdx] = stepText;
    } else {
      task.steps.push(stepText);
      if (task.steps.length > 50) task.steps.shift();
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
                <span class="dtc-count" v-if="progressMap[t.id].batch_total > 1">
                  {{ progressMap[t.id].batch_done }}/{{ progressMap[t.id].batch_total }} 文件
                </span>
              </div>
            </div>

            <!-- 状态文本 -->
            <div class="dtc-status" v-if="!progressMap[t.id]">{{ t.status }}</div>

            <!-- 详情开关 -->
            <button
              v-if="progressMap[t.id] && Object.keys(progressMap[t.id].files).length > 0"
              class="dtc-detail-toggle"
              @click="toggleDetails(t.id)"
              :title="expandedTasks.has(t.id) ? '收起详情' : '展开详情'"
            >
              {{ expandedTasks.has(t.id) ? '▲ 收起详情' : '▼ 详细信息' }}
              <span class="dtc-toggle-badge">{{ progressMap[t.id].files.size }}</span>
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
                      {{ fmtSize(fp.file_total) }}
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

    <div v-if="dlTasks.length === 0 && dlHistory.length === 0" class="dl-results-empty">
      <span class="bl-zh">暂无任务</span>
      <span class="bl-en">No tasks</span>
    </div>
  </section>
</template>
