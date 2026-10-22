<script setup lang="ts">
import { ref, inject, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Instance } from "../App.vue";
import { readUserConfig } from "../utils/config";
import { useGameInstall } from "../composables/useGameInstall";

const activeInstance = inject<any>("activeInstance");

// 实例列表由 App.vue provide。安装完新实例必须刷新这份共享状态，
// 否则要重启启动器（或切页触发重新扫描）才能看到刚装好的版本。
const instances = inject<Ref<Instance[]>>("instances", ref([]) as Ref<Instance[]>);

async function refreshInstances() {
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    instances.value = await invoke<Instance[]>("list_instances", { mcDir });
  } catch (e) {
    console.warn("[DL] 刷新实例列表失败:", e);
  }
}

// ★ PCL-style 版本判定: 检查 mod/game version 是否与当前实例兼容
function getMCVersion(): string {
  const v = activeInstance?.value?.version || "";
  // 从版本名提取 vanilla MC 版本 (如 "13.2.2 for 26.1.2" → "26.1.2", "26.1.2" → "26.1.2")
  const parts = v.split(" for ");
  if (parts.length >= 2) return parts[parts.length - 1];
  return v;
}
function isVersionCompatible(modVersions: string[]): boolean {
  if (!modVersions || modVersions.length === 0) return true; // 未知 = 可能兼容
  const mcVer = getMCVersion();
  if (!mcVer) return true;
  return modVersions.some(mv => {
    // 精确匹配或前缀匹配 (如 "26.1" 匹配 "26.1.2")
    return mv === mcVer || mcVer.startsWith(mv + ".") || mv.startsWith(mcVer + ".");
  });
}
function compatBadge(modVersions: string[]): { cls: string; text: string } {
  if (!modVersions || modVersions.length === 0) return { cls: "dl-compat-unknown", text: "版本未知" };
  return isVersionCompatible(modVersions)
    ? { cls: "dl-compat-ok", text: "✓ 版本匹配" }
    : { cls: "dl-compat-warn", text: "⚠ 版本可能不兼容" };
}

const emit = defineEmits<{
  (e: "navigate", page: string): void;
  /** 用户从「本地整合包」按钮选了文件，交给 App.vue 走拖放安装页 */
  (e: "local-pack", path: string): void;
}>();

type Section = "main" | "game" | "modpack" | "resource" | "search" | "detail" | "install" | "modpack-install";

const active = ref<Section>("main");
const searchType = ref("");
const modSource = ref("Modrinth"); // Modrinth 或 CurseForge

const categories = [
  {
    id: "game" as Section, zh: "原版游戏", en: "VANILLA", icon: "▣",
    desc: "安装 Mojang 官方发布的 Minecraft 版本，可搭配 Forge / Fabric / NeoForge / Quilt 等加载器。",
    meta: ["版本覆盖 1.0 → 最新快照", "5 种加载器：Forge / NeoForge / Fabric / Quilt / Cleanroom", "可指定实例名与下载源"],
  },
  {
    id: "modpack" as Section, zh: "整合包", en: "MODPACKS", icon: "◫", bbsmcType: "modpack",
    desc: "从 Modrinth / CurseForge 一键安装整合包，自带配置与资源，装完即可启动。",
    meta: ["支持 .mrpack 与 CurseForge 整合包", "自动安装对应加载器与依赖", "overrides 自动覆盖到实例"],
  },
  {
    id: "resource" as Section, zh: "模组资源", en: "MODS & MORE", icon: "⬡",
    desc: "搜索 Mod、资源包、光影与数据包，直接安装到指定实例。",
    meta: ["Mod / 资源包 / 光影 / 数据包", "自动匹配实例的游戏版本", "支持拖入本地文件安装"],
  },
];

// ── 下载任务管理（页面内所有安装流程共用）──
const installMsg = ref("");
const addDlTask = inject<(name: string) => number>("addDlTask", () => { console.error("[DL] addDlTask 注入失败!"); return 0; });
const updateDlTask = inject<(id: number, status: string) => void>("updateDlTask", () => { console.error("[DL] updateDlTask 注入失败!"); });
const finishDlTask = inject<(id: number, error?: string) => void>("finishDlTask", () => { console.error("[DL] finishDlTask 注入失败!"); });

// MCIMirror 已停止运营：Mod / 整合包一律走官方源，不再读 mod_source 配置。
// 保留这个函数只是为了让 8 个调用点不用改（后端仍需要 source 参数区分官方/镜像，传入官方即可）。
async function getModSource() { return "官方源"; }

