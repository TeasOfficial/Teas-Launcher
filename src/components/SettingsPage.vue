<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

const activeTab = ref("general");

const tabs = [
  { id: "general", zh: "通用", en: "GENERAL" },
  { id: "launch", zh: "启动", en: "LAUNCH" },
  { id: "network", zh: "网络", en: "NETWORK" },
  { id: "advanced", zh: "高级", en: "ADVANCED" },
  { id: "about", zh: "关于", en: "ABOUT" },
  ];
  const cfg = ref<Record<string, any>>({});

async function save(key: string, value: any) {
  try { await invoke("config_write", { scope: "user", key, value }); } catch { /* */ }
}

const windowPresets = [
  { label: "854 × 480 (默认)", w: "854", h: "480" },
  { label: "1280 × 720",       w: "1280", h: "720" },
  { label: "1920 × 1080",      w: "1920", h: "1080" },
  { label: "自定义",            w: "", h: "", custom: true },
];
const windowPreset = ref("854 × 480 (默认)");
const presetOpen = ref(false);

const memPresets = [
  { label: "2 GB",  value: "2048 MB" },
  { label: "4 GB",  value: "4096 MB" },
  { label: "6 GB",  value: "6144 MB" },
  { label: "8 GB",  value: "8192 MB" },
  { label: "12 GB", value: "12288 MB" },
  { label: "16 GB", value: "16384 MB" },
  { label: "自定义", value: "", custom: true },
];
const memPreset = ref("4 GB");
const memOpen = ref(false);

function selectMemPreset(p: typeof memPresets[0]) {
  memPreset.value = p.label;
  memOpen.value = false;
  if (!p.custom) { cfg.value.max_memory = p.value; save("max_memory", p.value); }
}
function onCustomMem(e: any) { cfg.value.max_memory = e.target.value; save("max_memory", e.target.value); }

const generalSettings = [
  { key: "close_after",   labelZh: "启动后关闭启动器", labelEn: "Close After Launch", def: "否", toggle: true },
];

interface JavaEntry { path: string; version: string; }

const javaList = ref<JavaEntry[]>([]);
const javaOpen = ref(false);
const javaVal = ref("javaw");

async function scanJava() {
  try { javaList.value = await invoke<JavaEntry[]>("scan_java"); } catch { /* */ }
}

function selectJava(entry: JavaEntry) {
  javaVal.value = entry.path;
  cfg.value.java_path = entry.path;
  save("java_path", entry.path);
  javaOpen.value = false;
}

async function pickCustomJava() {
  javaOpen.value = false;
  try {
    // 使用 Tauri dialog 打开文件选择器
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      filters: [{ name: "Java", extensions: ["exe"] }],
      defaultPath: javaVal.value,
    });
    if (selected) {
      const sp = selected as string;
      javaVal.value = sp;
      const norm = (p: string) => p.toLowerCase().replace(/\\/g, '/');
      if (!javaList.value.some(p => norm(p.path) === norm(sp))) {
        javaList.value.push({ path: sp, version: "自定义" });
      }
      cfg.value.java_path = selected as string;
      save("java_path", selected as string);
    }
  } catch { /* */ }
}

onMounted(async () => {
  try { cfg.value = await invoke("config_read", { scope: "user" }); } catch { /* */ }
  const w = cfg.value.window_width || "854";
  const h = cfg.value.window_height || "480";
  const match = windowPresets.find(p => p.w === w && p.h === h);
  windowPreset.value = match ? match.label : "自定义"; const memMatch = memPresets.find(p => p.value === (cfg.value.max_memory || "4096 MB")); memPreset.value = memMatch ? memMatch.label : (cfg.value.max_memory ? "自定义" : "4 GB");
  javaVal.value = cfg.value.java_path || "javaw";
  // 恢复代理设置
  proxyType.value = cfg.value.proxy_type || "None";
  proxyHost.value = cfg.value.proxy_host || "";
  proxyPort.value = cfg.value.proxy_port || "";
  proxyUser.value = cfg.value.proxy_user || "";
  proxyPass.value = cfg.value.proxy_pass || "";
  scanStrategy.value = cfg.value.jar_scan_strategy || "B";
  scanJava();
});

