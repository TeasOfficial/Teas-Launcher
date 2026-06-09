<script setup lang="ts">
import { ref, inject } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Account } from "../App.vue";

const accounts = inject<Account[]>("accounts")! as unknown as Ref<Account[]>;
const setActiveAccount = inject<(i: number) => void>("setActiveAccount")!;

async function saveAccounts() {
  try { await invoke("config_write", { scope: "user", key: "accounts", value: accounts.value }); } catch { /* */ }
}

const showDialog = ref(false);
const dialogMode = ref<"microsoft" | "offline" | "authlib" | null>(null);
const offlineName = ref("");
const authlibUrl = ref("");
const authlibEmail = ref("");
const authlibPass = ref("");

function openDialog(mode: "microsoft" | "offline" | "authlib") {
  dialogMode.value = mode;
  offlineName.value = "";
  authlibUrl.value = "";
  authlibEmail.value = "";
  authlibPass.value = "";
  showDialog.value = true;
}

function closeDialog() {
  showDialog.value = false;
  dialogMode.value = null;
}

async function createAccount() {
  if (dialogMode.value === "offline" && offlineName.value.trim()) {
    accounts.value.push({
      icon: "◉", nameZh: "离线账户", nameEn: "Offline Account",
      statusZh: offlineName.value.trim().toUpperCase(),
      statusEn: offlineName.value.trim().toUpperCase(),
      active: accounts.value.length === 0, type: "offline",
    } as Account);
    await saveAccounts();
    closeDialog();
  } else if (dialogMode.value === "authlib" && authlibUrl.value.trim()) {
    accounts.value.push({
      icon: "⬡", nameZh: "外置验证", nameEn: "Authlib",
      statusZh: "未登录", statusEn: "DISCONNECTED",
      active: accounts.value.length === 0, type: "authlib",
    } as Account);
    await saveAccounts();
    closeDialog();
  } else if (dialogMode.value === "microsoft") {
    closeDialog();
  }
}

function setActive(index: number) {
  setActiveAccount(index);
}

async function removeAccount(index: number) {
  accounts.value.splice(index, 1);
  // 如果删除的是当前激活的，激活第一个
  if (!accounts.value.some(a => a.active) && accounts.value.length > 0) {
    accounts.value[0].active = true;
    accounts.value = [...accounts.value];
  }
  await saveAccounts();
}
</script>

<template>
  <section class="page active">
    <div class="page-header">
      <h2 class="page-title">
        <span class="bl-zh">账户管理</span>
        <span class="bl-en">ACCOUNT MANAGEMENT</span>
      </h2>
      <span class="page-line"></span>
      <button class="acct-new-btn" @click="showDialog = true">
        <span class="bl-zh">+ 新建账户</span>
        <span class="bl-en">+ NEW ACCOUNT</span>
      </button>
    </div>

    <div v-if="accounts.length === 0 && !showDialog" class="acct-empty">
      <p class="acct-empty-zh">暂无账户，请点击右上角「+ 新建账户」添加</p>
      <p class="acct-empty-en">No accounts. Click [+ NEW ACCOUNT] to add one.</p>
    </div>

    <TransitionGroup name="acct-list" tag="div" class="account-cards" v-else>
      <div
        class="acct-card"
        :class="{ 'acct-active': a.active }"
        v-for="(a, i) in accounts"
        :key="a.statusEn + i + a.type"
        @click="setActive(i)"
      >
        <button class="acct-remove-btn" @click.stop="removeAccount(i)" title="移除账户">✕</button>
        <div class="acct-card-inner">
          <span class="acct-icon">{{ a.icon }}</span>
          <span class="acct-name">
            <span class="bl-zh">{{ a.nameZh }}</span>
            <span class="bl-en">{{ a.nameEn }}</span>
          </span>
          <span class="acct-status" :class="{ active: a.active, offline: !a.active }">
            <span class="bl-zh">{{ a.statusZh }}</span>
            <span class="bl-en">{{ a.statusEn }}</span>
          </span>
        </div>
      </div>
    </TransitionGroup>

    <!-- 弹窗部分不变 -->
    <Transition name="modal">
      <div v-if="showDialog" class="modal-overlay" @click.self="closeDialog">
        <div class="modal-panel">
          <div class="modal-head">
            <span class="modal-title">
              <span class="bl-zh">新建账户</span>
              <span class="bl-en">NEW ACCOUNT</span>
            </span>
            <button class="modal-close" @click="closeDialog">✕</button>
          </div>

          <div v-if="!dialogMode" class="modal-body modal-types">
            <button class="acct-type-btn" @click="openDialog('microsoft')">
              <span class="atb-icon">⊞</span>
              <span class="atb-zh">微软正版</span>
              <span class="atb-en">MICROSOFT</span>
            </button>
            <button class="acct-type-btn" @click="openDialog('offline')">
              <span class="atb-icon">◉</span>
              <span class="atb-zh">离线账户</span>
              <span class="atb-en">OFFLINE</span>
            </button>
            <button class="acct-type-btn" @click="openDialog('authlib')">
              <span class="atb-icon">⬡</span>
              <span class="atb-zh">外置验证</span>
              <span class="atb-en">AUTHLIB</span>
            </button>
          </div>

          <div v-if="dialogMode === 'offline'" class="modal-body">
            <div class="form-field">
              <span class="form-label">玩家名 / Username</span>
              <input class="form-input" v-model="offlineName" placeholder="Steve" maxlength="16" @keyup.enter="createAccount" />
            </div>
            <div class="modal-actions">
              <button class="acct-type-btn cancel" @click="closeDialog">取消</button>
              <button class="acct-type-btn confirm" @click="createAccount">创建</button>
            </div>
          </div>

          <div v-if="dialogMode === 'authlib'" class="modal-body">
            <div class="form-field">
              <span class="form-label">验证服务器 / Auth Server URL</span>
              <input class="form-input" v-model="authlibUrl" placeholder="https://auth.example.com/api/authlib" @keyup.enter="createAccount" />
            </div>
            <div class="form-field">
              <span class="form-label">邮箱 / Email</span>
              <input class="form-input" v-model="authlibEmail" placeholder="player@example.com" />
            </div>
            <div class="form-field">
              <span class="form-label">密码 / Password</span>
              <input class="form-input" type="password" v-model="authlibPass" placeholder="••••••••" />
            </div>
            <div class="modal-actions">
              <button class="acct-type-btn cancel" @click="closeDialog">取消</button>
              <button class="acct-type-btn confirm" @click="createAccount">创建</button>
            </div>
          </div>

          <div v-if="dialogMode === 'microsoft'" class="modal-body">
            <p class="modal-desc">即将打开浏览器进行 Microsoft 账户登录验证。</p>
            <p class="modal-desc-sub">A browser window will open for Microsoft authentication.</p>
            <div class="modal-actions">
              <button class="acct-type-btn cancel" @click="closeDialog">取消</button>
              <button class="acct-type-btn confirm" @click="createAccount">打开浏览器</button>
            </div>
          </div>

        </div>
      </div>
    </Transition>
  </section>
</template>
