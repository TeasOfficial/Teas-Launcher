<script setup lang="ts">
import { ref, inject } from "vue";
import { invoke } from "@tauri-apps/api/core";

const activeInstance = inject<any>("activeInstance");

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

type Section = "main" | "game" | "modpack" | "resource" | "search" | "detail";

const active = ref<Section>("main");
const searchType = ref("");
const modSource = ref("Modrinth"); // Modrinth 或 CurseForge

const categories = [
  { id: "game" as Section, zh: "原版游戏", en: "VANILLA", icon: "▣" },
  { id: "modpack" as Section, zh: "整合包", en: "MODPACKS", icon: "◫", bbsmcType: "modpack" },
  { id: "resource" as Section, zh: "模组资源", en: "MODS & MORE", icon: "⬡" },
];

const gameVersions = [
  { name: "release", zh: "正式版", en: "Release", desc: "稳定版本，推荐使用" },
  { name: "snapshot", zh: "快照版", en: "Snapshot", desc: "预览下周目内容" },
  { name: "old_beta old_alpha", zh: "旧版", en: "Legacy", desc: "历史版本归档" },
];
const gameExpanded = ref("");
const gameLeaving = ref(""); // 正在收起的卡片类型
const gameVersionList = ref<{ id: string; type: string; url: string; time: string; releaseTime: string }[]>([]);
const gameLoading = ref(false);

async function toggleGameCard(type: string) {
  if (gameExpanded.value === type) {
    // 收起：先清 gameExpanded 触发 Vue leave，但保持 expanded 类到动画结束
    gameExpanded.value = "";
    gameLeaving.value = type;
    setTimeout(() => { gameLeaving.value = ""; }, 300);
    return;
  }
  // 展开其他卡片时，如果当前有展开的先收起
  if (gameExpanded.value) { gameExpanded.value = ""; }
  gameExpanded.value = type;
  gameVersionList.value = [];
  gameLoading.value = true;
  try {
    const manifest = await invoke<any>("fetch_version_manifest");
    const all: any[] = manifest.versions || [];
    const filters = type.split(" ");
    gameVersionList.value = all
      .filter((v: any) => filters.includes(v.type))
      .sort((a, b) => b.releaseTime.localeCompare(a.releaseTime));
    // ★ 同时获取 Forge 支持的 MC 版本列表（用于标记兼容性）
    try {
      forgeMcVersions.value = await invoke<string[]>("fetch_forge_mc_versions");
    } catch { forgeMcVersions.value = []; }
  } catch { /* */ }
  gameLoading.value = false;
}

// 判断是否应该显示版本列表（展开或正在离开动画中）
function isGameCardExpanded(itemName: string) {
  return gameExpanded.value === itemName || gameLeaving.value === itemName;
}

// ── 游戏版本安装对话框 ──
const showGameInstall = ref(false);
const selectedGameVer = ref("");
const forgeMcVersions = ref<string[]>([]); // Forge 支持的 MC 版本 (用于标记兼容)
const selectedLoader = ref("");
const selectedLoaderVer = ref("");

// 加载器版本选择子页面
const showLoaderVersions = ref(false);
const loaderVersionList = ref<{ version: string; stable: boolean }[]>([]);
const loaderVersionLoading = ref(false);
const pendingLoader = ref("");

async function openLoaderVersions(loaderId: string) {
  if (!loaderId) { selectedLoader.value = ""; selectedLoaderVer.value = ""; return; }
  pendingLoader.value = loaderId;
  showLoaderVersions.value = true;
  loaderVersionList.value = [];
  loaderVersionLoading.value = true;
  try {
    if (loaderId === "forge") {
      const list = await invoke<any[]>("fetch_forge_versions", {
        mcVersion: selectedGameVer.value,
      });
      loaderVersionList.value = (list || []).map(v => ({
        version: v.version,
        stable: v.category === "installer",
      }));
    } else if (loaderId === "neoforge") {
      const list = await invoke<any[]>("fetch_neoforge_versions");
      loaderVersionList.value = (list || [])
        .filter((v: any) => v.mc_version === selectedGameVer.value || v.mc_version === "")
        .map((v: any) => ({ version: v.version, stable: v.stable }));
    } else if (loaderId === "fabric" || loaderId === "quilt") {
      const fnName = loaderId === "fabric" ? "fetch_fabric_versions" : "fetch_quilt_versions";
      const full = await invoke<any>(fnName);
      const gameNormalized = selectedGameVer.value.replace("∞", "infinite").replace("Combat Test 7c", "1.16_combat-3");
      const supported = (full.game || []).some((g: any) => g.version === gameNormalized);
      if (supported) {
        loaderVersionList.value = (full.loader || []).map((v: any) => ({
          version: v.version, stable: v.stable,
        }));
      }
    } else if (loaderId === "cleanroom") {
      const list = await invoke<any[]>("fetch_cleanroom_versions");
      loaderVersionList.value = (list || []).map((v: any) => ({
        version: v.version, stable: !v.isBeta,
      }));
    }
  } catch { /* */ }
  loaderVersionLoading.value = false;
}

