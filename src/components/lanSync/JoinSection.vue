<script setup lang="ts">
import { computed } from "vue";
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
        @keydown.enter="emit('joinNearby')"
      >
      <button
        id="lan-pin-join"
        type="button"
        class="lan-button lan-button-primary"
        :disabled="!selectedEndpointId || !pinValid"
        @click="emit('joinNearby')"
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
  </section>
</template>