const gameSources = ["官方源", "BMCLAPI"];
const modSources = ["官方源", "MCIMirror"];
const gameSource = ref(cfg.value.game_source || "BMCLAPI");
const modSource = ref(cfg.value.mod_source || "MCIMirror");

function saveGameSource(v: string) { gameSource.value = v; save("game_source", v); }
function saveModSource(v: string) { modSource.value = v; save("mod_source", v); }

const dlThreads = ref(cfg.value.dl_threads || "8");

function saveDlThreads(v: string) {
  const n = Math.max(1, Math.min(64, parseInt(v) || 8));
  dlThreads.value = String(n);
  save("dl_threads", String(n));
}

// ═══════════════════════════ 代理设置
const proxyTypes = ["None", "HTTP", "SOCKS5"];
const proxyType = ref(cfg.value.proxy_type || "None");
const proxyHost = ref(cfg.value.proxy_host || "");
const proxyPort = ref(cfg.value.proxy_port || "");
const proxyUser = ref(cfg.value.proxy_user || "");
const proxyPass = ref(cfg.value.proxy_pass || "");
const proxyDetecting = ref(false);
const proxyStatus = ref("");
const proxyTesting = ref(false);
const proxyTestResults = ref<{ name: string; ok: boolean; status?: number; latency_ms?: number; error?: string }[]>([]);

function saveProxyType(v: string) { proxyType.value = v; save("proxy_type", v); }
function saveProxyField(key: string, value: string) { save(key, value); }

async function testProxy() {
  proxyTesting.value = true;
  proxyTestResults.value = [];
  try {
    const result = await invoke<any>("test_proxy_connectivity");
    proxyTestResults.value = result.results || [];
  } catch (e) {
    proxyTestResults.value = [{ name: "错误", ok: false, error: String(e) }];
  }
  proxyTesting.value = false;
}

async function detectProxy() {
  proxyDetecting.value = true;
  proxyStatus.value = "检测中...";
  try {
    const result = await invoke<any>("detect_system_proxy");
    if (result.enabled) {
      proxyType.value = result.proxy_type || "HTTP";
      proxyHost.value = result.host || "";
      proxyPort.value = String(result.port || "");
      save("proxy_type", proxyType.value);
      save("proxy_host", proxyHost.value);
      save("proxy_port", proxyPort.value);
      proxyStatus.value = `已检测到: ${result.host}:${result.port}`;
    } else {
      proxyStatus.value = "未检测到系统代理";
    }
  } catch (e) {
    proxyStatus.value = `检测失败: ${e}`;
  }
  proxyDetecting.value = false;
  setTimeout(() => { proxyStatus.value = ""; }, 4000);
}

const advancedSettings = [
  { key: "version_isolation",   labelZh: "版本隔离", labelEn: "Isolation",     def: "是" },
  { key: "log_level",           labelZh: "日志级别", labelEn: "Log Level",     def: "INFO" },
];

// ── JAR 扫描策略 ──
const scanStrategy = ref("B");
const scanHelpOpen = ref(false);
const scanStrategies = [
  { id: "A", label: "A — 快速预检", desc: "大小初筛，仅小文件校验", zh: "快速预检" },
  { id: "B", label: "B — PCL 风格", desc: "仅校验实例JAR，跳过全量扫描", zh: "PCL 风格" },
  { id: "C", label: "C — 并行扫描", desc: "多线程递归校验全量", zh: "并行扫描" },
];
function saveScanStrategy(id: string) {
  scanStrategy.value = id;
  save("jar_scan_strategy", id);
}

