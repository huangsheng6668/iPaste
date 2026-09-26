<script setup lang="ts">
import { ChevronLeft, ChevronRight, Copy } from "lucide-vue-next";
import OcrEngineSelect from "../ocr/OcrEngineSelect.vue";
import { t } from "../../i18n";
import { OCR_LANGUAGE_OPTIONS } from "../../lib/ocrLanguages";
import type { OcrEngine } from "../../types";

// 查看器右侧 OCR 面板：状态文案、引擎/语言切换、结果文本与复制。
// 纯展示组件；选区清理与重跑由宿主经 emit 处理。
defineProps<{
  panelCollapsed: boolean;
  isRecognizing: boolean;
  /** 已有识别结果（标题与文本域据此切换）。 */
  hasResult: boolean;
  /** 结果含非空文本（复制按钮据此显隐，与原实现的 imageOcrResult?.text 一致）。 */
  hasResultText: boolean;
  summary: string;
  loadingText: string;
  errorText: string | null;
  text: string;
  engine: OcrEngine;
  selectedLanguage: string;
}>();

const emit = defineEmits<{
  toggle: [];
  rerun: [];
  changeLanguage: [language: string];
  pasteText: [];
  clearSelection: [];
}>();
</script>

<template>
  <aside
    class="viewer-image-ocr-panel"
    :class="{ 'viewer-image-ocr-panel-collapsed': panelCollapsed }"
    @wheel.stop
  >
    <button
      type="button"
      class="viewer-image-ocr-toggle"
      :aria-label="panelCollapsed ? t('viewer.expandOcr') : t('viewer.collapseOcr')"
      :data-tooltip="panelCollapsed ? t('viewer.expandOcr') : t('viewer.collapseOcr')"
      @pointerdown.stop
      @click="emit('toggle')"
    >
      <ChevronLeft
        v-if="panelCollapsed"
        class="size-4"
      />
      <ChevronRight
        v-else
        class="size-4"
      />
    </button>

    <div
      class="viewer-image-ocr-panel-body"
      @pointerdown.stop
      @wheel.stop
    >
      <div class="viewer-image-ocr-heading">
        <div class="min-w-0">
          <h2>{{ t("viewer.ocrTitle") }}</h2>
          <p v-if="hasResult">
            {{ summary }}
          </p>
          <p v-else-if="isRecognizing">
            {{ t("viewer.ocrRecognizing") }}
          </p>
          <p v-else>
            {{ t("viewer.ocrFailed") }}
          </p>
        </div>
        <OcrEngineSelect
          :engine="engine"
          :disabled="isRecognizing"
          @rerun="emit('rerun')"
        />
        <select
          class="ocr-language-select"
          :value="selectedLanguage"
          :disabled="isRecognizing"
          :aria-label="t('ocr.languageLabel')"
          @change="emit('changeLanguage', ($event.target as HTMLSelectElement).value)"
        >
          <option
            v-for="option in OCR_LANGUAGE_OPTIONS"
            :key="option.id"
            :value="option.id"
          >
            {{ t(option.labelKey) }}
          </option>
        </select>
        <button
          v-if="hasResultText"
          type="button"
          class="viewer-paste-button"
          @click="emit('pasteText')"
        >
          <Copy class="size-4" />
          <span>{{ t("viewer.copyText") }}</span>
        </button>
      </div>

      <p
        v-if="errorText"
        class="viewer-image-ocr-error"
      >
        {{ errorText }}
      </p>
      <p
        v-else-if="isRecognizing"
        class="viewer-image-ocr-loading"
      >
        {{ loadingText }}
      </p>
      <textarea
        v-else-if="hasResult"
        class="viewer-image-ocr-text subtle-scrollbar"
        :value="text"
        readonly
        spellcheck="false"
        @focus="emit('clearSelection')"
        @pointerdown="emit('clearSelection')"
      />
    </div>
  </aside>
</template>
