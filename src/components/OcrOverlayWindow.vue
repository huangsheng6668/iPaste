<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { useRegionSelection } from "../composables/useRegionSelection";

const monitorIndex = Number(new URLSearchParams(window.location.search).get("monitor") ?? "0");
const frameSrc = (() => {
  const framePath = new URLSearchParams(window.location.search).get("frame");
  return framePath ? convertFileSrc(framePath) : "";
})();
const { rect, isSelecting, beginSelection, updateSelection, endSelection } = useRegionSelection();
const submitFailed = ref(false);
const rootRef = ref<HTMLElement | null>(null);

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0) return;
  if (submitFailed.value) {
    void cancel();
    return;
  }
  rootRef.value?.setPointerCapture(event.pointerId);
  beginSelection(event.clientX, event.clientY);
}

function onPointerMove(event: PointerEvent) {
  updateSelection(event.clientX, event.clientY);
}

function onPointerUp(event: PointerEvent) {
  if (event.button !== 0) return;
  const selection = endSelection();
  if (!selection) return;
  void ipasteApi
    .submitScreenshotSelection({
      monitorIndex,
      left: selection.left,
      top: selection.top,
      width: selection.width,
      height: selection.height,
    })
    .catch(() => {
      // 截屏失败：留在遮罩内提示，点击或 Esc 关闭
      submitFailed.value = true;
    });
}

function cancel() {
  void ipasteApi.cancelScreenshotOcr().catch(() => {});
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
    return;
  }
  event.preventDefault();
  cancel();
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown, true);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown, true);
});
</script>

<template>
  <div
    ref="rootRef"
    class="ocr-overlay"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @contextmenu.prevent="cancel"
  >
    <img
      v-if="frameSrc"
      class="ocr-overlay-frame"
      :src="frameSrc"
      alt=""
    >
    <div
      v-if="!isSelecting"
      class="ocr-overlay-dim"
    />
    <div
      v-else
      class="ocr-overlay-selection"
      :style="{
        left: `${rect.left}px`,
        top: `${rect.top}px`,
        width: `${rect.width}px`,
        height: `${rect.height}px`,
      }"
    />
    <p
      v-if="submitFailed"
      class="ocr-overlay-hint ocr-overlay-hint-error"
    >
      {{ t("ocrScreenshot.recognizeFailed") }}
    </p>
    <p
      v-else
      class="ocr-overlay-hint"
    >
      {{ t("ocrScreenshot.overlayHint") }}
    </p>
  </div>
</template>

<style scoped>
.ocr-overlay {
  position: fixed;
  inset: 0;
  overflow: hidden;
  cursor: crosshair;
  user-select: none;
  touch-action: none;
  background: #000;
}

.ocr-overlay-frame {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: fill;
  pointer-events: none;
  user-select: none;
}

.ocr-overlay-dim {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 0.72);
}

/* 框选中心透亮、四周压暗：以巨大 box-shadow 代替整层遮罩挖洞 */
.ocr-overlay-selection {
  position: absolute;
  border: 2px solid var(--accent);
  border-radius: 2px;
  box-shadow: 0 0 0 100000px rgb(0 0 0 / 0.72);
}

.ocr-overlay-hint {
  position: absolute;
  left: 50%;
  bottom: 48px;
  transform: translateX(-50%);
  padding: 6px 14px;
  border-radius: 8px;
  background: rgb(0 0 0 / 0.65);
  color: #fff;
  font-size: 0.8125rem;
  pointer-events: none;
}

.ocr-overlay-hint-error {
  background: rgb(180 40 40 / 0.85);
}
</style>
