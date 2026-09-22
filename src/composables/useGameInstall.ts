/**
 * 游戏版本安装向导 — 从 DownloadsPage.vue 抽出的独立状态机
 *
 * 负责三件事：
 * 1. 原版版本列表的展开/收起（release / snapshot / legacy）
 * 2. 加载器选择（含版本列表加载与兼容性判定）
 * 3. 提交安装任务并驱动下载任务状态
 */
import { ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { versionCmp } from "../utils/version";
import { gameSourceParam, readUserConfig } from "../utils/source";

export interface LoaderOption {
  id: string;
  zh: string;
  en: string;
  conflicts: string[];
  desc: string;
  minVer?: string;
  maxVer?: string;
}

export interface GameVersionEntry {
  id: string;
  type: string;
  url: string;
  time: string;
  releaseTime: string;
}

/** 向导依赖外部注入的下载任务管理接口 */
export interface GameInstallDeps {
  addDlTask: (name: string) => number;
  updateDlTask: (id: number, status: string) => void;
  finishDlTask: (id: number, error?: string) => void;
  /** 页面级提示文案，由向导写入 */
  installMsg: Ref<string>;
}

export const GAME_VERSION_GROUPS = [
  { name: "release", zh: "正式版", en: "Release", desc: "稳定版本，推荐使用" },
  { name: "snapshot", zh: "快照版", en: "Snapshot", desc: "预览下周目内容" },
  { name: "old_beta old_alpha", zh: "旧版", en: "Legacy", desc: "历史版本归档" },
];

export const LOADER_OPTIONS: LoaderOption[] = [
  { id: "", zh: "无（原版）", en: "Vanilla", conflicts: [], desc: "不安装任何模组加载器" },
  { id: "forge", zh: "Forge", en: "Forge", conflicts: ["fabric", "quilt"], desc: "最广泛的模组生态" },
  { id: "fabric", zh: "Fabric", en: "Fabric", conflicts: ["forge", "neoforge", "cleanroom"], desc: "轻量高性能加载器", minVer: "1.14" },
  { id: "neoforge", zh: "NeoForge", en: "NeoForge", conflicts: ["forge", "fabric", "quilt"], desc: "Forge 社区分支", minVer: "1.20.1" },
  { id: "quilt", zh: "Quilt", en: "Quilt", conflicts: ["forge", "neoforge", "cleanroom"], desc: "Fabric 社区分支", minVer: "1.18", maxVer: "1.20.4" },
  { id: "cleanroom", zh: "Cleanroom", en: "Cleanroom", conflicts: ["forge", "neoforge", "fabric", "quilt"], desc: "Forge 1.12.2 轻量替代", minVer: "1.12.2", maxVer: "1.12.2" },
];

export function useGameInstall(deps: GameInstallDeps) {
  const { addDlTask, updateDlTask, finishDlTask, installMsg } = deps;

  // ── 原版版本列表展开 ──
  const gameVersions = GAME_VERSION_GROUPS;
  const gameExpanded = ref("");
  const gameLeaving = ref(""); // 正在播放收起动画的卡片
  const gameVersionList = ref<GameVersionEntry[]>([]);
  const gameLoading = ref(false);
  const forgeMcVersions = ref<string[]>([]); // Forge 支持的 MC 版本，用于兼容标记

  async function toggleGameCard(type: string) {
    if (gameExpanded.value === type) {
      // 收起：清 gameExpanded 触发 leave，但保留 expanded 类到动画结束
      gameExpanded.value = "";
      gameLeaving.value = type;
      setTimeout(() => { gameLeaving.value = ""; }, 300);
      return;
    }
    if (gameExpanded.value) gameExpanded.value = "";
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
      try {
        forgeMcVersions.value = await invoke<string[]>("fetch_forge_mc_versions");
      } catch { forgeMcVersions.value = []; }
    } catch { /* 拉取失败时保持空列表 */ }
    gameLoading.value = false;
  }

  /** 展开中或正在播放收起动画时都需要保留列表可见 */
  function isGameCardExpanded(itemName: string) {
    return gameExpanded.value === itemName || gameLeaving.value === itemName;
  }

  // ── 安装对话框 ──
  const showGameInstall = ref(false);
  const selectedGameVer = ref("");
  const selectedLoader = ref("");
  const selectedLoaderVer = ref("");
  const installGameInstanceName = ref("");

  // ── 加载器版本选择子页 ──
  const showLoaderVersions = ref(false);
  const loaderVersionList = ref<{ version: string; stable: boolean }[]>([]);
  const loaderVersionLoading = ref(false);
  const pendingLoader = ref("");

  async function openLoaderVersions(loaderId: string) {
    if (!loaderId) {
      selectedLoader.value = "";
      selectedLoaderVer.value = "";
      return;
    }
    pendingLoader.value = loaderId;
    showLoaderVersions.value = true;
    loaderVersionList.value = [];
    loaderVersionLoading.value = true;
    try {
      if (loaderId === "forge") {
        const list = await invoke<any[]>("fetch_forge_versions", { mcVersion: selectedGameVer.value });
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
            version: v.version,
            stable: v.stable,
          }));
        }
      } else if (loaderId === "cleanroom") {
        const list = await invoke<any[]>("fetch_cleanroom_versions");
        loaderVersionList.value = (list || []).map((v: any) => ({
          version: v.version,
          stable: !v.isBeta,
        }));
      }
    } catch { /* 加载失败时保持空列表，用户可返回 */ }
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

  // ── 加载器兼容性 ──
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

  function isLoaderDisabled(loader: LoaderOption): boolean {
    if (!selectedLoader.value) return false;
    const sel = LOADER_OPTIONS.find(l => l.id === selectedLoader.value);
    if (!sel) return false;
    return sel.conflicts.includes(loader.id) || loader.conflicts.includes(sel.id);
  }

  function selectLoader(id: string) {
    selectedLoader.value = id;
  }

  /** 默认实例名：<MC版本>-<Loader> <Loader版本> */
  function gameInstallName(): string {
    if (!selectedLoader.value) return selectedGameVer.value;
    const loader = LOADER_OPTIONS.find(l => l.id === selectedLoader.value);
    const base = loader ? `${selectedGameVer.value}-${loader.en}` : selectedGameVer.value;
    return selectedLoaderVer.value ? `${base} ${selectedLoaderVer.value}` : base;
  }

  async function doInstallGame() {
    const name = installGameInstanceName.value.trim() || gameInstallName();
    showGameInstall.value = false;
    const taskId = addDlTask(name);
    installMsg.value = `正在安装 ${name}...`;
    let success = false;
    let errorMsg = "";
    try {
      const mcDir = await invoke<string>("get_minecraft_dir");
      // install_game 的 source 走的是游戏源语义（后端比较 "Mojang"），
      // 不能把 mod 源(getModSource)传进来，否则选了官方源也照样走镜像
      const source = gameSourceParam(await readUserConfig());
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
      setTimeout(() => {
        if (installMsg.value === `安装失败: ${errorMsg}` || installMsg.value === `正在安装 ${name}...`) {
          installMsg.value = "";
        }
      }, 5000);
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
    installGameInstanceName.value = "";
    showGameInstall.value = true;
  }

  return {
    gameVersions,
    gameExpanded,
    gameLeaving,
    gameVersionList,
    gameLoading,
    toggleGameCard,
    isGameCardExpanded,
    showGameInstall,
    selectedGameVer,
    forgeMcVersions,
    selectedLoader,
    selectedLoaderVer,
    installGameInstanceName,
    showLoaderVersions,
    loaderVersionList,
    loaderVersionLoading,
    pendingLoader,
    openLoaderVersions,
    confirmLoaderVersion,
    backToLoaderSelect,
    loaderOptions: LOADER_OPTIONS,
    isLoaderCompatible,
    getRecommendedLoader,
    getLoaderIncompatReason,
    isLoaderDisabled,
    selectLoader,
    gameInstallName,
    doInstallGame,
    closeGameInstall,
    openGameInstall,
  };
}
