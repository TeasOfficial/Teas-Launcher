<script setup lang="ts">
import { ref, computed, inject, watch, onMounted, onBeforeUnmount, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readUserConfig } from "../utils/config";
import type { Instance } from "../App.vue";
import { versionCmp } from "../utils/version";

const instances = inject<Instance[]>("instances")! as unknown as Ref<Instance[]>;
const emit = defineEmits<{
  /** 只设为当前实例（不跳页） */
  "select-instance": [inst: Instance];
  /** 设为当前实例并跳去仪表盘 */
  "start-instance": [inst: Instance];
  "navigate": [page: string];
}>();

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

// ── 排序 / 分组 ──────────────────────────────
// 前两种是平铺排序，后两种按同名字段分栏（每组一个小标题）。
type SortMode = "az" | "version" | "loader" | "mods";

const sortOptions: { id: SortMode; label: string }[] = [
  { id: "az", label: "A-Z 名称" },
  { id: "version", label: "游戏版本" },
  { id: "loader", label: "加载器" },
  { id: "mods", label: "模组数量" },
];

const sortMode = ref<SortMode>("az");
const sortOpen = ref(false);
const sortLabel = computed(() => sortOptions.find(o => o.id === sortMode.value)!.label);

// 点击下拉框外部时收起。不能用 @blur：div 不可聚焦，blur 永不触发，
// 而且模板作用域里没有 setTimeout（SettingsPage 里的同款写法实际是坏的）。
function onDocMousedown(e: MouseEvent) {
  if (!(e.target as Element | null)?.closest?.(".inst-sort")) sortOpen.value = false;
}
watch(sortOpen, open => {
  if (open) document.addEventListener("mousedown", onDocMousedown, true);
  else document.removeEventListener("mousedown", onDocMousedown, true);
});
onBeforeUnmount(() => document.removeEventListener("mousedown", onDocMousedown, true));

function selectSort(id: SortMode) {
  sortMode.value = id;
  sortOpen.value = false;
}

/** 加载器分组的固定顺序：原版在前，其余按主流程度，未知的排最后 */
const LOADER_ORDER = ["Vanilla", "Forge", "NeoForge", "Fabric", "Quilt", "Cleanroom"];
function loaderRank(name: string) {
  const i = LOADER_ORDER.indexOf(name);
  return i < 0 ? LOADER_ORDER.length : i;
}

/** numeric 让 "XPlus 1.21.8" 里的数字按数值而非字典序比较 */
function byName(a: Instance, b: Instance) {
  return a.name.localeCompare(b.name, "zh-Hans-CN", { numeric: true });
}

interface InstGroup { key: string; label: string; items: Instance[] }

const groups = computed<InstGroup[]>(() => {
  const list = filteredInstances.value;
  const mode = sortMode.value;

  // 平铺：整表一个"组"，label 为空则不渲染分组标题
  if (mode === "az" || mode === "mods") {
    const items = [...list];
    if (mode === "az") items.sort(byName);
    else items.sort((a, b) => b.mods - a.mods || byName(a, b));
    return [{ key: "_all", label: "", items }];
  }

  const map = new Map<string, Instance[]>();
  for (const inst of list) {
    const key = (mode === "version" ? inst.version : inst.loaderName || "Vanilla") || "未知";
    const arr = map.get(key);
    if (arr) arr.push(inst);
    else map.set(key, [inst]);
  }

  const entries = [...map.entries()];
  if (mode === "version") {
    // 新版本在前；快照/beta 解析不出数字，统一沉到最后
    entries.sort((a, b) => versionCmp(b[0], a[0]) || a[0].localeCompare(b[0]));
  } else {
    entries.sort((a, b) => loaderRank(a[0]) - loaderRank(b[0]) || a[0].localeCompare(b[0]));
  }
  return entries.map(([key, items]) => ({ key, label: key, items: items.sort(byName) }));
});

const totalCount = computed(() => filteredInstances.value.length);

function selectInstance(inst: Instance) {
  // 只选中：以前这里会直接跳仪表盘，误触按钮旁边就会丢页面，见 UI-REVIEW P1-3
  emit("select-instance", inst);
}