const scanHelpSections = [
  {
    id: "A", name: "A — 快速预检", recommend: false,
    pros: "速度快于全量扫描；用文件大小做初筛，跳过大文件(>100KB)的EOCD校验",
    cons: "小尺寸JAR(如部分mod)仍需打开读取；不能检测文件内容损坏但EOCD完好的情况",
    desc: "遍历 libraries 目录，对每个 JAR 先检查文件大小——空文件直接删除，大文件信任通过，只有100KB以下的小文件才打开并扫描 EOCD 签名。在安全性和速度之间取平衡。"
  },
  {
    id: "B", name: "B — PCL-CE 风格", recommend: true,
    pros: "启动速度最快，几乎零开销；PCL / HMCL 均采用此策略；文件在下载阶段已通过SHA1校验",
    cons: "不检测外部手动放入的损坏JAR；如果下载阶段校验被跳过，可能残留损坏文件",
    desc: "严格参照 PCL-CE 的行为：启动时不扫描 libraries 目录。文件在下载时已通过 SHA1 + 大小双重校验，损坏的 JAR 理论上不会进入 libraries。仅检查实例自身的版本 JAR（如 1.21.5.jar）。"
  },
  {
    id: "C", name: "C — 并行扫描", recommend: false,
    pros: "速度最快的全量扫描方案；多线程充分利用多核CPU和SSD并行IO",
    cons: "CPU 和磁盘 IO 占用较高；机械硬盘上可能反而更慢（随机读取竞争）",
    desc: "使用多线程递归遍历 libraries 目录，每个子目录独立线程处理。SSD + 多核 CPU 环境下比单线程快 3-5 倍。但会短暂占用大量系统资源。"
  },
];

function getVal(key: string, def: string) { return cfg.value[key] ?? def; }