function confirmLoaderVersion(ver: string) {
  selectedLoader.value = pendingLoader.value;
  selectedLoaderVer.value = ver;
  showLoaderVersions.value = false;
}

function backToLoaderSelect() {
  showLoaderVersions.value = false;
  // 保持之前的选择不变
}

interface LoaderOption { id: string; zh: string; en: string; conflicts: string[]; desc: string; minVer?: string; maxVer?: string }
const loaderOptions: LoaderOption[] = [
  { id: "", zh: "无（原版）", en: "Vanilla", conflicts: [], desc: "不安装任何模组加载器" },
  { id: "forge", zh: "Forge", en: "Forge", conflicts: ["fabric", "quilt"], desc: "最广泛的模组生态" },
  { id: "fabric", zh: "Fabric", en: "Fabric", conflicts: ["forge", "neoforge", "cleanroom"], desc: "轻量高性能加载器", minVer: "1.14" },
  { id: "neoforge", zh: "NeoForge", en: "NeoForge", conflicts: ["forge", "fabric", "quilt"], desc: "Forge 社区分支", minVer: "1.20.1" },
  { id: "quilt", zh: "Quilt", en: "Quilt", conflicts: ["forge", "neoforge", "cleanroom"], desc: "Fabric 社区分支", minVer: "1.18", maxVer: "1.20.4" },
  { id: "cleanroom", zh: "Cleanroom", en: "Cleanroom", conflicts: ["forge", "neoforge", "fabric", "quilt"], desc: "Forge 1.12.2 轻量替代", minVer: "1.12.2", maxVer: "1.12.2" },
];

function parseMcVersion(ver: string): number[] {
  // 提取 "1.20.1" 中的数字部分
  const m = ver.match(/^(\d+)\.(\d+)(?:\.(\d+))?/);
  if (!m) return [0, 0, 0];
  return [parseInt(m[1]), parseInt(m[2]), parseInt(m[3] || "0")];
}

function versionCmp(a: string, b: string): number {
  const va = parseMcVersion(a), vb = parseMcVersion(b);
  for (let i = 0; i < 3; i++) { if (va[i] !== vb[i]) return va[i] - vb[i]; }
  return 0;
}

function isLoaderCompatible(loader: LoaderOption): boolean {
  if (!loader.minVer && !loader.maxVer) return true;
  const ver = selectedGameVer.value;
  if (loader.minVer && versionCmp(ver, loader.minVer) < 0) return false;
  if (loader.maxVer && versionCmp(ver, loader.maxVer) > 0) return false;
  return true;
}

function getRecommendedLoader(): string {
  return versionCmp(selectedGameVer.value, "1.21") >= 0 ? "neoforge" : "forge";
}

function getLoaderIncompatReason(loader: LoaderOption): string {
  if (loader.minVer && versionCmp(selectedGameVer.value, loader.minVer) < 0)
    return `需要 ${loader.minVer}+`;
  if (loader.maxVer && versionCmp(selectedGameVer.value, loader.maxVer) > 0)
    return `仅支持 ≤${loader.maxVer}`;
  return "";
}

const installGameInstanceName = ref("");

const gameInstallName = function(): string {
  if (!selectedLoader.value) return selectedGameVer.value;
  const loader = loaderOptions.find(l => l.id === selectedLoader.value);
  const base = loader ? `${selectedGameVer.value}-${loader.en}` : selectedGameVer.value;
  return selectedLoaderVer.value ? `${base} ${selectedLoaderVer.value}` : base;
};

