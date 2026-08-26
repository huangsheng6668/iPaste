<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, ChevronDown, ChevronRight, Copy, X } from "lucide-vue-next";
import { t, type I18nKey } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";
import { errorMessage } from "../lib/appError";
import { fingerprintOf, sendTargets as buildSendTargets, statusKey, type DeviceStatusKey } from "../lib/deviceDisplay";
import { INVALID_TICKET, useDeviceSync } from "../composables/useDeviceSync";
import type { AutoSyncMode } from "../types/generated/AutoSyncMode";
import type { DeviceInfo } from "../types/generated/DeviceInfo";

const sync = useDeviceSync();
const { devices, inviteTicket, inviteExpiresAt, joinError, pairRequest, pairError, loadError } = sync;

// —— 窗口壳 ——

async function closeWindow() {
  await getCurrentWindow().close();
}

// —— 设备行操作 ——

const actionError = ref<string | null>(null);

async function runDeviceAction(action: () => Promise<unknown>) {
  actionError.value = null;
  try {
    await action();
    return true;
  } catch (unknownError) {
    actionError.value = errorMessage(unknownError);
    return false;
  }
}

function isRevoked(entry: DeviceInfo) {
  return Boolean(entry.device.revokedAt);
}

// —— 手动发送（设备列表 section 底部快捷按钮）——

// 复用 sendTargets 纯函数判定「有可发送目标」：除 __all__ 外还有在线未撤销设备。
const hasOnlineDevice = computed(() => buildSendTargets(devices.value).length > 1);

function sendCurrent() {
  void runDeviceAction(() => ipasteApi.deviceSendClip(null, { kind: "current" }));
}

// DeviceOnline 的 serde 值（connected）与 i18n key（online）不同名，显式映射保住字面量类型。
const STATUS_LABEL_KEYS: Record<DeviceStatusKey, I18nKey> = {
  connected: "deviceSync.status.online",
  connecting: "deviceSync.status.connecting",
  offline: "deviceSync.status.offline",
};

function statusText(entry: DeviceInfo) {
  if (isRevoked(entry)) return t("deviceSync.status.revoked");
  return t(STATUS_LABEL_KEYS[statusKey(entry.online)]);
}

async function onAutoSyncChange(entry: DeviceInfo, event: Event) {
  const select = event.target as HTMLSelectElement;
  const mode = select.value as AutoSyncMode;
  const succeeded = await runDeviceAction(() => sync.setAutoSync(entry.device.nodeId, mode));
  if (!succeeded) {
    // 设置失败：select 显示回滚为后端权威值，避免停留在未生效的选择（B5）。
    select.value = entry.device.autoSyncMode;
  }
}

function onDisconnect(entry: DeviceInfo) {
  void runDeviceAction(() => sync.disconnect(entry.device.nodeId));
}

// 撤销走两击确认（对齐 useClearHistory 的状态确认模式）：第一次点击进入
// 确认态并 3 秒后自动回退，第二次点击执行。
const confirmingRevokeId = ref<string | null>(null);
let revokeResetTimer: ReturnType<typeof setTimeout> | null = null;

function resetRevokeConfirm() {
  confirmingRevokeId.value = null;
  if (revokeResetTimer) {
    clearTimeout(revokeResetTimer);
    revokeResetTimer = null;
  }
}

function onRevoke(entry: DeviceInfo) {
  const nodeId = entry.device.nodeId;
  if (confirmingRevokeId.value !== nodeId) {
    confirmingRevokeId.value = nodeId;
    if (revokeResetTimer) clearTimeout(revokeResetTimer);
    revokeResetTimer = setTimeout(resetRevokeConfirm, 3000);
    return;
  }
  resetRevokeConfirm();
  void runDeviceAction(() => sync.revoke(nodeId));
}

function onRemove(entry: DeviceInfo) {
  void runDeviceAction(() => sync.remove(entry.device.nodeId));
}

// —— 邀请（host 侧）——

const creatingInvite = ref(false);
const inviteError = ref<string | null>(null);
const ticketCopied = ref<string | null>(null);
let copyResetTimer: ReturnType<typeof setTimeout> | null = null;

// 邀请有效期间每秒刷新「现在」，驱动 mm:ss 倒计时；取消/作废/卸载时清理。
const nowMs = ref(Date.now());
let countdownTimer: ReturnType<typeof setInterval> | null = null;

const inviteCountdown = computed(() => {
  if (!inviteExpiresAt.value) return "00:00";
  const remaining = Math.max(0, Math.floor((inviteExpiresAt.value - nowMs.value) / 1000));
  const minutes = String(Math.floor(remaining / 60)).padStart(2, "0");
  const seconds = String(remaining % 60).padStart(2, "0");
  return `${minutes}:${seconds}`;
});

