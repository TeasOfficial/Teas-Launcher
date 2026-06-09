<script setup lang="ts">
import { ref, computed, inject, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Instance } from "../App.vue";

const instances = inject<Instance[]>("instances")! as unknown as Ref<Instance[]>;
const emit = defineEmits<{ "select-instance": [inst: Instance]; "navigate": [page: string] }>();

const deleting = ref("");
const searchText = ref("");

const filteredInstances = computed(() => {
  const q = searchText.value.toLowerCase().trim();
  if (!q) return instances.value;
  return instances.value.filter(i =>
    i.name.toLowerCase().includes(q) ||
    i.version.toLowerCase().includes(q) ||
    (i.loaderName || "").toLowerCase().includes(q)
  );
});

function selectInstance(inst: Instance) {
  emit("select-instance", inst);
}

const deleteError = ref("");

async function refreshList() {
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const list = await invoke<Instance[]>("list_instances", { mcDir });
    instances.value = list;
  } catch { /* */ }
}

async function deleteInstance(inst: Instance) {
  if (deleting.value === inst.name) {
    try {
      const mcDir = await invoke<string>("get_minecraft_dir");
      await invoke("delete_instance", { mcDir, instanceName: inst.name });
      await refreshList();
    } catch (e) {
      deleteError.value = String(e);
      setTimeout(() => { deleteError.value = ""; }, 5000);
    }
    deleting.value = "";
  } else {
    deleting.value = inst.name;
    setTimeout(() => { deleting.value = ""; }, 3000);
  }
}
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <h2 class="page-title">
        <span class="bl-zh">实例列表</span>
        <span class="bl-en">INSTANCES</span>
      </h2>
      <span class="page-line"></span>
      <input class="set-input" v-model="searchText" placeholder="搜索实例..." style="width:180px;margin-right:8px;flex-shrink:0" />
      <button class="acct-new-btn" style="margin-right:0" @click="refreshList">
        <span class="bl-zh">↻ 刷新</span>
        <span class="bl-en">REFRESH</span>
      </button>
    </div>

    <div class="instance-cards">
      <div
        class="inst-card"
        :class="{ 'inst-active': inst.active }"
        v-for="inst in filteredInstances"
        :key="inst.name"
      >
        <div class="inst-card-head" @click="selectInstance(inst)">
          <span class="inst-dot" :class="{ active: inst.active }"></span>
          <span class="inst-name">{{ inst.name }}</span>
          <span class="inst-ver">{{ inst.loaderName ?? inst.loader }}</span>
        </div>
        <div class="inst-card-body" @click="selectInstance(inst)">
          <div class="inst-stat">
            <span><span class="bl-zh">MC 版本</span><span class="bl-en">Version</span></span>
            <span>{{ inst.version }}</span>
          </div>
          <div class="inst-stat">
            <span><span class="bl-zh">加载器</span><span class="bl-en">Loader</span></span>
            <span>{{ inst.loaderName ?? (inst.loader ? 'Forge' : 'Vanilla') }}</span>
          </div>
          <div class="inst-stat">
            <span><span class="bl-zh">最后游玩</span><span class="bl-en">Last Played</span></span>
            <span>{{ inst.lastPlayed }}</span>
          </div>
        </div>
        <div class="inst-card-actions">
          <button class="inst-settings-btn" @click.stop>
            <span>设置</span>
          </button>
          <button
            class="inst-delete-btn"
            :class="{ confirm: deleting === inst.name }"
            @click.stop="deleteInstance(inst)"
          >
            <span>{{ deleting === inst.name ? '确认删除' : '删除' }}</span>
          </button>
        </div>
      </div>

      <div class="inst-card new-instance" @click="emit('navigate', 'downloads')">
        <div class="inst-card-add">
          <span class="bl-zh">+ 新建实例</span>
          <span class="bl-en">+ NEW INSTANCE</span>
        </div>
      </div>
    </div>
    <!-- 删除错误提示 -->
    <Transition name="modal">
      <div v-if="deleteError" class="modal-overlay" @click.self="deleteError = ''">
        <div class="modal-panel" style="width:360px">
          <div class="modal-head">
            <span class="modal-title">删除失败</span>
            <button class="modal-close" @click="deleteError = ''">✕</button>
          </div>
          <div class="modal-body">
            <p class="modal-desc" style="color:var(--danger)">{{ deleteError }}</p>
          </div>
        </div>
      </div>
    </Transition>
  </section>
</template>