async function doInstallGame() {
  const name = installGameInstanceName.value.trim() || gameInstallName();
  showGameInstall.value = false;
  const taskId = addDlTask(name);
  installMsg.value = `正在安装 ${name}...`;
  let success = false;
  let errorMsg = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const source = await getModSource();
    const msg = await invoke<string>("install_game", {
      mcDir, instanceName: name, mcVersion: selectedGameVer.value,
      loader: selectedLoader.value || null,
      loaderVersion: selectedLoaderVer.value || null,
      source,
    });
    installMsg.value = msg;
    success = true;
  } catch (e) {
    errorMsg = String(e);
    installMsg.value = `安装失败: ${errorMsg}`;
  } finally {
    updateDlTask(taskId, success ? "已完成" : "失败");
    finishDlTask(taskId, success ? undefined : errorMsg);
    setTimeout(() => { if (installMsg.value === `安装失败: ${errorMsg}` || installMsg.value === `正在安装 ${name}...`) installMsg.value = ""; }, 5000);
  }
  closeGameInstall();
}

function closeGameInstall() {
  showGameInstall.value = false;
  selectedLoader.value = "";
  selectedLoaderVer.value = "";
  showLoaderVersions.value = false;
}

function openGameInstall(verId: string) {
  selectedGameVer.value = verId;
  selectedLoader.value = "";
  selectedLoaderVer.value = "";
  showLoaderVersions.value = false;
  showGameInstall.value = true;
}

function selectLoader(id: string) {
  // 检查冲突：如果点击的 loader 与已选的冲突，直接切换
  selectedLoader.value = id;
}

function isLoaderDisabled(loader: LoaderOption): boolean {
  if (!selectedLoader.value) return false;
  // 已选的 loader 和当前 loader 互相冲突
  const sel = loaderOptions.find(l => l.id === selectedLoader.value);
  if (!sel) return false;
  return sel.conflicts.includes(loader.id) || loader.conflicts.includes(sel.id);
}

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
  resource: { zh: "资源浏览", en: "RESOURCES" },
  search: { zh: "搜索资源", en: "SEARCH" },
  detail: { zh: "资源详情", en: "DETAILS" },
};

const sources = ["全部", "CurseForge", "Modrinth", "BBSMC"];
const loaders = ["全部", "Forge", "Fabric", "NeoForge"];

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

const searchSource = ref("全部");
const searchSort = ref("relevance");
const searchQuery = ref("");
const searchVersion = ref("");
const searchResults = ref<SearchHit[]>([]);
const searchLoading = ref(false);
const searchTotal = ref(0);
const searchOffset = ref(0);
const currentBbsmcType = ref("");
const searchLimit = 20;

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
      const hits = (result.hits || []) as SearchHit[];
      searchResults.value = hits.filter(h => h.project_type === currentBbsmcType.value);
      searchTotal.value = result.total_hits || 0;
    }
  } catch { /* */ }
  searchLoading.value = false;
}

interface CfFile { id: number; fileName: string; fileLength: number; downloadUrl: string; }
interface CfVersion { id: number; name: string; gameVersions: string[]; loaders: string[]; files: CfFile[]; }
interface VersionFile { filename: string; url: string; size: number; }
interface ProjectVersion { id: string; name: string; version_number: string; loaders: string[]; game_versions: string[]; files: VersionFile[]; }
interface ProjectDetail { title: string; description: string; icon_url: string; downloads: number; followers: number; author: string; versions: ProjectVersion[]; }

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

const showModpackDialog = ref(false);
const modpackFileName = ref("");
const modpackFileUrl = ref("");
const modpackInstanceName = ref("");
const modpackConflict = ref(false);

