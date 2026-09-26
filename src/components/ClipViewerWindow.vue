<script setup lang="ts">
import {
  ClipboardPaste,
  CornerDownLeft,
  Copy,
  Image as ImageIcon,
  Save,
  X,
} from "lucide-vue-next";
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { useImageViewer } from "../composables/useImageViewer";
import { useImageOcr } from "../composables/useImageOcr";
import { useClipEditor, type ClipEditorHandle } from "../composables/useClipEditor";
import { useViewerWindow } from "../composables/useViewerWindow";
import ViewerToolbar from "./viewer/ViewerToolbar.vue";
import ViewerOcrPanel from "./viewer/ViewerOcrPanel.vue";
import ViewerStage from "./viewer/ViewerStage.vue";
import { clipImageSrc } from "../lib/clipMedia";
import { isEditableTarget } from "../lib/dom";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "../stores/ipasteStore";
import { useSettingsStore } from "../stores/settingsStore";
import { typeLabel } from "../lib/format";
import type { ClipViewerPayload } from "../types";

const payload = ref<ClipViewerPayload | null>(null);
const error = ref<string | null>(null);

const item = computed(() => payload.value?.item);
const title = computed(() => {
  const current = item.value;
  if (!current) return t("viewer.titleFallback");
  return current.displayName?.trim() || t("clip.clipboardTitle", { type: typeLabel(current.clipType) });
});
const isImage = computed(() => item.value?.clipType === "image");
const imageSrc = computed(() => (item.value ? clipImageSrc(item.value) : ""));
const viewerCallbacks = { clearImageTextSelection: () => clearImageTextSelection() };
const viewer = useImageViewer(viewerCallbacks);
const {
  imageStageElement, imageViewMode, isImageDragging,
  canPanImage, isImageActualSize,
  imageZoomLabel, imageStyle, imageFrameStyle, canZoomOutImage, canZoomInImage,
  updateImageStageSize, handleImageLoad, resetImageViewState, fitImageToStage,
  showImageActualSize, zoomImageIn, zoomImageOut, rotateImageClockwise,
  handleImageWheel, startImagePan, moveImagePan, finishImagePan, endImageDrag, clampImagePan,
} = viewer;
// 图片 OCR 的选区浮层要写回编辑器状态，而编辑器要等 useClipEditor 运行后才存在：
// 以可空 ref 声明依赖方向，编辑器创建后一次赋值，读取只发生在交互期
const editorHandle = shallowRef<ClipEditorHandle | null>(null);
const ocr = useImageOcr(viewer, { item, isImage, editor: editorHandle });
const {
  isRecognizingImage, imageOcrResult, imageOcrError, isImageOcrPanelCollapsed,
  showImageOcrPanel, ocrTextLayerStyle, imageOcrSummary, imageOcrLoadingText,
  imageOcrLines, imageOcrWords, selectedImageOcrWordIndexes, imageOcrSelectionHighlights, imageOcrSelectionText,
  imageOcrText, recognizeImageText, pasteImageOcrText, toggleImageOcrPanel,
  selectedOcrLanguage, changeOcrLanguage,
  startImageOcrSelection, moveImageOcrSelection, finishImageOcrSelection,
  endImageOcrSelection,
  clearImageTextSelection, resetOcrState,
} = ocr;
// 独立窗口不走 App.vue 的 store.load（见 App.vue 早退分支），OCR 面板的
// 引擎切换需读取当前引擎与云 OCR 配置状态
const appStore = useIpasteStore(); // 窗口快照自举（load）
const settings = useSettingsStore();
// isPinned 的 ref 由下方 useViewerWindow 持有（其 hasChanged 依赖编辑器，只能后建），
// 经 computed 延迟取值，编辑器选项构造时即为最终形态
const editor = useClipEditor(item, {
  payload,
  isPinned: computed(() => viewerWindow.isPinned.value),
  isImage,
  ocr,
  error,
});
const {
  draftText, editorElement, selectionAction, hasChanged, stats, metricText, lines,
  resetDraft, applyChanges, pasteDraft, pasteSelection, scheduleSelectionAction,
  hideSelectionAction, focusEditorAtStart,
} = editor;
editorHandle.value = { hideSelectionAction, selectionAction };
const viewerWindow = useViewerWindow(editor, {
  error,
  payload,
  isImage,
  autoRecognize: recognizeImageText,
});
const {
  isPinned, showClosePrompt, isSavingBeforeClose,
  startWindowDrag, togglePinned, closeWindow, cancelClose,
  loadPayload, saveAndClose, discardAndClose,
} = viewerWindow;
const displayTime = computed(() => {
  const current = item.value;
  if (!current) return "";
  return current.collection === "history" ? current.lastCapturedAt : current.createdAt;
});