// ── 行尾 ⋯ 菜单 ──────────────────────────────
/** 当前打开菜单的实例名（同一时刻只允许一个） */
const menuFor = ref<string | null>(null);

function toggleMenu(name: string) {
  menuFor.value = menuFor.value === name ? null : name;
}
function closeMenu() { menuFor.value = null; }

function startInstance(inst: Instance) {
  closeMenu();
  emit("start-instance", inst);
}

async function openFolder(inst: Instance) {
  closeMenu();
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    await invoke("open_instance_folder", { mcDir, instanceName: inst.name });
  } catch (e) {
    deleteError.value = `打开目录失败: ${String(e)}`;
    setTimeout(() => { deleteError.value = ""; }, 5000);
  }
}

// 点页面其它地方关闭菜单（下拉不收起是长期问题，这里先按行内菜单处理）
function onDocClick() { if (menuFor.value) closeMenu(); }
onMounted(() => document.addEventListener("click", onDocClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocClick));

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

// ── 实例设置 ─────────────────────────────────
// 每个字段都可以"跟随全局"（用户级 tc.ini）。后端只保存被显式覆盖的项：
// 留空提交即写入 null，后端会删掉该键；全部清空则连配置文件一起删掉。

/** 参与覆盖的全部键（含下面单独渲染的分辨率两项） */
const ALL_KEYS = ["java_path", "max_memory", "jvm_args", "window_width", "window_height"];

const textFields = [
  { key: "java_path", zh: "Java 路径", en: "JAVA", browse: true,
    gv: (g: Record<string, any>) => g.java_path || "javaw" },
  { key: "max_memory", zh: "最大内存", en: "MAX MEMORY", browse: false,
    gv: (g: Record<string, any>) => g.max_memory || "4096 MB" },
  { key: "jvm_args", zh: "JVM 参数", en: "JVM ARGS", browse: false,
    gv: (g: Record<string, any>) => g.jvm_args || "-XX:+UseG1GC" },
];

const settingsInst = ref<Instance | null>(null);
/** 列表 / 设置页 两种视图（设置不再是弹窗） */
const view = ref<"list" | "settings">("list");
const globalCfg = ref<Record<string, any>>({});
const instCfg = ref<Record<string, any>>({});
const edit = ref<Record<string, string>>({});
const savingSettings = ref(false);
const settingsError = ref("");

async function openSettings(inst: Instance) {
  settingsError.value = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const [g, ic] = await Promise.all([
      readUserConfig(),
      // 配置损坏时按"无覆盖"处理，不要连设置面板都打不开
      invoke<Record<string, any>>("instance_config_read", { mcDir, instanceName: inst.name })
        .catch(() => ({})),
    ]);
    globalCfg.value = g || {};
    instCfg.value = ic || {};
  } catch (e) {
    settingsError.value = String(e);
    globalCfg.value = {};
    instCfg.value = {};
  }
  edit.value = {
    java_path: instCfg.value.java_path ?? "",
    max_memory: instCfg.value.max_memory ?? "",
    jvm_args: instCfg.value.jvm_args ?? "",
    window_width: instCfg.value.window_width ?? "",
    window_height: instCfg.value.window_height ?? "",
  };
  settingsInst.value = inst;
  closeMenu();
  view.value = "settings";
}

function closeSettings() {
  closeMenu();
  settingsInst.value = null;
  settingsError.value = "";
  view.value = "list";
}

async function browseJava() {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      filters: [{ name: "Java", extensions: ["exe"] }],
      defaultPath: edit.value.java_path || globalCfg.value.java_path || undefined,
    });
    if (selected) edit.value.java_path = selected as string;
  } catch { /* */ }
}

async function saveSettings() {
  const inst = settingsInst.value;
  if (!inst) return;
  savingSettings.value = true;
  settingsError.value = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    for (const k of ALL_KEYS) {
      const next = (edit.value[k] || "").trim();
      const cur = instCfg.value[k] == null ? "" : String(instCfg.value[k]);
      if (next === cur) continue; // 没动过的字段不写
      await invoke("instance_config_write", {
        mcDir,
        instanceName: inst.name,
        key: k,
        value: next === "" ? null : next,
      });
    }
    logSaved(inst.name);
    closeSettings();
  } catch (e) {
    settingsError.value = String(e);
  } finally {
    savingSettings.value = false;
  }
}

