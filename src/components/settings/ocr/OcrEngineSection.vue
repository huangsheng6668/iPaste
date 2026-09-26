<script setup lang="ts">
import { computed } from "vue";
import { ScanText } from "lucide-vue-next";
import { t } from "../../../i18n";
import type { OcrEngine } from "../../../types";

// OCR 引擎选择区（本地 / OpenAI 兼容）：纯展示 + emit，
// 当前引擎与云引擎就绪状态由宿主传入，切换经 select 交回宿主落库。
const props = defineProps<{
  engine: OcrEngine;
  /** 云引擎（OpenAI 兼容）是否已配置齐 base/model/key。 */
  openaiConfigured: boolean;
}>();

const emit = defineEmits<{
  select: [engine: OcrEngine];
}>();

const engineOptions: Array<{ value: OcrEngine; label: string; description: string }> = [
  {
    value: "local",
    label: t("settings.bigmodel.engineLocal"),
    description: t("settings.bigmodel.engineLocalDescription"),
  },
  {
    value: "openai",
    label: t("settings.openai.engineOpenai"),
    description: t("settings.openai.engineOpenaiDescription"),
  },
];

/** 云引擎未配置齐时视为未就绪（本地引擎恒就绪）。 */
const engineReady = computed(() => (props.engine === "openai" ? props.openaiConfigured : true));

const badgeLabel = computed(() =>
  props.engine === "openai" ? t("settings.openai.engineOpenai") : t("settings.bigmodel.engineLocal"),
);
</script>

<template>
  <section class="settings-panel settings-column-panel">
    <div class="settings-panel-heading">
      <div class="settings-icon settings-icon-violet">
        <ScanText class="size-5" />
      </div>
      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.bigmodel.engineTitle") }}
        </h2>
        <p class="mt-1 text-sm text-[var(--text-2)]">
          {{ t("settings.bigmodel.engineSubtitle") }}
        </p>
      </div>
      <span
        class="ocr-status-badge"
        :class="{ 'ocr-status-badge-ready': engineReady }"
      >
        {{ badgeLabel }}
      </span>
    </div>

    <div class="ocr-mode-options">
      <button
        v-for="option in engineOptions"
        :key="option.value"
        type="button"
        class="ocr-mode-option"
        :class="{ 'ocr-mode-option-active': engine === option.value }"
        :aria-pressed="engine === option.value"
        @click="emit('select', option.value)"
      >
        <span class="ocr-mode-option-header">
          <span>{{ option.label }}</span>
        </span>
        <span class="ocr-mode-option-description">{{ option.description }}</span>
      </button>
    </div>
    <p class="ocr-mode-hint">
      {{ t("settings.bigmodel.engineHint") }}
    </p>
  </section>
</template>
