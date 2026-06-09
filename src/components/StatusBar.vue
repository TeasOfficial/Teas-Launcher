<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const clock = ref("00:00:00");
let timer: number;

onMounted(() => {
  const tick = () => {
    const now = new Date();
    clock.value =
      now.getHours().toString().padStart(2, "0") + ":" +
      now.getMinutes().toString().padStart(2, "0") + ":" +
      now.getSeconds().toString().padStart(2, "0");
  };
  tick();
  timer = window.setInterval(tick, 1000);
});
onUnmounted(() => clearInterval(timer));

const appWindow = getCurrentWindow();
</script>

<template>
  <header class="status-bar" data-tauri-drag-region>
    <div class="status-bar-left" data-tauri-drag-region>
      <span class="sb-clock">{{ clock }}</span>
    </div>
    <div class="status-bar-spacer" data-tauri-drag-region></div>
    <div class="status-bar-right">
      <div class="window-controls">
        <button class="win-btn" @click="appWindow.minimize()" title="最小化">
          <svg viewBox="0 0 12 2" width="10" height="10"><rect width="12" height="2" fill="currentColor"/></svg>
        </button>
        <button class="win-btn win-close-btn" @click="appWindow.close()" title="关闭">
          <svg viewBox="0 0 12 12" width="10" height="10"><path d="M1 1l10 10M11 1l-10 10" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>
        </button>
      </div>
    </div>
  </header>
</template>