function onPresetChange(e: any) {
  windowPreset.value = e.target.value;
  presetOpen.value = false;
  const p = windowPresets.find(p => p.label === e.target.value);
  if (p && !p.custom) {
    cfg.value.window_width = p.w; cfg.value.window_height = p.h;
    save("window_width", p.w); save("window_height", p.h);
  }
}
function onCustomWH(which: string, e: any) {
  cfg.value[which] = e.target.value;
  save(which, e.target.value);
}
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <h2 class="page-title">
        <span class="bl-zh">设置</span>
        <span class="bl-en">SETTINGS</span>
      </h2>
      <span class="page-line"></span>
    </div>

    <div class="tab-bar">
      <button v-for="tab in tabs" :key="tab.id" :class="['tab-btn', { active: activeTab === tab.id }]" @click="activeTab = tab.id">
        <span class="bl-zh">{{ tab.zh }}</span>
        <span class="bl-en">{{ tab.en }}</span>
      </button>
    </div>

    <Transition name="tab-view" mode="out-in">
      <div class="settings-grid" v-if="activeTab === 'general'" key="general">
        <!-- 窗口分辨率 -->
        <div class="set-item">
          <span class="set-label">
            <span class="bl-zh">窗口分辨率</span>
            <span class="bl-en">Resolution</span>
          </span>
          <div style="flex:1;display:flex;flex-direction:column;gap:6px">
            <div class="custom-select" @click="presetOpen = !presetOpen" @blur="setTimeout(() => presetOpen = false, 150)">
              <span class="set-select-val">{{ windowPreset }}</span>
              <span class="set-select-arrow" :class="{ open: presetOpen }">▾</span>
              <Transition name="drop">
                <div class="set-select-options" v-if="presetOpen">
                  <div
                    class="set-select-opt"
                    :class="{ active: p.label === windowPreset }"
                    v-for="p in windowPresets"
                    :key="p.label"
                    @click.stop="onPresetChange({ target: { value: p.label } })"
                  >{{ p.label }}</div>
                </div>
              </Transition>
            </div>
            <Transition name="drop">
              <div class="set-custom-wh" v-if="windowPreset === '自定义'">
              <input class="set-input" style="width:70px;text-align:center" :value="getVal('window_width','854')" placeholder="854" @change="(e: any) => onCustomWH('window_width', e)" />
              <span style="color:var(--text-dim);margin:0 8px">×</span>
              <input class="set-input" style="width:70px;text-align:center" :value="getVal('window_height','480')" placeholder="480" @change="(e: any) => onCustomWH('window_height', e)" />
            </div>
            </Transition>
          </div>
        </div>

        <div class="set-item" v-for="s in generalSettings" :key="s.key">
          <span class="set-label">
            <span class="bl-zh">{{ s.labelZh }}</span>
            <span class="bl-en">{{ s.labelEn }}</span>
          </span>
          <template v-if="s.toggle">
            <div class="toggle-group">
              <button :class="['toggle-btn', { active: getVal(s.key, s.def) === '是' }]" @click="cfg[s.key]='是'; save(s.key,'是')">是</button>
              <button :class="['toggle-btn', { active: getVal(s.key, s.def) === '否' }]" @click="cfg[s.key]='否'; save(s.key,'否')">否</button>
            </div>
          </template>
          <input v-else class="set-input" :value="getVal(s.key, s.def)" @change="(e: any) => { cfg[s.key] = e.target.value; save(s.key, e.target.value); }" />
        </div>
      </div>

      <div class="settings-grid" v-else-if="activeTab === 'launch'" key="launch">
        <!-- 内存分配 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">游戏内存</span><span class="bl-en">Memory</span></span>
          <div style="flex:1;display:flex;flex-direction:column;gap:6px">
            <div class="custom-select" @click="memOpen = !memOpen" @blur="setTimeout(() => memOpen = false, 150)">
              <span class="set-select-val">{{ memPreset }}</span>
              <span class="set-select-arrow" :class="{ open: memOpen }">▾</span>
              <Transition name="drop">
                <div class="set-select-options" v-if="memOpen">
                  <div class="set-select-opt" :class="{ active: p.label === memPreset }" v-for="p in memPresets" :key="p.label" @click.stop="selectMemPreset(p)">{{ p.label }}</div>
                </div>
              </Transition>
            </div>
            <Transition name="drop">
              <div v-if="memPreset === '自定义'" style="display:flex;align-items:center;gap:8px">
                <input class="set-input" style="width:100px;text-align:center" :value="getVal('max_memory','4096 MB')" placeholder="4096 MB" @change="(e: any) => onCustomMem(e)" />
              </div>
            </Transition>
          </div>
        </div>

        <!-- Java 路径 -->
        <div class="set-item">
          <span class="set-label">
            <span class="bl-zh">Java 路径</span>
            <span class="bl-en">Java Path</span>
          </span>
          <div style="flex:1;display:flex;flex-direction:column;gap:6px">
            <div class="custom-select" @click="javaOpen = !javaOpen" @blur="setTimeout(() => javaOpen = false, 150)">
              <span class="set-select-val" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ javaVal }}</span>
              <span class="set-select-arrow" :class="{ open: javaOpen }">▾</span>
              <Transition name="drop">
                <div class="set-select-options" v-if="javaOpen" style="max-height:200px;overflow-y:auto">
                  <div
                    class="set-select-opt"
                    :class="{ active: entry.path === javaVal }"
                    v-for="entry in javaList"
                    :key="entry.path"
                    @click.stop="selectJava(entry)"
                  >
                    <span style="color:var(--accent);margin-right:8px">{{ entry.version }}</span>
                    <span style="font-size:9px;opacity:.6">{{ entry.path }}</span>
                  </div>
                  <div class="set-select-opt" style="color:var(--accent)" @click.stop="pickCustomJava()">+ 自定义选择...</div>
                </div>
              </Transition>
            </div>
          </div>
        </div>

        <div class="set-item">
          <span class="set-label"><span class="bl-zh">JVM 参数</span><span class="bl-en">JVM Args</span></span>
          <input class="set-input" :value="getVal('jvm_args','-XX:+UseG1GC')" @change="(e: any) => { cfg.jvm_args = e.target.value; save('jvm_args', e.target.value);; }" />
        </div>
      </div>

      <div class="settings-grid" v-else-if="activeTab === 'network'" key="network">
        <!-- 代理类型 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">代理类型</span><span class="bl-en">Proxy Type</span></span>
          <div class="toggle-group">
            <button v-for="t in proxyTypes" :key="t" :class="['toggle-btn', { active: proxyType === t }]" @click="saveProxyType(t)">
              {{ t === 'None' ? '无' : t }}
            </button>
          </div>
        </div>

        <template v-if="proxyType !== 'None'">
          <!-- 代理地址 -->
          <div class="set-item">
            <span class="set-label"><span class="bl-zh">代理地址</span><span class="bl-en">Host</span></span>
            <div style="display:flex;gap:8px;flex:1">
              <input class="set-input" style="flex:1" :value="proxyHost" placeholder="127.0.0.1" @change="(e: any) => { proxyHost = e.target.value; saveProxyField('proxy_host', e.target.value); }" />
              <span style="color:var(--text-dim);display:flex;align-items:center">:</span>
              <input class="set-input" style="width:80px;text-align:center" :value="proxyPort" placeholder="8080" @change="(e: any) => { proxyPort = e.target.value; saveProxyField('proxy_port', e.target.value); }" />
            </div>
          </div>

          <!-- 认证（可选） -->
          <div class="set-item">
            <span class="set-label"><span class="bl-zh">用户名</span><span class="bl-en">Username</span></span>
            <input class="set-input" :value="proxyUser" placeholder="(可选)" @change="(e: any) => { proxyUser = e.target.value; saveProxyField('proxy_user', e.target.value); }" />
          </div>
          <div class="set-item">
            <span class="set-label"><span class="bl-zh">密码</span><span class="bl-en">Password</span></span>
            <input class="set-input" type="password" :value="proxyPass" placeholder="(可选)" @change="(e: any) => { proxyPass = e.target.value; saveProxyField('proxy_pass', e.target.value); }" />
          </div>
        </template>

        <!-- 检测系统代理 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">系统代理</span><span class="bl-en">System Proxy</span></span>
          <div style="display:flex;align-items:center;gap:12px;flex:1">
            <button class="acct-type-btn confirm" :disabled="proxyDetecting" @click="detectProxy" style="font-size:11px">
              {{ proxyDetecting ? '检测中...' : '检测系统代理' }}
            </button>
            <span v-if="proxyStatus" style="font-size:10px;color:var(--accent)">{{ proxyStatus }}</span>
          </div>
        </div>

        <!-- 代理连通性测试 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">连通性测试</span><span class="bl-en">Test</span></span>
          <div style="flex:1;display:flex;align-items:center;gap:16px;flex-wrap:wrap">
            <button class="acct-type-btn confirm" :disabled="proxyTesting" @click="testProxy" style="font-size:11px;white-space:nowrap">
              {{ proxyTesting ? '测试中...' : '测试代理连通性' }}
            </button>
            <div v-if="proxyTestResults.length > 0" style="display:flex;align-items:center;gap:20px;flex-wrap:wrap">
              <div v-for="r in proxyTestResults" :key="r.name" style="display:flex;align-items:center;gap:6px;font-family:var(--font-mono);font-size:11px">
                <span :style="{color: r.ok ? 'var(--accent)' : 'var(--danger)'}">{{ r.ok ? '●' : '✕' }}</span>
                <span style="color:var(--text)">{{ r.name }}</span>
                <span v-if="r.ok" style="color:var(--accent)">{{ r.latency_ms }}ms</span>
                <span v-else class="proxy-test-error">{{ r.error }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="settings-grid" v-else-if="activeTab === 'advanced'" key="advanced">
        <!-- 游戏下载源 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">游戏下载源</span><span class="bl-en">Game Source</span></span>
          <div class="toggle-group">
            <button v-for="s in gameSources" :key="s" :class="['toggle-btn', { active: gameSource === s }]" @click="saveGameSource(s)">{{ s }}</button>
          </div>
        </div>
        <!-- 资源下载源 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">资源下载源</span><span class="bl-en">Mod Source</span></span>
          <div class="toggle-group">
            <button v-for="s in modSources" :key="s" :class="['toggle-btn', { active: modSource === s }]" @click="saveModSource(s)">{{ s }}</button>
          </div>
        </div>

        <!-- 下载线程数 -->
        <div class="set-item">
          <span class="set-label"><span class="bl-zh">下载线程数</span><span class="bl-en">Threads</span></span>
          <input class="set-input" style="width:80px;text-align:center" :value="dlThreads" @change="(e: any) => saveDlThreads(e.target.value)" placeholder="8" />
        </div>

        <!-- JAR 扫描策略 -->
        <div class="set-item">
          <span class="set-label">
            <span class="bl-zh">JAR 扫描策略</span>
            <span class="bl-en">JAR Scan</span>
          </span>
          <div style="flex:1;display:flex;align-items:center;gap:8px">
            <div class="toggle-group" style="flex:1">
              <button v-for="s in scanStrategies" :key="s.id"
                :class="['toggle-btn', { active: scanStrategy === s.id }]"
                @click="saveScanStrategy(s.id)"
                :title="s.desc">{{ s.label }}</button>
            </div>
            <button class="set-help-btn" @click="scanHelpOpen = !scanHelpOpen" title="帮助">?</button>
          </div>
        </div>

        <div class="set-item" v-for="s in advancedSettings" :key="s.key">
          <span class="set-label">
            <span class="bl-zh">{{ s.labelZh }}</span>
            <span class="bl-en">{{ s.labelEn }}</span>
          </span>
          <input class="set-input" :value="getVal(s.key, s.def)" @change="(e: any) => { cfg[s.key] = e.target.value; save(s.key, e.target.value); }" />
        </div>
      </div>

      <div class="settings-grid" v-else key="about" style="gap:16px">
        <!-- 作者卡片 — 全宽 -->
        <div class="about-author-card">
          <a href="https://github.com/Shinkarna" target="_blank" class="about-avatar-link">
            <img class="about-avatar" src="../assets/author-avatar.png" alt="NekoGan" />
          </a>
          <div class="about-author-info">
            <div class="about-name">Teas Launcher</div>
            <div class="about-author">
              <span class="bl-zh">作者: NekoGan</span>
            </div>
            <div class="about-desc">
              <span class="bl-zh">工业科幻仪表盘风格 Minecraft 启动器</span>
              <span class="bl-en">Industrial Sci-Fi Dashboard Minecraft Launcher</span>
            </div>
            <div class="about-desc" style="margin-top:4px;font-size:11px">
              <span class="bl-zh">开发辅助: Claude Code · DeepSeek</span>
              <span class="bl-en">Built with: Claude Code · DeepSeek</span>
            </div>
          </div>
          <a href="https://github.com/TeasOfficial" target="_blank" class="about-org-link" title="TeasOfficial">
            <img class="about-org-avatar" src="../assets/teas-avatar.png" alt="TeasOfficial" />
            <span class="about-org-label">TeasOfficial</span>
          </a>
        </div>

        <!-- 鸣谢卡片 — 全宽 -->
        <div class="about-credit-card">
          <div class="about-credit-title">
            <span class="bl-zh">鸣谢</span>
            <span class="bl-en">ACKNOWLEDGMENTS</span>
          </div>
          <p class="about-credit-desc">
            <span class="bl-zh">以下项目为本启动器提供了宝贵的代码参考与设计灵感</span>
            <span class="bl-en">Code references and design inspiration from the following projects</span>
          </p>
          <div class="about-links">
            <a href="https://github.com/HMCL-dev/HMCL" target="_blank" class="about-link-card">
              <img class="about-link-avatar" src="../assets/hmcl-icon.png" alt="HMCL" />
              <div class="about-link-text">
                <span class="about-link-name">HMCL</span>
                <span class="about-link-sub">Hello Minecraft! Launcher</span>
                <p class="about-link-desc">功能全面的跨平台 Minecraft 启动器，支持模组管理、自动安装、整合包创建与界面自定义</p>
              </div>
            </a>
            <a href="https://github.com/PCL-Community/PCL2" target="_blank" class="about-link-card">
              <img class="about-link-avatar" src="../assets/pclce-icon.ico" alt="PCL CE" />
              <div class="about-link-text">
                <span class="about-link-name">PCL 社区版</span>
                <span class="about-link-sub">Plain Craft Launcher CE</span>
                <p class="about-link-desc">基于 PCL 开源代码二次开发的社区版本，包含主线暂未制作的功能与改进</p>
              </div>
            </a>
            <a href="https://github.com/Meloong-Git/PCL" target="_blank" class="about-link-card">
              <img class="about-link-avatar" src="../assets/pcl-icon.ico" alt="PCL" />
              <div class="about-link-text">
                <span class="about-link-name">PCL</span>
                <span class="about-link-sub">Plain Craft Launcher</span>
                <p class="about-link-desc">龙腾猫跃开发的经典 Minecraft 启动器，下载安装、整合包制作等功能的开创者与行业标杆</p>
              </div>
            </a>
            <a href="https://github.com/PrismLauncher/PrismLauncher" target="_blank" class="about-link-card">
              <span class="about-link-avatar" style="position:relative;display:inline-block;width:48px;height:48px;flex-shrink:0;background:transparent;border:none">
                <img src="../assets/prism-rainbow.svg" alt="" style="position:absolute;width:48px;height:48px" />
                <img src="../assets/prism-block.svg" alt="PrismLauncher" style="position:absolute;width:48px;height:48px" />
              </span>
              <div class="about-link-text">
                <span class="about-link-name">PrismLauncher</span>
                <span class="about-link-sub">MultiMC 精神续作</span>
                <p class="about-link-desc">国外最受欢迎的第三方启动器，跨平台支持，强大的实例管理与模组包生态系统</p>
              </div>
            </a>
          </div>
        </div>

        <!-- 技术栈卡片 — 全宽 -->
        <div class="about-tech-card">
          <div class="about-credit-title">
            <span class="bl-zh">技术栈</span>
            <span class="bl-en">TECH STACK</span>
          </div>
          <p class="about-credit-desc">
            <span class="bl-zh">本启动器采用以下技术构建，兼顾性能与开发体验</span>
            <span class="bl-en">Built with modern technologies for performance and developer experience</span>
          </p>
          <div class="about-tech-grid">
            <div class="about-tech-item">
              <span class="about-tech-label">Rust + Tauri v2</span>
              <span class="about-tech-detail">系统级原生桌面壳，<br/>轻量二进制（~5MB），<br/>安全内存管理，无 GC 停顿</span>
            </div>
            <div class="about-tech-item">
              <span class="about-tech-label">Vue 3 + TypeScript</span>
              <span class="about-tech-detail">Composition API 响应式架构，<br/>Vite 秒级热更新，<br/>完整类型推导与检查</span>
            </div>
            <div class="about-tech-item">
              <span class="about-tech-label">工业科幻设计语言</span>
              <span class="about-tech-detail">暗色仪表盘主题，<br/>双语 UI 系统 (中/英)，<br/>Orbitron + JetBrains Mono 字体</span>
            </div>
            <div class="about-tech-item">
              <span class="about-tech-label">多源智能下载</span>
              <span class="about-tech-detail">Modrinth API 搜索与安装，<br/>多线程并行 + 信号量限速，<br/>BMCLAPI / MCIMirror 镜像自动回退</span>
            </div>
            <div class="about-tech-item">
              <span class="about-tech-label">加载器自动安装</span>
              <span class="about-tech-detail">HMCL 风格 Forge / Fabric / NeoForge 安装，<br/>原版文件自动同步，<br/>完整性校验与损坏修复</span>
            </div>
            <div class="about-tech-item">
              <span class="about-tech-label">系统代理集成</span>
              <span class="about-tech-detail">Windows 注册表自动检测代理，<br/>HTTP / SOCKS5 支持，<br/>认证代理兼容</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- JAR 扫描策略帮助弹窗 -->
    <Transition name="modal">
      <div class="modal-overlay" v-if="scanHelpOpen" @click.self="scanHelpOpen = false">
        <div class="modal-panel" style="width:520px">
          <div class="modal-head">
            <span class="modal-title">JAR 扫描策略说明</span>
            <button class="modal-close" @click="scanHelpOpen = false">✕</button>
          </div>
          <div class="modal-body" style="max-height:55vh;overflow-y:auto;gap:12px;padding:14px 18px">
            <div v-for="s in scanHelpSections" :key="s.id" style="border-bottom:1px solid var(--border);padding-bottom:10px">
              <div style="display:flex;align-items:center;gap:8px;margin-bottom:3px">
                <span style="font-weight:700;font-size:13px;color:var(--secondary)">{{ s.name }}</span>
                <span v-if="s.recommend" style="font-size:10px;background:var(--accent-dim);color:var(--accent);padding:1px 6px;border-radius:3px">推荐</span>
              </div>
              <p class="modal-desc-sub">{{ s.desc }}</p>
              <div style="display:flex;gap:12px;font-size:11px;margin-top:4px">
                <span style="color:#4CAF50;flex:1">✓ {{ s.pros }}</span>
                <span style="color:#FF5252;flex:1">✗ {{ s.cons }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </section>
</template>