onMounted(async () => {
  loadPayload();
  void appStore.load().catch(() => undefined);
  document.addEventListener("keydown", handleViewerKeydown, true);
  window.addEventListener("resize", handleViewerResize);
  void nextTick(focusEditorAtStart);
});

onUnmounted(() => {
  document.removeEventListener("keydown", handleViewerKeydown, true);
  window.removeEventListener("resize", handleViewerResize);
});

watch(imageSrc, () => {
  resetImageViewState();
  resetOcrState();
});

// ViewerStage 把舞台 DOM 经 defineExpose 暴露出来；命中测试与选区定位都依赖
// 真实元素，这里同步到 useImageViewer 持有的 imageStageElement。
const viewerStageComponent = ref<InstanceType<typeof ViewerStage> | null>(null);
watch(
  viewerStageComponent,
  (component) => {
    imageStageElement.value = component?.stageElement ?? null;
  },
  { flush: "post" },
);


function handleViewerKeydown(event: KeyboardEvent) {
  if (
    isImage.value
    && event.key.toLowerCase() === "c"
    && (event.metaKey || event.ctrlKey)
    && !event.altKey
    && !event.shiftKey
    && imageOcrSelectionText.value.trim()
  ) {
    event.preventDefault();
    void ipasteApi.copyClip("text", imageOcrSelectionText.value);
    return;
  }

  // O：识别图片文字（无修饰键，仅图片模式且未在识别中）
  if (
    isImage.value
    && !isRecognizingImage.value
    && event.key.toLowerCase() === "o"
    && !event.metaKey
    && !event.ctrlKey
    && !event.altKey
    && !event.shiftKey
    && !event.defaultPrevented
    && !isEditableTarget(event.target)
  ) {
    event.preventDefault();
    void recognizeImageText();
    return;
  }

  if (
    event.defaultPrevented
    || event.key !== "Escape"
    || event.metaKey
    || event.ctrlKey
    || event.altKey
    || event.shiftKey
  ) {
    return;
  }

  event.preventDefault();
  if (showClosePrompt.value) {
    cancelClose();
    return;
  }

  void closeWindow();
}

function handleViewerResize() {
  hideSelectionAction();
  clearImageTextSelection();
  updateImageStageSize();
  if (isImage.value && imageViewMode.value === "fit") {
    fitImageToStage();
    return;
  }

  clampImagePan();
}

</script>