// ── 游戏版本安装向导（状态机见 composables/useGameInstall.ts）──
const {
  gameVersions, gameExpanded, gameLeaving, gameVersionList, gameLoading,
  toggleGameCard,
  selectedGameVer, forgeMcVersions,
  selectedLoader, selectedLoaderVer, installGameInstanceName,
  showLoaderVersions, loaderVersionList, loaderVersionLoading, pendingLoader,
  openLoaderVersions, confirmLoaderVersion, backToLoaderSelect,
  loaderOptions, isLoaderCompatible, getRecommendedLoader, getLoaderIncompatReason,
  isLoaderDisabled, gameInstallName,
  doInstallGame, closeGameInstall, openGameInstall,
} = useGameInstall({ addDlTask, updateDlTask, finishDlTask, installMsg, refreshInstances });


const resourceTypes = [
  { id: "mods", zh: "模组", en: "Mods", desc: "Forge / Fabric / NeoForge", bbsmcType: "mod" },
  { id: "texture", zh: "材质包", en: "Textures", desc: "改变游戏视觉外观", bbsmcType: "resourcepack" },
  { id: "shader", zh: "光影", en: "Shaders", desc: "增强光照与渲染效果", bbsmcType: "shader" },
  { id: "map", zh: "地图", en: "Maps", desc: "冒险 / 解谜 / 生存", bbsmcType: "map" },
  { id: "datapack", zh: "数据包", en: "Datapacks", desc: "修改机制与合成配方", bbsmcType: "datapack" },
];

const sectionTitle: Record<string, { zh: string; en: string }> = {
  main: { zh: "获取内容", en: "BROWSE" },
  game: { zh: "原版游戏", en: "VANILLA GAME" },
  install: { zh: "安装游戏", en: "INSTALL GAME" },
  "modpack-install": { zh: "安装整合包", en: "INSTALL MODPACK" },
  resource: { zh: "资源浏览", en: "RESOURCES" },
  search: { zh: "搜索资源", en: "SEARCH" },
  detail: { zh: "资源详情", en: "DETAILS" },
};

// 版本下拉筛选（从搜索结果/详情动态收集）
const showVersionDropdown = ref(false);
const detailVersionFilter = ref("全部");
const showDetailVerDropdown = ref(false);

