<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { t } from "../../i18n";
import type { NearbyDevice } from "../../types/generated/NearbyDevice";

const props = defineProps<{
  /** 面板映射后的失败文案（前端哨兵已 i18n 化，后端 reason 原样透传）。 */
  errorText: string | null;
  /** 完整票据输入（跨网络路径，原样保留）。 */
  modelValue: string;
  nearby: NearbyDevice[];
  selectedEndpointId: string | null;
  pin: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  "update:selectedEndpointId": [value: string];
  "update:pin": [value: string];
  join: [];
  joinNearby: [];
}>();

const pinValid = computed(() => /^\d{6}$/.test(props.pin));

/** 确认层是否打开：配对动作先经指纹核对，确认后才 emit joinNearby。 */
const confirming = ref(false);

/** 所选条目指纹：endpointId 前 8 个 hex 字符，与主机侧 fingerprint_hex 同源。 */
const selectedFingerprint = computed(() => {
  const device = props.nearby.find((d) => d.endpointId === props.selectedEndpointId);
  return device ? device.endpointId.slice(0, 8) : "";
});

/** 所选条目名称：确认层中与指纹一起供用户核对目标设备。 */
const selectedDeviceName = computed(() => {
  const device = props.nearby.find((d) => d.endpointId === props.selectedEndpointId);
  return device ? device.deviceName : "";
});

/** 确认按钮引用：确认层打开后把焦点移入，键盘流从确认动作继续。 */
const confirmButtonRef = ref<HTMLButtonElement | null>(null);

async function requestJoin() {
  if (!pinValid.value || !props.selectedEndpointId) return;
  confirming.value = true;
  await nextTick();
  confirmButtonRef.value?.focus();
}

function confirmJoin() {
  confirming.value = false;
  emit("joinNearby");
}

function cancelJoin() {
  confirming.value = false;
}
</script>

<template>
  <!-- 加入设备（guest 侧）：附近设备 + PIN 为主，票据为折叠的次要入口 -->
  <section class="lan-section">
    <h2 class="lan-section-title">
      {{ t("deviceSync.join.title") }}
    </h2>

    <span class="lan-label">{{ t("deviceSync.nearby.title") }}</span>
    <p
      v-if="nearby.length === 0"
      class="lan-empty"
    >
      {{ t("deviceSync.nearby.empty") }}
    </p>
    <ul
      v-else
      class="lan-nearby-list"
      role="listbox"
      :aria-label="t('deviceSync.nearby.title')"
    >
      <li
        v-for="device in nearby"
        :key="device.endpointId"
      >
        <button
          type="button"
          class="lan-nearby-item"
          :class="{ 'lan-nearby-item-selected': device.endpointId === selectedEndpointId }"
          role="option"
          :aria-selected="device.endpointId === selectedEndpointId"
          @click="emit('update:selectedEndpointId', device.endpointId)"
        >
          {{ device.deviceName }}
        </button>
      </li>
    </ul>

    <label
      class="lan-label"
      for="lan-pin-input"
    >{{ t("deviceSync.pin.label") }}</label>
    <div class="lan-ticket-row">
      <input
        id="lan-pin-input"
        :value="pin"
        class="lan-input lan-pin-input"
        type="text"
        inputmode="numeric"
        autocomplete="off"
        maxlength="6"
        :placeholder="t('deviceSync.pin.placeholder')"
        @input="emit('update:pin', ($event.target as HTMLInputElement).value)"
        @keydown.enter="requestJoin"
      >
      <button
        id="lan-pin-join"
        type="button"
        class="lan-button lan-button-primary"
        :disabled="!selectedEndpointId || !pinValid"
        @click="requestJoin"
      >
        {{ t("deviceSync.pin.button") }}
      </button>
    </div>

    <details class="lan-collapsible">
      <summary class="lan-collapsible-header">
        {{ t("deviceSync.join.ticketFallback") }}
      </summary>
      <label
        class="lan-label"
        for="lan-join-input"
      >{{ t("deviceSync.join.label") }}</label>
      <div class="lan-ticket-row">
        <input
          id="lan-join-input"
          :value="modelValue"
          class="lan-input"
          type="text"
          :placeholder="t('deviceSync.join.label')"
          @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
          @keydown.enter="emit('join')"
        >
        <button
          type="button"
          class="lan-button lan-button-primary"
          :disabled="!modelValue.trim()"
          @click="emit('join')"
        >
          {{ t("deviceSync.join.button") }}
        </button>
      </div>
    </details>

    <p
      v-if="errorText"
      class="lan-error"
      role="alert"
    >
      {{ errorText }}
    </p>

    <!-- 配对确认层：核对所选设备指纹与主机侧「本机指纹」一致后再 emit（H2 防钓鱼） -->
    <div
      v-if="confirming"
      class="dialog-backdrop"
      @click.self="cancelJoin"
    >
      <div
        class="dialog-panel"
        role="dialog"
        aria-modal="true"
        @keydown.esc="cancelJoin"
      >
        <div class="dialog-body">
          <h2 class="dialog-title">
            {{ t("deviceSync.joinConfirm.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("deviceSync.joinConfirm.hint") }}
          </p>
          <p class="mt-3 text-sm font-semibold text-[var(--text-1)]">
            {{ selectedDeviceName }}
          </p>
          <p class="mt-1 font-mono text-lg text-[var(--text-1)]">
            {{ selectedFingerprint }}
          </p>
        </div>
        <div class="dialog-footer">
          <button
            type="button"
            class="btn-ghost"
            @click="cancelJoin"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            ref="confirmButtonRef"
            type="button"
            class="btn-primary"
            :disabled="!selectedFingerprint"
            @click="confirmJoin"
          >
            {{ t("deviceSync.joinConfirm.confirm") }}
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
