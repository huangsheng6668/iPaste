<script setup lang="ts">
import { t } from "../../i18n";

defineProps<{
  /** join 失败文案（面板已映射 INVALID_TICKET）。 */
  errorText: string | null;
  modelValue: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  join: [];
}>();
</script>

<template>
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
    <p
      v-if="errorText"
      class="lan-error"
    >
      {{ errorText }}
    </p>
  </section>
</template>