// 搜索筛选使用的版本列表（常用版本 + 搜索结果中的其他版本会自动追加）
const searchFilterVersions = ref(["全部", "1.21.5", "1.21.4", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.19.2", "1.18.2", "1.16.5", "1.12.2"]);

function setVersion(v: string) {
  searchVersion.value = v === "全部" ? "" : v;
  showVersionDropdown.value = false;
  doSearch(0);
}

// 从详情版本列表动态收集所有 game_versions 作为筛选选项
const detailFilterOptions = function() {
  const set = new Set<string>();
  set.add("全部");
  for (const v of detailVersions.value) {
    for (const gv of v.game_versions) {
      set.add(gv);
    }
  }
  return Array.from(set).sort().reverse();
};

function filteredDetailVersions() {
  if (detailVersionFilter.value === "全部") return detailVersions.value;
  return detailVersions.value.filter(v =>
    v.game_versions.some(gv => gv === detailVersionFilter.value)
  );
}

interface SearchHit {
  project_id: string; title: string; description: string; author: string;
  project_type: string; downloads: number; icon_url: string;
  versions: string[]; categories: string[]; display_categories: string[];
}

const searchSort = ref("relevance");
const searchQuery = ref("");
const searchVersion = ref("");
const searchResults = ref<SearchHit[]>([]);
const searchLoading = ref(false);
const searchTotal = ref(0);
const searchOffset = ref(0);
const currentBbsmcType = ref("");
const searchLimit = 20;
/** 是否已经发起过搜索 — 区分"还没搜"与"搜了但没结果" */
const searchRequested = ref(false);
/** 搜索失败原因，为空表示无错误 */
const searchError = ref("");

let searchDebounce: number | null = null;

function debouncedSearch() {
  if (searchDebounce !== null) clearTimeout(searchDebounce);
  searchDebounce = window.setTimeout(() => {
    doSearch(0);
  }, 400); // 400ms debounce
}

/** 点「搜索」立即执行：先取消待触发的 debounce，避免一次输入发两遍请求 */
function searchNow() {
  if (searchDebounce !== null) {
    clearTimeout(searchDebounce);
    searchDebounce = null;
  }
  doSearch(0);
}

const sortOptions = [
  { value: "relevance", zh: "相关性" },
  { value: "downloads", zh: "下载量" },
  { value: "follows", zh: "关注数" },
  { value: "newest", zh: "最新发布" },
  { value: "updated", zh: "最近更新" },
];

async function doSearch(page: number = 0) {
  if (page === 0) searchResults.value = []; // 新搜索先清空
  searchLoading.value = true;
  searchOffset.value = page * searchLimit;
  searchRequested.value = true;
  searchError.value = "";
  try {
    if (modSource.value === "CurseForge") {
      const result = await invoke<any>("search_curseforge", {
        source: await getModSource(),
        query: searchQuery.value,
        projectType: currentBbsmcType.value,
        version: searchVersion.value,
        sort: searchSort.value,
        limit: searchLimit,
        offset: searchOffset.value,
      });
      const hits = (result.data || []) as any[];
      searchResults.value = hits.map((h: any) => ({
        project_id: String(h.id), title: h.name, description: h.summary || "",
        author: h.authors?.[0]?.name || "", project_type: currentBbsmcType.value,
        downloads: h.downloadCount || 0, icon_url: h.logo?.thumbnailUrl || "",
        versions: [], categories: [], display_categories: [],
      }));
      searchTotal.value = result.pagination?.totalCount || 0;
    } else {
      const result = await invoke<any>("search_bbsmc", {
        source: await getModSource(),
        query: searchQuery.value, projectType: currentBbsmcType.value,
        version: searchVersion.value, sort: searchSort.value,
        limit: searchLimit, offset: searchOffset.value,
      });
      // 服务端已按 project_type facet 过滤，此处不能再按返回的 project_type 二次过滤：
      // Modrinth 的 facet 会跨类型命中（datapack facet 也会返回 project_type=mod 的世界生成包），
      // 二次过滤会把结果整批丢掉，表现为"搜索永远没结果"。
      searchResults.value = (result.hits || []) as SearchHit[];
      searchTotal.value = result.total_hits || 0;
    }
  } catch (e) {
    searchError.value = String(e);
    searchTotal.value = 0;
    console.error("[search] 搜索失败:", e);
  }
  searchLoading.value = false;
}

interface CfFile { id: number; fileName: string; fileLength: number; downloadUrl: string; }
interface VersionFile { filename: string; url: string; size: number; }
interface ProjectVersion { id: string; name: string; version_number: string; loaders: string[]; game_versions: string[]; files: VersionFile[]; }
interface ProjectDetail { title: string; description: string; icon_url: string; downloads: number; followers: number; author: string; versions: ProjectVersion[]; /** 兼容性横幅用；CF 分支没有这个字段，故可选 */ game_versions?: string[]; }

const projectDetail = ref<ProjectDetail | null>(null);
const detailVersions = ref<ProjectVersion[]>([]);
const cfModId = ref(0);

async function openDetail(project: SearchHit) {
  active.value = "detail";
  projectDetail.value = null;
  detailVersions.value = [];
  try {
    if (modSource.value === "CurseForge") {
      const modId = parseInt(project.project_id);
      cfModId.value = modId;
      const detail = await invoke<any>("get_curseforge_project", { modId, source: await getModSource() });
      projectDetail.value = {
        title: detail.data?.name || "", description: detail.data?.summary || "",
        icon_url: detail.data?.logo?.thumbnailUrl || "", downloads: detail.data?.downloadCount || 0,
        followers: 0, author: detail.data?.authors?.[0]?.name || "", versions: [],
      };
      const files = await invoke<any>("get_curseforge_files", { modId, source: await getModSource() });
      // CurseForge /files API 返回扁平列表 [{id, fileName, downloadUrl, fileLength}]
      const cfData = (files.data || []) as any[];
      detailVersions.value = cfData.map((v: any) => ({
        id: String(v.id), name: v.fileName, version_number: v.fileName,
        loaders: v.loaders || [], game_versions: v.gameVersions || [],
        files: [{
          filename: v.fileName, url: v.downloadUrl || "", size: v.fileLength || 0,
        }],
      }));
    } else {
      projectDetail.value = await invoke<ProjectDetail>("get_project", { projectId: project.project_id });
      const verData = await invoke<any>("get_project_versions", { projectId: project.project_id });
      detailVersions.value = (verData || []) as ProjectVersion[];
    }
  } catch { /* */ }
}

async function installCfFile(file: CfFile & { projectType: string }) {
  const taskId = addDlTask(file.fileName);
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const instName = activeInstance?.value?.name || "";
    await invoke<string>("install_curseforge_file", {
      mcDir, fileId: file.id, fileName: file.fileName,
      projectType: file.projectType, source: await getModSource(),
      instanceName: instName,
    });
    updateDlTask(taskId, "已完成");
    finishDlTask(taskId);
  } catch (e) {
    updateDlTask(taskId, "失败");
    finishDlTask(taskId, String(e));
  }
}

const modpackFileName = ref("");
const modpackFileUrl = ref("");
const modpackFileSize = ref(0);
const modpackInstanceName = ref("");
const modpackConflict = ref(false);
/** 安装整合包页的返回目标（详情页 / 搜索页） */
const modpackReturnTo = ref<Section>("detail");

function openModpackInstall(f: VersionFile, versionName: string) {
  modpackFileName.value = f.filename;
  modpackFileUrl.value = f.url;
  modpackFileSize.value = f.size || 0;
  // 实例名不该带扩展名：CF 的 versionName 往往就是文件名（xxx.zip）
  modpackInstanceName.value = (versionName || f.filename).replace(/\.(zip|mrpack)$/i, "");
  modpackConflict.value = false;
  // 从详情页点「安装」进来的，取消/完成后回到详情页
  modpackReturnTo.value = active.value === "detail" ? "detail" : "search";
  active.value = "modpack-install";
  void checkConflict();
}

/** 管理员运行时系统会拦截拖放（UIPI），这里提供不依赖拖放的选择入口 */
async function pickLocalPack() {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [{ name: "整合包", extensions: ["mrpack", "zip"] }],
    });
    if (typeof selected === "string" && selected) emit("local-pack", selected);
  } catch { /* 用户取消 */ }
}