const savedHint = ref("");
function logSaved(name: string) {
  savedHint.value = `${name} 的设置已保存`;
  setTimeout(() => { if (savedHint.value === `${name} 的设置已保存`) savedHint.value = ""; }, 3000);
}
</script>

<template>
  <section class="page active">
    <!-- 列表页头（设置页有自己的页头） -->
    <div class="page-header" v-if="view === 'list'">
      <h2 class="page-title">
        <span class="bl-zh">实例列表</span>
        <span class="bl-en">INSTANCES</span>
      </h2>
      <span class="page-line"></span>
      <span class="inst-count">{{ totalCount }} 个实例</span>
      <div class="custom-select inst-sort" @click="sortOpen = !sortOpen">
        <span class="set-select-val">{{ sortLabel }}</span>
        <span class="set-select-arrow" :class="{ open: sortOpen }">▾</span>
        <Transition name="drop">
          <div class="set-select-options" v-if="sortOpen">
            <div
              class="set-select-opt"
              :class="{ active: o.id === sortMode }"
              v-for="o in sortOptions"
              :key="o.id"
              @click.stop="selectSort(o.id)"
            >{{ o.label }}</div>
          </div>
        </Transition>
      </div>
      <input class="set-input inst-search" v-model="searchText" placeholder="搜索实例..." />
      <button class="acct-new-btn" @click="emit('navigate', 'downloads')">
        <span class="bl-zh">＋ 新建实例</span>
        <span class="bl-en">NEW</span>
      </button>
      <button class="acct-new-btn" @click="refreshList">
        <span class="bl-zh">↻ 刷新</span>
        <span class="bl-en">REFRESH</span>
      </button>
    </div>

    <!-- 设置页页头 -->
    <div class="page-header" v-else>
      <button class="dl-back-btn" @click="closeSettings">
        ← <span class="bl-zh">返回</span><span class="bl-en">BACK</span>
      </button>
      <h2 class="page-title">
        <span class="bl-zh">实例设置</span>
        <span class="bl-en">INSTANCE SETTINGS</span>
      </h2>
      <span class="page-line"></span>
    </div>

    <div class="inst-list" v-if="view === 'list'">
      <div class="inst-row inst-row-head">
        <span class="inst-dot"></span>
        <span>实例 / INSTANCE</span>
        <span>MC 版本</span>
        <span>加载器</span>
        <span class="ir-num">模组</span>
        <span>最后游玩</span>
        <span></span>
      </div>

      <template v-for="g in groups" :key="g.key">
        <div v-if="g.label" class="inst-group">
          <span class="ig-label">{{ g.label }}</span>
          <span class="ig-count">{{ g.items.length }}</span>
          <span class="ig-line"></span>
        </div>
        <div
          v-for="inst in g.items"
          :key="inst.name"
          class="inst-row"
          :class="{ 'inst-row-active': inst.active }"
          @click="selectInstance(inst)"
          @dblclick="startInstance(inst)"
          title="单击选中 · 双击进入仪表盘"
        >
          <span class="inst-dot" :class="{ active: inst.active }"></span>
          <span class="ir-name" :title="inst.name">{{ inst.name }}</span>
          <span class="ir-ver">{{ inst.version }}</span>
          <span class="ir-loader">{{ inst.loaderName || 'Vanilla' }}</span>
          <span class="ir-num">{{ inst.mods }}</span>
          <span class="ir-played">{{ inst.lastPlayed }}</span>
          <span class="ir-menu-wrap">
            <button
              class="inst-more-btn"
              :class="{ open: menuFor === inst.name }"
              title="更多操作"
              @click.stop="toggleMenu(inst.name)"
            >⋯</button>
            <Transition name="drop">
              <div v-if="menuFor === inst.name" class="inst-menu" @click.stop>
                <button class="inst-menu-item" @click="startInstance(inst)">
                  <span class="bl-zh">启动</span><span class="bl-en">PLAY</span>
                </button>
                <button class="inst-menu-item" @click="openSettings(inst)">
                  <span class="bl-zh">设置</span><span class="bl-en">SETTINGS</span>
                </button>
                <button class="inst-menu-item" @click="openFolder(inst)">
                  <span class="bl-zh">打开目录</span><span class="bl-en">OPEN FOLDER</span>
                </button>
                <button
                  class="inst-menu-item danger"
                  :class="{ confirm: deleting === inst.name }"
                  @click="deleteInstance(inst)"
                >
                  <span class="bl-zh">{{ deleting === inst.name ? '确认删除' : '删除' }}</span>
                </button>
              </div>
            </Transition>
          </span>
        </div>
      </template>

      <div v-if="totalCount === 0" class="inst-empty">
        <span class="ie-zh">{{ searchText.trim() ? '没有匹配的实例' : '还没有任何实例' }}</span>
        <span class="ie-en">{{ searchText.trim() ? 'NO MATCH' : 'NO INSTANCES' }}</span>
        <button v-if="!searchText.trim()" class="acct-new-btn" @click="emit('navigate', 'downloads')">
          前往获取内容
        </button>
      </div>
    </div>

    <!-- 保存提示 -->
    <Transition name="dl-view">
      <div v-if="savedHint" class="dl-toast">{{ savedHint }}</div>
    </Transition>

    <!-- 实例设置（整页） -->
    <div v-if="view === 'settings'" class="iset-page">
      <div class="iset-page-grid">
        <div class="iset-page-main">
          <div class="iset-inst">{{ settingsInst?.name }}</div>
          <div class="iset-note">
            留空 = 跟随全局设置（灰色占位符是全局当前值）。只有你填写的项会写进
            <code>{{ settingsInst?.name }}/teas-instance.json</code>，其余仍用全局值。
          </div>

          <div class="iset-row" v-for="f in textFields" :key="f.key">
            <div class="iset-head">
              <span class="iset-label">
                <span class="bl-zh">{{ f.zh }}</span>
                <span class="bl-en">{{ f.en }}</span>
              </span>
              <button v-if="edit[f.key]" class="iset-clear" @click="edit[f.key] = ''">
                跟随全局
              </button>
            </div>
            <div class="iset-line">
              <input
                class="form-input iset-input"
                v-model="edit[f.key]"
                :placeholder="`跟随全局：${f.gv(globalCfg)}`"
              />
              <button v-if="f.browse" class="iset-browse" @click="browseJava">浏览</button>
            </div>
          </div>

          <div class="iset-row">
            <div class="iset-head">
              <span class="iset-label">
                <span class="bl-zh">窗口分辨率</span>
                <span class="bl-en">WINDOW SIZE</span>
              </span>
              <button
                v-if="edit.window_width || edit.window_height"
                class="iset-clear"
                @click="edit.window_width = ''; edit.window_height = ''"
              >跟随全局</button>
            </div>
            <div class="iset-line">
              <input
                class="form-input iset-wh"
                v-model="edit.window_width"
                :placeholder="globalCfg.window_width || '854'"
              />
              <span class="iset-x">×</span>
              <input
                class="form-input iset-wh"
                v-model="edit.window_height"
                :placeholder="globalCfg.window_height || '480'"
              />
            </div>
          </div>

          <div v-if="settingsError" class="iset-error">{{ settingsError }}</div>
        </div>

        <aside class="iset-page-side">
          <div class="dl-side-title">全局当前值<span>GLOBAL</span></div>
          <div class="dl-side-row"><span>Java</span><b class="dl-side-path">{{ globalCfg.java_path || 'javaw' }}</b></div>
          <div class="dl-side-row"><span>最大内存</span><b>{{ globalCfg.max_memory || '4096 MB' }}</b></div>
          <div class="dl-side-row"><span>JVM 参数</span><b class="dl-side-path">{{ globalCfg.jvm_args || '默认' }}</b></div>
          <div class="dl-side-row"><span>分辨率</span><b>{{ globalCfg.window_width || '854' }} × {{ globalCfg.window_height || '480' }}</b></div>

          <div class="iset-page-actions">
            <button class="acct-new-btn iset-cancel" @click="closeSettings">取消</button>
            <button class="acct-new-btn" :disabled="savingSettings" @click="saveSettings">
              {{ savingSettings ? '保存中...' : '保存' }}
            </button>
          </div>
        </aside>
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