// 票据仍在展示但倒计时已归零（后端已作废）：UI 停止广告死票据（B3）——
// 复制禁用、票据置灰划线、倒计时文案换「已过期」；再次生成邀请会正常覆盖。
const inviteExpired = computed(() => {
  if (!inviteTicket.value || !inviteExpiresAt.value) return false;
  return inviteExpiresAt.value - nowMs.value <= 0;
});

watch(
  inviteTicket,
  (ticket) => {
    if (ticket && countdownTimer === null) {
      nowMs.value = Date.now();
      countdownTimer = setInterval(() => {
        nowMs.value = Date.now();
      }, 1000);
    } else if (!ticket && countdownTimer !== null) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
  },
  { immediate: true },
);

async function onCreateInvite() {
  inviteError.value = null;
  creatingInvite.value = true;
  try {
    await sync.createInvite();
  } catch (unknownError) {
    inviteError.value = errorMessage(unknownError);
  } finally {
    creatingInvite.value = false;
  }
}

async function onCancelInvite() {
  inviteError.value = null;
  try {
    await sync.cancelInvite();
  } catch (unknownError) {
    inviteError.value = errorMessage(unknownError);
  }
}

async function copyTicket() {
  const ticket = inviteTicket.value;
  if (!ticket) return;
  try {
    await navigator.clipboard.writeText(ticket);
    ticketCopied.value = "ok";
  } catch {
    ticketCopied.value = "error";
  }
  if (copyResetTimer) clearTimeout(copyResetTimer);
  copyResetTimer = setTimeout(() => {
    ticketCopied.value = null;
  }, 2000);
}

// —— 加入（guest 侧）——

const joinInput = ref("");

const joinErrorText = computed(() => {
  if (!joinError.value) return null;
  if (joinError.value === INVALID_TICKET) return t("deviceSync.join.invalidTicket");
  // 后端 reason 原样透传展示。
  return joinError.value;
});

async function onJoin() {
  await sync.join(joinInput.value);
}

// —— 传输设置（自定义中继 + 自动推送全局开关）——

const relayOpen = ref(false);
const relayInput = ref("");
const relaySaved = ref(false);
const relayError = ref<string | null>(null);
const savingRelay = ref(false);

// 自动推送全局开关（store 缺省 master=true / notify=false）。
const autoPushMaster = ref(true);
const autoPushNotify = ref(false);
const autoPushError = ref<string | null>(null);

onMounted(async () => {
  if (!isTauri) return;
  try {
    const settings = await ipasteApi.syncTransportSettingsGet();
    relayInput.value = settings.relayUrl ?? "";
  } catch {
    // 读取失败保持空输入（= n0 默认中继），不打断面板。
  }
  try {
    const autoPush = await ipasteApi.syncAutoPushSettingsGet();
    autoPushMaster.value = autoPush.master;
    autoPushNotify.value = autoPush.notify;
  } catch {
    // 读取失败保持缺省开关，不打断面板。
  }
});

async function onSaveRelay() {
  relayError.value = null;
  relaySaved.value = false;
  savingRelay.value = true;
  try {
    const normalized = relayInput.value.trim();
    const result = await ipasteApi.syncTransportSettingsSet(normalized ? normalized : null);
    relayInput.value = result.relayUrl ?? "";
    relaySaved.value = true;
  } catch (unknownError) {
    relayError.value = errorMessage(unknownError);
  } finally {
    savingRelay.value = false;
  }
}

// 开关即改即存：以落库返回值回填（round-trip）；失败时回滚到改前状态并提示。
async function saveAutoPush() {
  autoPushError.value = null;
  const previous = { master: autoPushMaster.value, notify: autoPushNotify.value };
  try {
    const saved = await ipasteApi.syncAutoPushSettingsSet(
      autoPushMaster.value,
      autoPushNotify.value,
    );
    autoPushMaster.value = saved.master;
    autoPushNotify.value = saved.notify;
  } catch (unknownError) {
    autoPushMaster.value = previous.master;
    autoPushNotify.value = previous.notify;
    autoPushError.value = errorMessage(unknownError);
  }
}

onUnmounted(() => {
  if (countdownTimer) {
    clearInterval(countdownTimer);
    countdownTimer = null;
  }
  if (copyResetTimer) clearTimeout(copyResetTimer);
  if (revokeResetTimer) clearTimeout(revokeResetTimer);
});
</script>

