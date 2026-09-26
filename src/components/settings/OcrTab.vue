<script setup lang="ts">
import { computed } from "vue";
import { useSettingsStore } from "../../stores/settingsStore";
import { isOpenaiOcrConfigured } from "../../stores/lib/settings";
import OcrEngineSection from "./ocr/OcrEngineSection.vue";
import OcrCloudPromptsSection from "./ocr/OcrCloudPromptsSection.vue";
import OcrLocalSection from "./ocr/OcrLocalSection.vue";
import OcrMangaSection from "./ocr/OcrMangaSection.vue";
import type { OcrEngine } from "../../types";

const settings = useSettingsStore();
// 云引擎就绪状态供引擎选择区使用（与 useOpenaiOcr 共用同一断言，避免两处重复）。
const openaiConfigured = computed(() => isOpenaiOcrConfigured(settings.cloudOcr));

function updateOcrEngine(value: OcrEngine) {
  void settings.updateOcrEngine(value);
}


</script>

<template>
  <div class="settings-section">
    <OcrEngineSection
      :engine="settings.ocrEngine"
      :openai-configured="openaiConfigured"
      @select="updateOcrEngine"
    />

    <OcrCloudPromptsSection />

    <OcrLocalSection />

    <OcrMangaSection />
  </div>
</template>