async function checkConflict() {
  if (!modpackInstanceName.value.trim()) return;
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const list = await invoke<Instance[]>("list_instances", { mcDir });
    modpackConflict.value = list.some(i => i.name === modpackInstanceName.value.trim());
  } catch { /* */ }
}

async function confirmModpackInstall() {
  if (!modpackInstanceName.value.trim()) return;
  active.value = modpackReturnTo.value;
  const taskId = addDlTask(modpackInstanceName.value);
  console.log(`[DL] confirmModpackInstall 开始: taskId=${taskId} name=${modpackInstanceName.value}`);
  installMsg.value = `正在安装整合包 ${modpackInstanceName.value}...`;
  let success = false;
  let errorMsg = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const source = await getModSource();
    const dlThreads = parseInt((await readUserConfig()).dl_threads) || 8;
    console.log(`[DL] 调用 install_modpack: ${modpackFileName.value} source=${source} threads=${dlThreads}`);
    const msg = await invoke<string>("install_modpack", {
      mcDir, url: modpackFileUrl.value, filename: modpackFileName.value,
      instanceName: modpackInstanceName.value.trim(), source, dlThreads,
    });
    console.log(`[DL] install_modpack 返回成功: "${msg}"`);
    installMsg.value = msg;
    success = true;
    await refreshInstances();
  } catch (e) {
    console.error(`[DL] confirmModpackInstall 异常: taskId=${taskId}`, e);
    errorMsg = String(e);
    installMsg.value = `安装失败: ${errorMsg}`;
  } finally {
    updateDlTask(taskId, success ? "已完成" : "失败");
    finishDlTask(taskId, success ? undefined : errorMsg);
    console.log(`[DL] finally: finishDlTask(${taskId}) success=${success} error=${errorMsg || "无"}`);
    setTimeout(() => { installMsg.value = ""; }, 4000);
  }
}



async function installFile(url: string, filename: string) {
  const taskId = addDlTask(filename);
  console.log(`[DL] installFile 开始: taskId=${taskId} file=${filename} url=${url}`);
  installMsg.value = `正在下载 ${filename}...`;
  let success = false;
  let errorMsg = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const source = await getModSource();
    console.log(`[DL] 调用 install_file: ${filename} source=${source} type=${currentBbsmcType.value}`);
    const instName = activeInstance?.value?.name || "";
    const msg = await invoke<string>("install_file", { mcDir, url, filename, projectType: currentBbsmcType.value, source, instanceName: instName });
    console.log(`[DL] install_file 返回成功: "${msg}"`);
    installMsg.value = msg;
    success = true;
  } catch (e) {
    console.error(`[DL] installFile 异常: taskId=${taskId}`, e);
    errorMsg = String(e);
    installMsg.value = `下载失败: ${errorMsg}`;
  } finally {
    // finally 保证无论成功/失败/异常都完成任务
    updateDlTask(taskId, success ? "已完成" : "失败");
    finishDlTask(taskId, success ? undefined : errorMsg);
    console.log(`[DL] finally: finishDlTask(${taskId}) success=${success} error=${errorMsg || "无"}`);
    setTimeout(() => { installMsg.value = ""; }, 4000);
  }
}

function openSearch(type: { id: string; zh: string; en: string; bbsmcType: string }) {
  currentBbsmcType.value = type.bbsmcType;
  searchResults.value = [];
  searchType.value = type.zh;
  active.value = "search";
  searchQuery.value = "";
  searchVersion.value = "";
  searchSort.value = "relevance";
  doSearch();
}

function goBack() {
  if (active.value === "modpack-install") { active.value = modpackReturnTo.value; return; }
  if (active.value === "install") {
    // 加载器版本子页：先退回加载器列表，再退就是离开安装页
    if (showLoaderVersions.value) { backToLoaderSelect(); return; }
    cancelInstall();
    return;
  }
  if (active.value === "detail") { active.value = "search"; }
  else if (active.value === "search") { active.value = "resource"; }
  else { active.value = "main"; }
}