<template>
  <div class="lan-sync-panel">
    <header
      class="lan-header"
      data-tauri-drag-region
    >
      <span>{{ t("deviceSync.title") }}</span>
      <button
        type="button"
        class="lan-close"
        :aria-label="t('topBar.closePanel')"
        @click="closeWindow"
      >
        <X :size="16" />
      </button>
    </header>

    <!-- 设备列表 -->
    <section class="lan-section">
      <h2 class="lan-section-title">{{ t("deviceSync.list.title") }}</h2>
      <p
        v-if="loadError"
        class="lan-error"
        :title="loadError"
      >
        {{ t("deviceSync.list.loadFailed") }}
      </p>
      <p
        v-if="devices.length === 0"
        class="lan-empty"
      >
        {{ t("deviceSync.list.empty") }}
      </p>
      <ul
        v-else
        class="lan-devices"
      >
        <li
          v-for="entry in devices"
          :key="entry.device.nodeId"
          class="lan-device"
          :class="{ 'lan-device-revoked': isRevoked(entry) }"
        >
          <span
            class="lan-dot"
            :class="isRevoked(entry) ? 'lan-dot-revoked' : `lan-dot-${statusKey(entry.online)}`"
            :aria-label="statusText(entry)"
          />
          <div class="lan-device-main">
            <span class="lan-device-name">{{ entry.device.deviceName }}</span>
            <span class="lan-device-fingerprint">{{ fingerprintOf(entry.device.nodeId) }}</span>
          </div>
          <span class="lan-device-status">{{ statusText(entry) }}</span>
          <select
            class="lan-select"
            :disabled="isRevoked(entry)"
            :value="entry.device.autoSyncMode"
            :aria-label="t('deviceSync.autoSync.label')"
            @change="onAutoSyncChange(entry, $event)"
          >
            <option value="text_only">
              {{ t("deviceSync.autoSync.textOnly") }}
            </option>
            <option value="all">
              {{ t("deviceSync.autoSync.all") }}
            </option>
            <option value="off">
              {{ t("deviceSync.autoSync.off") }}
            </option>
          </select>
          <div class="lan-device-actions">
            <button
              v-if="!isRevoked(entry) && entry.online !== 'offline'"
              type="button"
              class="lan-action"
              @click="onDisconnect(entry)"
            >
              {{ t("deviceSync.action.disconnect") }}
            </button>
            <button
              v-if="!isRevoked(entry)"
              type="button"
              class="lan-action"
              :class="{ 'lan-action-danger': confirmingRevokeId === entry.device.nodeId }"
              @click="onRevoke(entry)"
            >
              {{
                confirmingRevokeId === entry.device.nodeId
                  ? t("deviceSync.action.revokeConfirm")
                  : t("deviceSync.action.revoke")
              }}
            </button>
            <button
              v-if="isRevoked(entry)"
              type="button"
              class="lan-action lan-action-danger"
              @click="onRemove(entry)"
            >
              {{ t("deviceSync.action.remove") }}
            </button>
          </div>
        </li>
      </ul>
      <p
        v-if="actionError"
        class="lan-error"
      >
        {{ actionError }}
      </p>
      <button
        type="button"
        class="lan-button"
        :disabled="!hasOnlineDevice"
        @click="sendCurrent"
      >
        {{ t("deviceSync.sendTo.current") }}
      </button>
    </section>

    <!-- 邀请设备（host 侧） -->
    <section class="lan-section">
      <h2 class="lan-section-title">{{ t("deviceSync.invite.title") }}</h2>
      <template v-if="inviteTicket">
        <div class="lan-ticket-row">
          <input
            class="lan-input"
            :class="{ 'lan-ticket-expired': inviteExpired }"
            readonly
            :value="inviteTicket"
            aria-readonly="true"
          >
          <button
            type="button"
            class="lan-button"
            :disabled="inviteExpired"
            @click="copyTicket"
          >
            <Check
              v-if="ticketCopied === 'ok'"
              :size="14"
            />
            <Copy
              v-else
              :size="14"
            />
            {{ ticketCopied === "ok" ? t("deviceSync.invite.copied") : t("deviceSync.invite.copy") }}
          </button>
        </div>
        <p
          v-if="ticketCopied === 'error'"
          class="lan-error"
        >
          {{ t("deviceSync.invite.copyFailed") }}
        </p>
        <div class="lan-ticket-row">
          <span class="lan-hint">
            {{
              inviteExpired
                ? t("deviceSync.invite.expired")
                : t("deviceSync.invite.expiresIn", { time: inviteCountdown })
            }}
          </span>
          <button
            type="button"
            class="lan-button"
            @click="onCancelInvite"
          >
            {{ t("deviceSync.invite.cancel") }}
          </button>
        </div>
      </template>
      <button
        v-else
        type="button"
        class="lan-button lan-button-primary"
        :disabled="creatingInvite"
        @click="onCreateInvite"
      >
        {{ t("deviceSync.invite.button") }}
      </button>
      <p
        v-if="inviteError"
        class="lan-error"
      >
        {{ inviteError }}
      </p>
    </section>

    <!-- 加入设备（guest 侧） -->
    <section class="lan-section">
      <h2 class="lan-section-title">{{ t("deviceSync.join.title") }}</h2>
      <label
        class="lan-label"
        for="lan-join-input"
      >{{ t("deviceSync.join.label") }}</label>
      <div class="lan-ticket-row">
        <input
          id="lan-join-input"
          v-model="joinInput"
          class="lan-input"
          type="text"
          :placeholder="t('deviceSync.join.label')"
          @keydown.enter="onJoin"
        >
        <button
          type="button"
          class="lan-button lan-button-primary"
          :disabled="!joinInput.trim()"
          @click="onJoin"
        >
          {{ t("deviceSync.join.button") }}
        </button>
      </div>
      <p
        v-if="joinErrorText"
        class="lan-error"
      >
        {{ joinErrorText }}
      </p>
    </section>

    <!-- 传输设置（折叠区） -->
    <section class="lan-section lan-section-footer">
      <button
        type="button"
        class="lan-collapsible-header"
        :aria-expanded="relayOpen"
        @click="relayOpen = !relayOpen"
      >
        <ChevronDown
          v-if="relayOpen"
          :size="14"
        />
        <ChevronRight
          v-else
          :size="14"
        />
        {{ t("deviceSync.relay.title") }}
      </button>
      <div
        v-if="relayOpen"
        class="lan-relay-body"
      >
        <label
          class="lan-label"
          for="lan-relay-input"
        >{{ t("deviceSync.relay.label") }}</label>
        <div class="lan-ticket-row">
          <input
            id="lan-relay-input"
            v-model="relayInput"
            class="lan-input"
            type="text"
            :placeholder="t('deviceSync.relay.placeholder')"
          >
          <button
            type="button"
            class="lan-button"
            :disabled="savingRelay"
            @click="onSaveRelay"
          >
            {{ t("common.save") }}
          </button>
        </div>
        <p
          v-if="relaySaved"
          class="lan-hint"
        >
          {{ t("deviceSync.relay.restartHint") }}
        </p>
        <p
          v-if="relayError"
          class="lan-error"
        >
          {{ relayError }}
        </p>
        <label class="lan-setting-row">
          <input
            type="checkbox"
            v-model="autoPushMaster"
            @change="saveAutoPush"
          >
          <span>{{ t("deviceSync.autoPush.master") }}</span>
        </label>
        <label class="lan-setting-row">
          <input
            type="checkbox"
            v-model="autoPushNotify"
            @change="saveAutoPush"
          >
          <span>{{ t("deviceSync.autoPush.notify") }}</span>
        </label>
        <p
          v-if="autoPushError"
          class="lan-error"
        >
          {{ autoPushError }}
        </p>
      </div>
    </section>

    <!-- 配对确认弹窗 -->
    <div
      v-if="pairRequest"
      class="lan-pair-overlay"
      role="dialog"
      aria-modal="true"
      :aria-label="t('deviceSync.pair.title')"
    >
      <div class="lan-pair-dialog">
        <h3 class="lan-pair-title">{{ t("deviceSync.pair.title") }}</h3>
        <p class="lan-pair-device">{{ pairRequest.deviceName }}</p>
        <p class="lan-pair-fingerprint">
          {{ t("deviceSync.pair.fingerprint") }}: {{ pairRequest.fingerprint }}
        </p>
        <div class="lan-pair-actions">
          <button
            type="button"
            class="lan-button lan-button-primary"
            @click="sync.respondPair(true)"
          >
            {{ t("deviceSync.pair.accept") }}
          </button>
          <button
            type="button"
            class="lan-button"
            @click="sync.respondPair(false)"
          >
            {{ t("deviceSync.pair.decline") }}
          </button>
        </div>
      </div>
    </div>
    <!-- 配对应答失败的残留错误（B2）：弹窗已关，以非模态浮层就地展示，
         新配对请求到达时清除。 -->
    <p
      v-else-if="pairError"
      class="lan-error lan-pair-error"
      role="alert"
    >
      {{ pairError }}
    </p>
  </div>
