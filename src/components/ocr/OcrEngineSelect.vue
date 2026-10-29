<script setup lang="ts">
import { useOcrEngineSelect } from "../../composables/useOcrEngineSelect";
import { t } from "../../i18n";
import type { OcrEngine } from "../../types";

const props = withDefaults(
  defineProps<{
    engine: OcrEngine;
    disabled?: boolean;
  }>(),
  { disabled: false },
);

const emit = defineEmits<{
  "update:engine": [value: OcrEngine];
  rerun: [];
}>();

// 包装既有引擎切换逻辑：选项复用设置页文案、未配置的云引擎禁用、
// 切换成功写入全局设置后通知调用方用新引擎重跑识别
const { ocrEngineOptions, selectedEngineValue, switchOcrEngine } = useOcrEngineSelect(
  () => props.engine,
);

async function changeEngine(event: Event) {
  if (props.disabled) return;
  const engine = await switchOcrEngine((event.target as HTMLSelectElement).value);
  if (engine) {
    emit("update:engine", engine);
    emit("rerun");
  }
}
</script>

<template>
  <!-- 无边框窗口标题区场景：阻止 mousedown 冒泡，避免触发原生窗口拖动吞掉点击 -->
  <select
    class="ocr-language-select"
    :value="selectedEngineValue"
    :disabled="disabled"
    :aria-label="t('ocr.engineLabel')"
    :title="t('ocr.engineLabel')"
    @mousedown.stop
    @change="changeEngine"
  >
    <option
      v-for="option in ocrEngineOptions"
      :key="option.value"
      :value="option.value"
      :disabled="!option.ready"
    >
      {{ option.ready ? option.label : `${option.label} · ${t('ocr.engineUnconfigured')}` }}
    </option>
  </select>
</template>

<style scoped>
/* 引擎下拉自带的控件皮肤（以 OCR 结果窗版本为基准）；布局差异
   （结果窗与查看器 OCR 面板的弹性伸缩）由使用方上下文样式提供 */
.ocr-language-select {
  height: var(--control-sm);
  min-width: 0;
  max-width: 100%;
  padding: 0 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-1);
  font-size: var(--text-sm);
  font-weight: var(--fw-medium);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-out),
    background-color var(--dur-fast) var(--ease-out);
}

.ocr-language-select:hover:not(:disabled) {
  border-color: var(--border-accent);
  background: var(--surface-hover);
}

.ocr-language-select:focus-visible {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.ocr-language-select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
