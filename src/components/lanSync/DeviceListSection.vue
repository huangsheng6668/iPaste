<script setup lang="ts">
import { t, type I18nKey } from "../../i18n";
import { fingerprintOf, statusKey, type DeviceStatusKey } from "../../lib/deviceDisplay";
import { ipasteApi } from "../../lib/ipasteApi";
import type { AutoSyncMode } from "../../types/generated/AutoSyncMode";
import type { DeviceInfo } from "../../types/generated/DeviceInfo";

defineProps<{
  devices: DeviceInfo[];
  /** 两击撤销确认：处于确认态的设备 nodeId。 */
  confirmingRevokeId: string | null;
  /** 设备列表加载失败横幅（useDeviceSync.loadError）。 */
  loadError: string | null;
  /** 设备行动作失败的就地错误（面板 runDeviceAction）。 */
  actionError: string | null;
}>();

const emit = defineEmits<{
  autoSyncChange: [entry: DeviceInfo, mode: AutoSyncMode, select: HTMLSelectElement];
  disconnect: [nodeId: string];
  revoke: [nodeId: string];
  remove: [nodeId: string];
}>();

function isRevoked(entry: DeviceInfo) {
  return Boolean(entry.device.revokedAt);
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

// 失败回滚（B5）需要改写本组件内的 select 显示，因此把 select 元素随事件上抛，
// 由面板在动作失败分支恢复后端权威值。
function onAutoSyncChange(entry: DeviceInfo, event: Event) {
  const select = event.target as HTMLSelectElement;
  emit("autoSyncChange", entry, select.value as AutoSyncMode, select);
}

// 接收偏好为 fire-and-forget 直调：后端落库后会 emit 设备列表事件，
// 面板订阅刷新时 select 随 :value 绑定自然回到后端权威值。
function onRecvSyncChange(entry: DeviceInfo, event: Event) {
  const mode = (event.target as HTMLSelectElement).value as AutoSyncMode;
  void ipasteApi.deviceSetRecvSync(entry.device.nodeId, mode);
}
</script>

<template>
  <!-- 设备列表 -->
  <section class="lan-section">
    <h2 class="lan-section-title">
      {{ t("deviceSync.list.title") }}
    </h2>
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
        <select
          class="lan-select"
          :disabled="isRevoked(entry)"
          :value="entry.device.recvAutoSyncMode"
          :aria-label="t('deviceSync.recvSync.label')"
          @change="onRecvSyncChange(entry, $event)"
        >
          <option value="text_only">
            {{ t("deviceSync.recvSync.textOnly") }}
          </option>
          <option value="all">
            {{ t("deviceSync.recvSync.all") }}
          </option>
          <option value="off">
            {{ t("deviceSync.recvSync.off") }}
          </option>
        </select>
        <div class="lan-device-actions">
          <button
            v-if="!isRevoked(entry) && entry.online !== 'offline'"
            type="button"
            class="lan-action"
            @click="emit('disconnect', entry.device.nodeId)"
          >
            {{ t("deviceSync.action.disconnect") }}
          </button>
          <button
            v-if="!isRevoked(entry)"
            type="button"
            class="lan-action"
            :class="{ 'lan-action-danger': confirmingRevokeId === entry.device.nodeId }"
            @click="emit('revoke', entry.device.nodeId)"
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
            @click="emit('remove', entry.device.nodeId)"
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
  </section>
</template>