</template>

<style scoped>
.lan-sync-panel {
  position: relative;
  width: 100vw;
  height: 100vh;
  overflow: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 14px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--window-radius);
  clip-path: inset(0 round var(--window-radius));
  background: var(--bg-app);
  color: var(--text-1);
}
html.dark .lan-sync-panel { border-color: var(--border-hairline); }
.lan-header { display: flex; align-items: center; gap: 8px; font-weight: 600; }
.lan-close {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--r-md);
  border: none;
  background: transparent;
  cursor: pointer;
  color: var(--text-2);
  transition: background-color 140ms ease, color 140ms ease;
}
.lan-close:hover { background: var(--surface-hover); color: var(--text-1); }

.lan-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-lg);
  background: var(--surface-2);
}
.lan-section-footer { margin-top: auto; }
.lan-section-title {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.lan-empty {
  margin: 0;
  color: var(--text-2);
}

/* 设备行 */
.lan-devices {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.lan-device {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 8px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-md);
}
.lan-device-revoked .lan-device-name {
  text-decoration: line-through;
  color: var(--text-2);
}
.lan-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--r-pill);
  flex-shrink: 0;
  background: var(--text-3);
}
.lan-dot-connected { background: var(--success); }
.lan-dot-connecting { background: #eab308; }
.lan-dot-offline { background: var(--text-3); }
.lan-dot-revoked { background: var(--danger); }
.lan-device-main {
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-width: 0;
  flex: 1 1 140px;
}
.lan-device-name {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.lan-device-fingerprint {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-2);
}
.lan-device-status {
  font-size: 12px;
  color: var(--text-2);
  white-space: nowrap;
}
.lan-device-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-left: auto;
}