// ── 安装游戏：整页流程（替代早期弹窗） ──
function openInstallPage(verId: string) {
  openGameInstall(verId);
  active.value = "install";
}
function cancelInstall() {
  closeGameInstall();
  active.value = "game";
}
async function confirmInstall() {
  await doInstallGame();
  active.value = "game";
}
/** 加载器 id → 中文名（摘要栏用） */
function loaderLabel(id: string) {
  return loaderOptions.find((l) => l.id === id)?.zh || "原版";
}
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <button v-if="active !== 'main'" class="dl-back-btn" @click="goBack">
        ← <span class="bl-zh">返回</span><span class="bl-en">BACK</span>
      </button>
      <h2 class="page-title">
        <span class="bl-zh">{{ sectionTitle[active]?.zh }}</span>
        <span class="bl-en">{{ sectionTitle[active]?.en }}</span>
      </h2>
      <span class="page-line"></span>
    </div>

    <Transition name="dl-view">
      <!-- 主分类 -->
      <div v-if="active === 'main'" key="main" class="dl-main-grid">
        <div class="dl-main-card" v-for="cat in categories" :key="cat.id" @click="cat.bbsmcType ? openSearch({ id: cat.id, zh: cat.zh, en: cat.en, bbsmcType: cat.bbsmcType }) : (active = cat.id)">
          <span class="dl-main-icon">{{ cat.icon }}</span>
          <span class="dl-main-zh">{{ cat.zh }}</span>
          <span class="dl-main-en">{{ cat.en }}</span>
          <span class="dl-main-desc">{{ cat.desc }}</span>
          <ul class="dl-main-meta"><li v-for="m in cat.meta" :key="m">{{ m }}</li></ul>
          <span class="dl-main-go">进入 <span aria-hidden="true">›</span></span>
        </div>
      </div>

      <!-- 游戏版本 -->
      <div v-else-if="active === 'game'" key="game" class="dl-game-list">
        <div v-for="item in gameVersions" :key="item.name"
          :class="['dl-sub-card', 'dl-game-card', { expanded: gameExpanded === item.name || gameLeaving === item.name }]"
          @click="toggleGameCard(item.name)">
          <div class="dl-sub-head">
            <span class="dl-sub-name">{{ item.zh }}</span>
            <span class="dl-sub-tag">{{ item.en }}</span>
            <span class="dl-game-arrow" :class="{ open: gameExpanded === item.name }">▾</span>
          </div>
          <div class="dl-sub-desc">{{ item.desc }}</div>
          <!-- 展开的版本列表 -->
          <Transition name="dl-game-versions">
            <div v-if="gameExpanded === item.name" class="dl-game-versions">
            <div v-if="gameLoading" style="padding:8px;color:var(--text-dim)">加载中...</div>
            <div v-else class="dl-game-ver-list">
              <div v-for="v in gameVersionList" :key="v.id" class="dl-game-ver-row"
                @click.stop="openInstallPage(v.id)">
                <span class="dl-game-ver-id">{{ v.id }}</span>
                <span class="dl-game-ver-type">{{ v.type }}</span>
                <span v-if="forgeMcVersions.includes(v.id)" class="dl-forge-tag" title="Forge 可用">⚒</span>
                <span class="dl-game-ver-time">{{ v.releaseTime.slice(0,10) }}</span>
              </div>
            </div>
          </div>
          </Transition>
        </div>
      </div>

      <!-- 资源下载子分类 -->
      <div v-else-if="active === 'resource'" key="resource" class="dl-sub-list">
        <div class="dl-sub-card" v-for="item in resourceTypes" :key="item.id" @click="openSearch(item)">
          <div class="dl-sub-head"><span class="dl-sub-name">{{ item.zh }}</span><span class="dl-sub-tag">{{ item.en }}</span></div>
          <div class="dl-sub-desc">{{ item.desc }}</div>
          <span class="dl-arrow">→</span>
        </div>
      </div>

      <!-- 搜索页 -->
      <div v-else-if="active === 'search'" key="search" class="dl-search-view">
        <div class="dl-search-bar" style="margin-bottom:6px;border:none;background:none;padding:0;gap:8px">
          <button :class="['toggle-btn', { active: modSource === 'Modrinth' }]" @click="modSource = 'Modrinth'; doSearch(0)">Modrinth</button>
          <button :class="['toggle-btn', { active: modSource === 'CurseForge' }]" @click="modSource = 'CurseForge'; doSearch(0)">CurseForge</button>
        </div>
        <div class="dl-search-bar">
          <input class="dl-search-input" v-model="searchQuery" :placeholder="`搜索${searchType}`" @keyup.enter="doSearch(0)" @input="debouncedSearch" />
          <button v-if="currentBbsmcType === 'modpack'" class="dl-install-btn dl-local-btn" style="flex-shrink:0"
            title="管理员身份运行时，Windows 会拦截从资源管理器拖放文件，可用此入口选择本地整合包"
            @click="pickLocalPack">
            <span class="bl-zh">本地整合包</span>
          </button>
          <button class="dl-install-btn" style="flex-shrink:0" @click="searchNow" :disabled="searchLoading">
            <span class="bl-zh">{{ searchLoading ? '搜索中...' : '搜索' }}</span>
          </button>
        </div>
        <div class="dl-filters">
          <div class="dl-filter-group">
            <span class="dl-filter-label">版本</span>
            <div class="custom-select" style="width:120px" @click="showVersionDropdown = !showVersionDropdown">
              <span class="set-select-val">{{ searchVersion || "全部" }}</span>
              <span class="set-select-arrow" :class="{ open: showVersionDropdown }">▾</span>
              <Transition name="drop">
                <div v-if="showVersionDropdown" class="set-select-options">
                  <div v-for="v in searchFilterVersions" :key="v" :class="['set-select-opt', { active: searchVersion === v || (v === '全部' && !searchVersion) }]" @click.stop="setVersion(v)">{{ v }}</div>
                </div>
              </Transition>
            </div>
          </div>
          <div class="dl-filter-group">
            <span class="dl-filter-label">排序</span>
            <button v-for="s in sortOptions" :key="s.value" :class="['dl-filter-btn', { active: searchSort === s.value }]" @click="searchSort = s.value; doSearch(0)">{{ s.zh }}</button>
          </div>
        </div>
        <!-- 结果列表 -->
        <div class="dl-results" v-if="searchResults.length > 0">
          <div class="dl-result-card" v-for="r in searchResults" :key="r.project_id" @click="openDetail(r)">
            <img class="dl-result-icon" :src="r.icon_url" v-if="r.icon_url" />
            <div class="dl-result-info">
              <div class="dl-result-title">{{ r.title }}</div>
              <div class="dl-result-desc">{{ (r.description || '').slice(0, 120) }}</div>
              <div class="dl-result-meta">
                <span class="dl-result-type">{{ r.project_type }}</span>
                <span>⬇ {{ r.downloads }}</span>
                <span>by {{ r.author }}</span>
                <span v-for="v in (r.versions||[]).slice(0,5)" :key="v" class="dl-result-ver">{{ v }}</span>
                <span v-if="getMCVersion() && currentBbsmcType !== 'modpack'" :class="'dl-compat-badge ' + compatBadge(r.versions).cls">{{ compatBadge(r.versions).text }}</span>
              </div>
            </div>
            <span class="dl-result-go" aria-hidden="true">›</span>
          </div>
          <!-- 分页 -->
          <div class="dl-pagination" v-if="searchTotal > searchLimit">
            <button class="dl-page-btn" :disabled="searchOffset === 0" @click="doSearch(Math.floor(searchOffset/searchLimit)-1)">← 上一页</button>
            <span class="dl-page-info">{{ Math.floor(searchOffset/searchLimit)+1 }} / {{ Math.ceil(searchTotal/searchLimit) }}</span>
            <button class="dl-page-btn" :disabled="searchOffset+searchLimit >= searchTotal" @click="doSearch(Math.floor(searchOffset/searchLimit)+1)">下一页 →</button>
          </div>
        </div>
        <div class="dl-results-error" v-else-if="searchError">
          <span class="bl-zh">搜索失败: {{ searchError }}</span>
          <span class="bl-en">SEARCH FAILED</span>
        </div>
        <div class="dl-results-empty" v-else-if="searchRequested && !searchLoading">
          <span class="bl-zh">没有找到结果，换个关键词试试</span>
          <span class="bl-en">NO RESULTS FOUND</span>
        </div>
        <div class="dl-results-empty" v-else-if="!searchLoading">
          <span class="bl-zh">输入关键词搜索{{ searchType }}</span>
          <span class="bl-en">Search for {{ searchType }}</span>
        </div>
        <div class="dl-results-empty" v-if="searchLoading">
          <span class="bl-zh">加载中...</span>
          <span class="bl-en">LOADING...</span>
        </div>

        <Transition name="dl-view">
          <div v-if="installMsg" class="dl-install-msg">{{ installMsg }}</div>
        </Transition>
      </div>

      <!-- 安装游戏（整页流程，替代早期弹窗） -->
      <div v-else-if="active === 'install'" key="install" class="dl-install-view">
        <!-- 加载器版本子页 -->
        <template v-if="showLoaderVersions">
          <div class="dl-install-subhead">
            <button class="dl-back-btn" @click="backToLoaderSelect()">
              ← <span class="bl-zh">选择加载器</span><span class="bl-en">LOADERS</span>
            </button>
            <span class="dl-install-subver">{{ selectedGameVer }} · {{ loaderLabel(pendingLoader) }}</span>
          </div>
          <div v-if="loaderVersionLoading" class="dl-install-empty">加载中...</div>
          <div v-else-if="loaderVersionList.length === 0" class="dl-install-empty">
            未找到适用于 {{ selectedGameVer }} 的 {{ loaderLabel(pendingLoader) }} 版本
          </div>
          <div v-else class="dl-ver-grid">
            <button v-for="v in loaderVersionList" :key="v.version" class="dl-ver-card"
              :class="{ active: selectedLoader === pendingLoader && selectedLoaderVer === v.version }"
              @click="confirmLoaderVersion(v.version)">
              <span class="dl-ver-card-num">{{ v.version }}</span>
              <span v-if="v.stable && loaderVersionList[0]?.version === v.version" class="dl-ver-card-tag">推荐</span>
            </button>
          </div>
        </template>

        <!-- 主页面：左选加载器 / 右安装摘要 -->
        <template v-else>
          <div class="dl-install-grid">
            <div class="dl-install-main">
              <div class="dl-install-hero">
                <span class="dl-install-hero-label">MINECRAFT</span>
                <span class="dl-install-hero-ver">{{ selectedGameVer }}</span>
              </div>
              <p class="dl-install-hint">选择模组加载器；不选则安装纯净原版。点击整行进入版本选择。</p>

              <div class="dl-loader-list">
                <button v-for="loader in loaderOptions" :key="loader.id"
                  class="dl-loader-row"
                  :class="{ active: selectedLoader === loader.id, off: isLoaderDisabled(loader) || !isLoaderCompatible(loader) }"
                  :disabled="isLoaderDisabled(loader) || !isLoaderCompatible(loader)"
                  @click="openLoaderVersions(loader.id)">
                  <span class="dl-loader-dot">{{ selectedLoader === loader.id ? '●' : '○' }}</span>
                  <span class="dl-loader-info">
                    <span class="dl-loader-name">
                      {{ loader.zh }}
                      <span v-if="getRecommendedLoader() === loader.id && isLoaderCompatible(loader)" class="dl-loader-badge">推荐</span>
                    </span>
                    <span class="dl-loader-desc">
                      {{ isLoaderCompatible(loader) ? loader.desc : `不兼容 · ${getLoaderIncompatReason(loader)}` }}
                    </span>
                  </span>
                  <span v-if="selectedLoader === loader.id && selectedLoaderVer" class="dl-loader-picked">{{ selectedLoaderVer }}</span>
                  <span v-else-if="isLoaderCompatible(loader) && !isLoaderDisabled(loader)" class="dl-loader-arrow">›</span>
                </button>
              </div>
            </div>

            <aside class="dl-install-side">
              <div class="dl-side-title">安装摘要<span>SUMMARY</span></div>

              <label class="dl-side-field">
                <span class="dl-side-label">实例名称</span>
                <input class="form-input" v-model="installGameInstanceName"
                  :placeholder="gameInstallName()" @keyup.enter="confirmInstall" />
              </label>

              <div class="dl-side-row"><span>游戏版本</span><b>{{ selectedGameVer }}</b></div>
              <div class="dl-side-row"><span>加载器</span><b>{{ selectedLoader ? loaderLabel(selectedLoader) : '原版' }}</b></div>
              <div class="dl-side-row"><span>加载器版本</span><b>{{ selectedLoaderVer || '—' }}</b></div>
              <div class="dl-side-row"><span>实例目录</span><b class="dl-side-path">versions/{{ installGameInstanceName || gameInstallName() }}</b></div>

              <div class="dl-side-actions">
                <button class="acct-type-btn cancel" @click="cancelInstall">取消</button>
                <button class="acct-type-btn confirm" @click="confirmInstall">确认安装</button>
              </div>
            </aside>
          </div>
        </template>
      </div>
      <!-- 安装整合包（整页流程） -->
      <div v-else-if="active === 'modpack-install'" key="modpack-install" class="dl-install-view">
        <div class="dl-install-grid">
          <div class="dl-install-main">
            <div class="dl-install-hero">
              <span class="dl-install-hero-label">MODPACK</span>
              <span class="dl-install-hero-ver">{{ projectDetail?.title || modpackInstanceName }}</span>
            </div>

            <div class="dl-modpack-file">
              <span class="dl-modpack-file-icon">◫</span>
              <span class="dl-modpack-file-name">{{ modpackFileName }}</span>
              <span class="dl-modpack-file-size" v-if="modpackFileSize">
                {{ (modpackFileSize / 1024 / 1024).toFixed(1) }} MB
              </span>
            </div>

            <p class="dl-install-hint">
              安装过程会自动完成下列步骤，进度可在「任务列表」查看；中途可在任务卡上取消。
            </p>
            <div class="dl-modpack-steps">
              <div class="dl-modpack-step"><b>01</b><span>下载整合包</span></div>
              <div class="dl-modpack-step"><b>02</b><span>解压 overrides</span></div>
              <div class="dl-modpack-step"><b>03</b><span>按清单拉取模组</span></div>
              <div class="dl-modpack-step"><b>04</b><span>安装对应加载器</span></div>
            </div>

            <div class="dl-modpack-about" v-if="projectDetail?.description">
              <div class="dl-side-title">整合包简介<span>ABOUT</span></div>
              <p>{{ projectDetail.description }}</p>
            </div>
          </div>

          <aside class="dl-install-side">
            <div class="dl-side-title">安装摘要<span>SUMMARY</span></div>

            <label class="dl-side-field">
              <span class="dl-side-label">实例名称</span>
              <input class="form-input" v-model="modpackInstanceName" @input="checkConflict"
                @keyup.enter="!modpackConflict && confirmModpackInstall()" />
            </label>
            <p v-if="modpackConflict" class="dl-side-error">该实例名已存在，请更换名称</p>

            <div class="dl-side-row"><span>来源文件</span><b class="dl-side-path">{{ modpackFileName }}</b></div>
            <div class="dl-side-row"><span>目标目录</span><b class="dl-side-path">versions/{{ modpackInstanceName.trim() || '…' }}</b></div>

            <div class="dl-side-actions">
              <button class="acct-type-btn cancel" @click="active = modpackReturnTo">取消</button>
              <button class="acct-type-btn confirm"
                :disabled="modpackConflict || !modpackInstanceName.trim()"
                @click="confirmModpackInstall">开始安装</button>
            </div>
          </aside>
        </div>
      </div>
      <!-- 项目详情 -->
      <div v-else key="detail" class="dl-detail-view">
        <div v-if="!projectDetail" class="dl-results-empty">
          <span class="bl-zh">加载中...</span>
        </div>
        <template v-else>
          <div class="dl-detail-header">
            <img class="dl-detail-icon" :src="projectDetail.icon_url" v-if="projectDetail.icon_url" />
            <div class="dl-detail-head-info">
              <div class="dl-detail-name">{{ projectDetail.title }}</div>
              <div class="dl-detail-meta">by {{ projectDetail.author }} | ⬇ {{ projectDetail.downloads }} | ♥ {{ projectDetail.followers }}</div>
              <div class="dl-detail-desc">{{ (projectDetail.description || '').slice(0, 300) }}</div>
            </div>
          </div>
          <div class="dl-detail-versions">
            <!-- 版本兼容性提示 -->
            <div v-if="getMCVersion() && currentBbsmcType !== 'modpack'"
              :class="'dl-compat-warn-banner ' + compatBadge(projectDetail.game_versions || []).cls"
              style="padding:6px 12px;margin-bottom:8px;font-size:12px;border-radius:4px">
              {{ compatBadge(projectDetail.game_versions || []).text }}（当前实例: {{ getMCVersion() }}）
            </div>
            <div style="display:flex;align-items:center;gap:10px;margin-bottom:8px">
              <h3 class="dl-section-title" style="margin-bottom:0;border:none"><span class="bl-zh">版本列表</span><span class="bl-en">VERSIONS</span></h3>
              <div class="custom-select" style="width:110px" @click="showDetailVerDropdown = !showDetailVerDropdown">
                <span class="set-select-val">{{ detailVersionFilter }}</span>
                <span class="set-select-arrow" :class="{ open: showDetailVerDropdown }">▾</span>
                <Transition name="drop">
                  <div v-if="showDetailVerDropdown" class="set-select-options">
                    <div v-for="v in detailFilterOptions()" :key="v" :class="['set-select-opt', { active: detailVersionFilter === v }]" @click.stop="detailVersionFilter = v; showDetailVerDropdown = false">{{ v }}</div>
                  </div>
                </Transition>
              </div>
            </div>
            <div class="dl-version-card" v-for="v in filteredDetailVersions()" :key="v.id">
              <div class="dl-ver-head">
                <span class="dl-ver-name">{{ v.name }}</span>
                <span class="dl-ver-num">{{ v.version_number }}</span>
              </div>
              <div class="dl-ver-loaders">
                <span class="dl-ver-tag" v-for="l in v.loaders" :key="l">{{ l }}</span>
                <span class="dl-ver-tag" v-for="gv in (v.game_versions||[]).slice(0,5)" :key="gv">{{ gv }}</span>
              </div>
              <div class="dl-ver-files">
                <div class="dl-file-row" v-for="f in v.files" :key="f.filename">
                  <span class="dl-file-name">{{ f.filename }}</span>
                  <span class="dl-file-size">{{ (f.size / 1024 / 1024).toFixed(1) }} MB</span>
                  <button v-if="currentBbsmcType !== 'modpack' && modSource !== 'CurseForge'" class="dl-install-btn" @click.stop="installFile(f.url, f.filename)">安装</button>
                  <button v-else-if="currentBbsmcType !== 'modpack' && modSource === 'CurseForge'" class="dl-install-btn" @click.stop="installCfFile({ id: (v as any).id || (f as any).id || 0, fileName: f.filename, fileLength: f.size, downloadUrl: f.url, projectType: currentBbsmcType })">安装</button>
                  <button v-else class="dl-install-btn" @click.stop="openModpackInstall(f, v.name)">安装</button>
                </div>
              </div>
            </div>
          </div>
        </template>
      </div>
    </Transition>

  </section>
</template>