<template>
  <main class="clip-viewer-shell">
    <ViewerToolbar
      :is-image="isImage"
      :is-pinned="isPinned"
      :title="title"
      :item="item"
      :display-time="displayTime"
      :can-zoom-out-image="canZoomOutImage"
      :can-zoom-in-image="canZoomInImage"
      :is-image-actual-size="isImageActualSize"
      :image-zoom-label="imageZoomLabel"
      :has-changed="hasChanged"
      :is-recognizing-image="isRecognizingImage"
      :is-ocr-result-active="Boolean(imageOcrResult)"
      @toggle-pin="togglePinned"
      @drag="startWindowDrag"
      @zoom-out="zoomImageOut"
      @zoom-in="zoomImageIn"
      @actual-size="showImageActualSize"
      @rotate="rotateImageClockwise"
      @recognize-text="recognizeImageText"
      @reset-draft="resetDraft"
      @apply-changes="applyChanges"
      @close="closeWindow"
    />

    <div
      v-if="error"
      class="viewer-error"
    >
      {{ error }}
    </div>

    <section
      v-else-if="item"
      class="clip-viewer-content"
      :class="{
        'clip-viewer-content-image': isImage,
      }"
    >
      <template v-if="isImage">
        <ViewerStage
          ref="viewerStageComponent"
          :src="imageSrc"
          :image-style="imageStyle"
          :image-frame-style="imageFrameStyle"
          :ocr-text-layer-style="ocrTextLayerStyle"
          :can-pan="canPanImage"
          :is-dragging="isImageDragging"
          :is-recognizing="isRecognizingImage"
          :lines="imageOcrLines"
          :words="imageOcrWords"
          :selected-word-indexes="selectedImageOcrWordIndexes"
          :highlights="imageOcrSelectionHighlights"
          @wheel="handleImageWheel"
          @pointer-down="startImagePan"
          @pointer-move="moveImagePan"
          @pointer-up="finishImagePan"
          @pointer-cancel="finishImagePan"
          @lost-pointer-capture="endImageDrag"
          @image-load="handleImageLoad"
          @word-pointer-down="({ event, selectionIndex }) => startImageOcrSelection(event, selectionIndex)"
          @word-pointer-move="moveImageOcrSelection"
          @word-pointer-up="finishImageOcrSelection"
          @word-pointer-cancel="finishImageOcrSelection"
          @word-pointer-lost-capture="endImageOcrSelection"
        >
          <ViewerOcrPanel
            v-if="showImageOcrPanel"
            :panel-collapsed="isImageOcrPanelCollapsed"
            :is-recognizing="isRecognizingImage"
            :has-result="Boolean(imageOcrResult)"
            :has-result-text="Boolean(imageOcrResult?.text)"
            :summary="imageOcrSummary"
            :loading-text="imageOcrLoadingText"
            :error-text="imageOcrError"
            :text="imageOcrText"
            :engine="settings.ocrEngine"
            :selected-language="selectedOcrLanguage"
            @toggle="toggleImageOcrPanel"
            @rerun="recognizeImageText"
            @change-language="changeOcrLanguage"
            @paste-text="pasteImageOcrText"
            @clear-selection="clearImageTextSelection"
          />
        </ViewerStage>
      </template>

      <textarea
        v-else
        ref="editorElement"
        v-model="draftText"
        class="viewer-editor subtle-scrollbar"
        spellcheck="false"
        @mouseup="scheduleSelectionAction"
        @keyup="scheduleSelectionAction"
        @blur="hideSelectionAction"
      />

      <button
        v-if="selectionAction"
        type="button"
        class="selection-paste-button"
        :style="{ left: `${selectionAction.left}px`, top: `${selectionAction.top}px` }"
        @mousedown.prevent
        @click="pasteSelection"
      >
        <Copy
          v-if="selectionAction.mode === 'copy'"
          class="size-3.5"
        />
        <ClipboardPaste
          v-else
          class="size-3.5"
        />
        <span>{{ selectionAction.mode === "copy" ? t("viewer.copySelection") : t("viewer.pasteSelection") }}</span>
      </button>
    </section>

    <footer
      v-if="item"
      class="clip-viewer-footer"
    >
      <span>{{ isImage ? metricText : stats }}</span>
      <span v-if="!isImage">{{ t("common.lineCount", { count: lines }) }}</span>
      <button
        type="button"
        class="viewer-paste-button"
        @click="pasteDraft"
      >
        <ImageIcon
          v-if="isImage"
          class="size-4"
        />
        <CornerDownLeft
          v-else
          class="size-4"
        />
        <span>{{ isImage ? t("viewer.pasteImage") : t("viewer.pasteCurrent") }}</span>
      </button>
    </footer>

    <div
      v-if="showClosePrompt"
      class="viewer-close-backdrop"
      @mousedown.self="cancelClose"
    >
      <section
        class="viewer-close-dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="viewer-close-title"
      >
        <h2 id="viewer-close-title">
          {{ t("viewer.saveChangesTitle") }}
        </h2>
        <p>{{ t("viewer.saveChangesDescription") }}</p>
        <div class="viewer-close-actions">
          <button
            type="button"
            class="viewer-action-button"
            :disabled="isSavingBeforeClose"
            @click="cancelClose"
          >
            <span>{{ t("common.cancel") }}</span>
          </button>
          <button
            type="button"
            class="viewer-action-button viewer-action-button-danger"
            :disabled="isSavingBeforeClose"
            @click="discardAndClose"
          >
            <X class="size-4" />
            <span>{{ t("viewer.discard") }}</span>
          </button>
          <button
            type="button"
            class="viewer-action-button viewer-action-button-primary"
            :disabled="isSavingBeforeClose"
            @click="saveAndClose"
          >
            <Save class="size-4" />
            <span>{{ isSavingBeforeClose ? t("common.saving") : t("viewer.saveAndClose") }}</span>
          </button>
        </div>
      </section>
    </div>
  </main>
</template>