/* 控件 */
.lan-select,
.lan-input {
  padding: 4px 8px;
  font-size: 13px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-md);
  background: var(--bg-app);
  color: var(--text-1);
}
.lan-input { flex: 1 1 160px; min-width: 0; }
.lan-input[readonly] {
  color: var(--text-2);
  font-family: var(--font-mono);
  font-size: 12px;
}
.lan-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 13px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-md);
  background: var(--surface-hover, transparent);
  color: var(--text-1);
  cursor: pointer;
  transition: background-color 140ms ease;
  white-space: nowrap;
}
.lan-button:hover:not(:disabled) { background: var(--surface-hover); }
.lan-button:disabled { opacity: 0.6; cursor: not-allowed; }
.lan-button-primary {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--on-accent);
}
.lan-button-primary:hover:not(:disabled) { filter: brightness(1.08); background: var(--accent); }
.lan-action {
  padding: 2px 8px;
  font-size: 12px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  white-space: nowrap;
}
.lan-action:hover { background: var(--surface-hover); color: var(--text-1); }
.lan-action-danger { color: var(--danger); border-color: var(--border-accent); }
.lan-action-danger:hover { color: var(--danger-strong); }

.lan-ticket-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
/* 已过期票据（B3）：置灰划线，不再广告死票据。 */
.lan-ticket-expired {
  color: var(--text-2);
  text-decoration: line-through;
}
.lan-label {
  font-size: 12px;
  color: var(--text-2);
}
.lan-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-2);
}
.lan-error {
  margin: 0;
  font-size: 12px;
  color: var(--danger);
}

/* 传输设置折叠区 */
.lan-collapsible-header {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
  border: none;
  background: transparent;
  cursor: pointer;
}
.lan-collapsible-header:hover { color: var(--text-1); }
.lan-relay-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.lan-setting-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-1);
  cursor: pointer;
}
.lan-setting-row input[type="checkbox"] {
  margin: 0;
  accent-color: var(--accent);
  cursor: pointer;
}

/* 配对确认弹窗 */
.lan-pair-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  background: rgba(0, 0, 0, 0.45);
  border-radius: var(--window-radius);
  z-index: 10;
}
.lan-pair-dialog {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px;
  min-width: 240px;
  max-width: 320px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-xl);
  background: var(--bg-app);
  color: var(--text-1);
}
.lan-pair-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
}
.lan-pair-device { margin: 0; font-weight: 500; }
.lan-pair-fingerprint {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-2);
}
.lan-pair-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
/* 配对应答失败的浮层错误（B2）：非模态、不拦截交互，新请求到达即清除。 */
.lan-pair-error {
  position: absolute;
  left: 50%;
  bottom: 24px;
  transform: translateX(-50%);
  max-width: calc(100% - 32px);
  padding: 8px 12px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-md);
  background: var(--bg-app);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  z-index: 11;
}
</style>
