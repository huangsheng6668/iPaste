<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { windowHandle } from "../platform/window";
import { X } from "lucide-vue-next";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { errorMessage } from "../lib/appError";
import { useDeviceSync } from "../composables/useDeviceSync";
import { joinErrorKey, SEND_TARGET_ALL } from "../lib/deviceDisplay";
import { showError, useUiStore } from "../stores/uiStore";
import { useTwoStepConfirm } from "../composables/useTwoStepConfirm";
import { useInviteCountdown } from "../composables/useInviteCountdown";
import { useSyncTransportSettings } from "../composables/useSyncTransportSettings";
import DeviceListSection from "./lanSync/DeviceListSection.vue";
import InviteSection from "./lanSync/InviteSection.vue";
import JoinSection from "./lanSync/JoinSection.vue";
import RelaySettingsSection from "./lanSync/RelaySettingsSection.vue";
import PairConfirmDialog from "./lanSync/PairConfirmDialog.vue";
import type { AutoSyncMode } from "../types/generated/AutoSyncMode";
import type { Category } from "../types/generated/Category";
import type { DeviceInfo } from "../types/generated/DeviceInfo";

const sync = useDeviceSync();
const {
  devices, inviteTicket, inviteExpiresAt, invitePin, joinError, pairRequest, pairError, loadError,
  nearby, selectedEndpointId, pinInput,
} = sync;

// —— 窗口壳 ——

async function closeWindow() {
  await windowHandle().close();
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

// select 失败回滚（B5）需要改写 DeviceListSection 内的 select 显示，
// 因此子组件把 select 元素随 autoSyncChange 事件上抛，此处恢复后端权威值。
async function onAutoSyncChange(entry: DeviceInfo, mode: AutoSyncMode, select: HTMLSelectElement) {
  const succeeded = await runDeviceAction(() => sync.setAutoSync(entry.device.nodeId, mode));
  if (!succeeded) {
    // 设置失败：select 显示回滚为后端权威值，避免停留在未生效的选择（B5）。
    select.value = entry.device.autoSyncMode;
  }
}

function onDisconnect(nodeId: string) {
  void runDeviceAction(() => sync.disconnect(nodeId));
}

// 撤销走两击确认（对齐 useClearHistory 的状态确认模式）：第一次点击进入
// 确认态并 3 秒后自动回退，第二次点击执行。
const revokeConfirm = useTwoStepConfirm();
const { confirmingKey } = revokeConfirm;

function onRevoke(nodeId: string) {
  if (!revokeConfirm.isConfirming(nodeId)) {
    revokeConfirm.request(nodeId);
    return;
  }
  revokeConfirm.cancel();
  void runDeviceAction(() => sync.revoke(nodeId));
}

function onRemove(nodeId: string) {
  void runDeviceAction(() => sync.remove(nodeId));
}

// —— 手动发送（设备列表 section 底部快捷按钮）——

// —— 发送内容（当前剪贴板 / 某个分类）——
// 分类列表由本窗口自行加载：Tauri 多窗口各自独立，主面板的 Pinia store
// 不跨窗口共享。
const categories = ref<Category[]>([]);

onMounted(async () => {
  try {
    categories.value = await ipasteApi.listCategories();
  } catch (unknownError) {
    // 分类拉取失败只降级为「只能发当前剪贴板」；不阻断设备管理其余功能。
    console.warn("[ipaste] list_categories failed:", unknownError);
  }
});

async function onSend(categoryId: string | null, targetId: string) {
  const target = targetId === SEND_TARGET_ALL ? null : targetId;
  try {
    if (!categoryId) {
      await ipasteApi.deviceSendClip(target, { kind: "current" });
      useUiStore().pushToast(t("deviceSync.send.currentSent"));
      return;
    }
    const result = await ipasteApi.deviceSendCategory(target, categoryId);
    const ui = useUiStore();
    if (result.failed > 0) {
      ui.pushToast(t("category.sendPartial", { sent: result.sent, failed: result.failed }));
      return;
    }
    ui.pushToast(t("category.sent", { name: result.categoryName, sent: result.sent }));
  } catch (unknownError) {
    showError(unknownError);
  }
}

// —— 邀请（host 侧）——

const creatingInvite = ref(false);
const inviteError = ref<string | null>(null);
const ticketCopied = ref<string | null>(null);
let copyResetTimer: ReturnType<typeof setTimeout> | null = null;

// 倒计时状态机抽在 useInviteCountdown（watch expiresAt 驱动每秒刷新、卸载清理）。
const countdownState = useInviteCountdown(inviteExpiresAt);
const inviteCountdown = computed(() => countdownState.countdown.value);

// 票据仍在展示但倒计时已归零（后端已作废）：UI 停止广告死票据（B3）——
// 复制禁用、票据置灰划线、倒计时文案换「已过期」；再次生成邀请会正常覆盖。
const inviteExpired = computed(() => Boolean(inviteTicket.value) && countdownState.expired.value);

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
  const key = joinErrorKey(joinError.value);
  // 后端 reason 原样透传展示；前端哨兵映射为 i18n 文案。
  return key ? t(key) : joinError.value;
});

async function onJoin() {
  await sync.join(joinInput.value);
}

// —— 传输设置（自定义中继 + 自动推送全局开关，状态机抽在 useSyncTransportSettings）——

const {
  relayInput, relaySaved, relayError, savingRelay,
  autoPushMaster, autoPushNotify, autoPushError,
  saveRelay, saveAutoPush,
} = useSyncTransportSettings();

onUnmounted(() => {
  if (copyResetTimer) clearTimeout(copyResetTimer);
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
        <X class="size-4" />
      </button>
    </header>

    <DeviceListSection
      :devices="devices"
      :categories="categories"
      :confirming-revoke-id="confirmingKey"
      :load-error="loadError"
      :action-error="actionError"
      @auto-sync-change="onAutoSyncChange"
      @disconnect="onDisconnect"
      @revoke="onRevoke"
      @remove="onRemove"
      @send="onSend"
    />

    <InviteSection
      :pin="invitePin"
      :ticket="inviteTicket"
      :countdown="inviteCountdown"
      :expired="inviteExpired"
      :creating="creatingInvite"
      :error="inviteError"
      :copied="ticketCopied"
      @create="onCreateInvite"
      @cancel="onCancelInvite"
      @copy-ticket="copyTicket"
    />

    <JoinSection
      v-model="joinInput"
      :error-text="joinErrorText"
      :nearby="nearby"
      :selected-endpoint-id="selectedEndpointId"
      :pin="pinInput"
      @update:selected-endpoint-id="selectedEndpointId = $event"
      @update:pin="pinInput = $event"
      @join="onJoin"
      @join-nearby="sync.joinNearby()"
    />

    <RelaySettingsSection
      :relay-input="relayInput"
      :relay-saved="relaySaved"
      :relay-error="relayError"
      :saving-relay="savingRelay"
      :auto-push-master="autoPushMaster"
      :auto-push-notify="autoPushNotify"
      :auto-push-error="autoPushError"
      @update:relay-input="relayInput = $event"
      @update:auto-push-master="autoPushMaster = $event"
      @update:auto-push-notify="autoPushNotify = $event"
      @save-relay="saveRelay"
      @save-auto-push="saveAutoPush"
    />

    <!-- 配对确认弹窗 -->
    <PairConfirmDialog
      v-if="pairRequest"
      :request="pairRequest"
      @accept="sync.respondPair(true)"
      @reject="sync.respondPair(false)"
    />
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