function openModpackInstall(f: VersionFile, versionName: string) {
  modpackFileName.value = f.filename;
  modpackFileUrl.value = f.url;
  modpackInstanceName.value = versionName || f.filename;
  modpackConflict.value = false;
  showModpackDialog.value = true;
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
  showModpackDialog.value = false;
  const taskId = addDlTask(modpackInstanceName.value);
  console.log(`[DL] confirmModpackInstall 开始: taskId=${taskId} name=${modpackInstanceName.value}`);
  installMsg.value = `正在安装整合包 ${modpackInstanceName.value}...`;
  let success = false;
  let errorMsg = "";
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const source = await getModSource();
    const dlThreads = parseInt((await invoke("config_read", {scope:"user"})).dl_threads) || 8;
    console.log(`[DL] 调用 install_modpack: ${modpackFileName.value} source=${source} threads=${dlThreads}`);
    const msg = await invoke<string>("install_modpack", {
      mcDir, url: modpackFileUrl.value, filename: modpackFileName.value,
      instanceName: modpackInstanceName.value.trim(), source, dlThreads,
    });
    console.log(`[DL] install_modpack 返回成功: "${msg}"`);
    installMsg.value = msg;
    success = true;
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

const installMsg = ref("");
const addDlTask = inject<(name: string) => number>("addDlTask", () => { console.error("[DL] addDlTask 注入失败!"); return 0; });
const updateDlTask = inject<(id: number, status: string) => void>("updateDlTask", () => { console.error("[DL] updateDlTask 注入失败!"); });
const finishDlTask = inject<(id: number, error?: string) => void>("finishDlTask", () => { console.error("[DL] finishDlTask 注入失败!"); });

async function getModSource() { try { const cfg=await invoke("config_read",{scope:"user"}); return cfg.mod_source||"MCIMirror"; } catch { return "MCIMirror"; } }

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

async function installProject(project: SearchHit) {
  installMsg.value = `正在安装 ${project.title.slice(0,30)}...`;
  try {
    const mcDir = await invoke<string>("get_minecraft_dir");
    const instName = activeInstance?.value?.name || "";
    const msg = await invoke<string>("install_project", {
      mcDir, projectId: project.project_id, projectType: currentBbsmcType.value,
      instanceName: instName,
    });
    installMsg.value = msg;
    setTimeout(() => { installMsg.value = ""; }, 4000);
  } catch (e) {
    installMsg.value = `安装失败: ${e}`;
    setTimeout(() => { installMsg.value = ""; }, 5000);
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
  if (active.value === "detail") { active.value = "search"; }
  else if (active.value === "search") { active.value = "resource"; }
  else { active.value = "main"; }
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

    <Transition name="dl-view" mode="out-in">
      <!-- 主分类 -->
      <div v-if="active === 'main'" key="main" class="dl-main-grid">
        <div class="dl-main-card" v-for="cat in categories" :key="cat.id" @click="cat.bbsmcType ? openSearch({ id: cat.id, zh: cat.zh, en: cat.en, bbsmcType: cat.bbsmcType }) : (active = cat.id)">
          <span class="dl-main-icon">{{ cat.icon }}</span>
          <span class="dl-main-zh">{{ cat.zh }}</span>
          <span class="dl-main-en">{{ cat.en }}</span>
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
                @click.stop="openGameInstall(v.id)">
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
          <input class="dl-search-input" v-model="searchQuery" :placeholder="`搜索${searchType}`" @keyup.enter="doSearch()" />
          <button class="dl-install-btn" style="flex-shrink:0" @click="doSearch()" :disabled="searchLoading">
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
          </div>
          <!-- 分页 -->
          <div class="dl-pagination" v-if="searchTotal > searchLimit">
            <button class="dl-page-btn" :disabled="searchOffset === 0" @click="doSearch(Math.floor(searchOffset/searchLimit)-1)">← 上一页</button>
            <span class="dl-page-info">{{ Math.floor(searchOffset/searchLimit)+1 }} / {{ Math.ceil(searchTotal/searchLimit) }}</span>
            <button class="dl-page-btn" :disabled="searchOffset+searchLimit >= searchTotal" @click="doSearch(Math.floor(searchOffset/searchLimit)+1)">下一页 →</button>
          </div>
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

    <!-- 整合包安装弹窗 -->
    <Transition name="modal">
      <div v-if="showModpackDialog" class="modal-overlay" @click.self="showModpackDialog = false">
        <div class="modal-panel" style="width:400px">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">安装整合包</span>
              <span class="bl-en">INSTALL MODPACK</span>
            </span>
            <button class="modal-close" @click="showModpackDialog = false">✕</button>
          </div>
          <div class="modal-body">
            <div class="form-field">
              <span class="form-label">实例名称 / Instance Name</span>
              <input class="form-input" v-model="modpackInstanceName" @input="checkConflict" @keyup.enter="!modpackConflict && confirmModpackInstall()" />
              <span v-if="modpackConflict" style="color:var(--danger);font-size:10px;margin-top:4px">该实例名已存在，请更换名称</span>
            </div>
            <div class="modal-actions">
              <button class="acct-type-btn cancel" @click="showModpackDialog = false">取消</button>
              <button class="acct-type-btn confirm" :disabled="modpackConflict" @click="confirmModpackInstall">安装</button>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- 游戏版本安装弹窗 -->
    <Transition name="modal">
      <div v-if="showGameInstall" class="modal-overlay" @click.self="closeGameInstall()">
        <div class="modal-panel" style="width:480px">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">安装游戏</span>
              <span class="bl-en">INSTALL GAME</span>
            </span>
            <button class="modal-close" @click="closeGameInstall()">✕</button>
          </div>
          <div class="modal-body" style="gap:8px;overflow-y:auto;max-height:60vh">
            <!-- 加载器版本选择子页面 -->
            <template v-if="showLoaderVersions">
              <div style="display:flex;align-items:center;gap:8px;margin-bottom:4px">
                <button class="dl-back-btn" @click="backToLoaderSelect()">← 返回</button>
                <span style="font-family:var(--font-mono);font-size:13px;color:var(--accent)">{{ gameInstallName() }}</span>
              </div>
              <div v-if="loaderVersionLoading" style="padding:20px;text-align:center;color:var(--text-dim)">加载中...</div>
              <div v-else class="dl-game-ver-list" style="max-height:300px;overflow-y:auto">
                <div v-for="v in loaderVersionList" :key="v.version" class="dl-game-ver-row"
                  style="cursor:pointer" @click="confirmLoaderVersion(v.version)">
                  <span class="dl-game-ver-id">{{ v.version }}</span>
                  <span v-if="v.stable" style="color:var(--accent);font-size:10px;border:1px solid var(--accent);padding:1px 4px">推荐</span>
                </div>
                <div v-if="loaderVersionList.length === 0" style="padding:20px;text-align:center;color:var(--text-dim)">
                  未找到适用于 {{ selectedGameVer }} 的版本
                </div>
              </div>
            </template>
            <!-- 加载器选择主页面 -->
            <template v-else>
            <div style="font-family:var(--font-mono);font-size:15px;color:var(--accent);text-align:center;word-break:break-all">
              {{ gameInstallName() }}
            </div>
            <div class="form-field">
              <span class="form-label">实例名称 / Instance Name</span>
              <input class="form-input" v-model="installGameInstanceName" :placeholder="gameInstallName()" @keyup.enter="doInstallGame()" />
            </div>
            <div class="modal-desc-sub" style="text-align:center">选择模组加载器 · 实例名称将自动生成</div>
            <div class="modal-types">
              <div v-for="loader in loaderOptions" :key="loader.id"
                :class="['acct-type-btn', { active: selectedLoader === loader.id }]"
                :style="(isLoaderDisabled(loader) || !isLoaderCompatible(loader)) ? {opacity:.3,pointerEvents:'none'} : {}"
                @click="openLoaderVersions(loader.id)">
                <span class="atb-icon" style="font-size:16px">{{ selectedLoader === loader.id ? '●' : '○' }}</span>
                <span class="atb-zh">{{ loader.zh }}
                  <span v-if="getRecommendedLoader() === loader.id && isLoaderCompatible(loader)" style="font-size:11px;color:var(--accent);margin-left:6px;border:1px solid var(--accent);padding:1px 6px;border-radius:2px">推荐</span>
                </span>
                <span class="atb-en">
                  {{ isLoaderCompatible(loader) ? loader.desc : `不兼容 (${getLoaderIncompatReason(loader)})` }}
                </span>
              </div>
            </div>
            <div class="modal-actions">
              <button class="acct-type-btn cancel" @click="closeGameInstall()">取消</button>
              <button class="acct-type-btn confirm" @click="doInstallGame()">
                <span class="bl-zh">确认安装</span>
                <span class="bl-en">CONFIRM</span>
              </button>
            </div>
            </template>
          </div>
        </div>
      </div>
    </Transition>
  </section>
</template>
