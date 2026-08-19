<script setup lang="ts">
import {
  ChevronLeft,
  ChevronRight,
  ClipboardPaste,
  CornerDownLeft,
  Copy,
  Image as ImageIcon,
  LoaderCircle,
  Maximize2,
  Pin,
  PinOff,
  RotateCcw,
  RotateCw,
  ScanText,
  Save,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-vue-next";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useImageViewer } from "../composables/useImageViewer";
import { useImageOcr } from "../composables/useImageOcr";
import { useClipEditor } from "../composables/useClipEditor";
import { useViewerWindow } from "../composables/useViewerWindow";
import { clipImageSrc } from "../lib/clipMedia";
import { t } from "../i18n";
import { clipViewerStorageKey, ipasteApi } from "../lib/ipasteApi";
import { formatTime, typeLabel } from "../lib/format";
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
const editorHandle = { hideSelectionAction: () => {}, selectionAction: ref<{ left: number; top: number; text: string; mode: "paste" | "copy" } | null>(null) };
const ocr = useImageOcr(viewer, { item, isImage, editor: editorHandle });
const {
  isRecognizingImage, imageOcrResult, imageOcrError, isImageOcrPanelCollapsed,
  showImageOcrPanel, ocrTextLayerStyle, imageOcrSummary, imageOcrLoadingText,
  imageOcrLines, imageOcrWords, selectedImageOcrWordIndexes, imageOcrSelectionHighlights, imageOcrSelectionText,
  imageOcrText, recognizeImageText, pasteImageOcrText, toggleImageOcrPanel,
  startImageOcrSelection, moveImageOcrSelection, finishImageOcrSelection,
  endImageOcrSelection,
  clearImageTextSelection, resetOcrState,
} = ocr;
const editorOptions = { payload, isPinned: ref(false), isImage, ocr, error };
const editor = useClipEditor(item, editorOptions);
const {
  draftText, editorElement, selectionAction, hasChanged, stats, metricText, lines,
  resetDraft, applyChanges, pasteDraft, pasteSelection, scheduleSelectionAction,
  hideSelectionAction, focusEditorAtStart,
} = editor;
editorHandle.hideSelectionAction = hideSelectionAction;
editorHandle.selectionAction = selectionAction;
const viewerWindow = useViewerWindow(editor.hasChanged, { error, hideSelectionAction });
const {
  windowLabel, isPinned, showClosePrompt, isSavingBeforeClose,
  startWindowDrag, togglePinned, closeWindow, cancelClose,
  forceCloseWindow,
} = viewerWindow;
editorOptions.isPinned = viewerWindow.isPinned;
const displayTime = computed(() => {
  const current = item.value;
  if (!current) return "";
  return current.collection === "history" ? current.lastCapturedAt : current.createdAt;
});

onMounted(async () => {
  loadPayload();
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

function loadPayload() {
  const params = new URLSearchParams(window.location.search);
  const label = params.get("label");
  if (!label) {
    error.value = t("viewer.payloadMissing");
    return;
  }
  windowLabel.value = label;

  const raw = localStorage.getItem(clipViewerStorageKey(label));
  if (!raw) {
    error.value = t("viewer.payloadExpired");
    return;
  }

  try {
    payload.value = JSON.parse(raw) as ClipViewerPayload;
    draftText.value = payload.value.item.text;
  } catch {
    error.value = t("viewer.payloadInvalid");
    return;
  }

  // 主面板「识别文字」一键入口：打开即自动识别
  if (params.get("auto-recognize") === "1" && isImage.value) {
    void recognizeImageText();
  }
}


async function saveAndClose() {
  if (!hasChanged.value) {
    await forceCloseWindow();
    return;
  }

  isSavingBeforeClose.value = true;
  try {
    await applyChanges();
  } finally {
    isSavingBeforeClose.value = false;
  }

  if (!hasChanged.value) {
    showClosePrompt.value = false;
    await forceCloseWindow();
  }
}

async function discardAndClose() {
  showClosePrompt.value = false;
  await forceCloseWindow();
}


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

function isEditableTarget(target: EventTarget | null): boolean {
  const element = target instanceof HTMLElement ? target : null;
  if (!element) return false;
  return element.isContentEditable
    || element.tagName === "INPUT"
    || element.tagName === "TEXTAREA";
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
    <header
      class="clip-viewer-toolbar"
      :class="{ 'clip-viewer-toolbar-image': isImage }"
    >
      <button
        type="button"
        class="viewer-icon-button"
        :class="{ 'viewer-icon-button-active': isPinned }"
        :aria-label="isPinned ? t('viewer.unpin') : t('viewer.pin')"
        :data-tooltip="isPinned ? t('viewer.unpin') : t('viewer.pin')"
        @click="togglePinned"
      >
        <PinOff
          v-if="isPinned"
          class="size-4"
        />
        <Pin
          v-else
          class="size-4"
        />
      </button>

      <div
        class="clip-viewer-drag-zone min-w-0 flex-1"
        @mousedown="startWindowDrag"
      >
        <h1 class="truncate text-base font-semibold text-slate-950">
          {{ title }}
        </h1>
        <p
          v-if="item"
          class="truncate text-xs text-slate-500"
        >
          {{ typeLabel(item.clipType) }} · {{ formatTime(displayTime) }}
        </p>
      </div>

      <div
        v-if="isImage"
        class="viewer-image-toolbox"
        role="toolbar"
        :aria-label="t('viewer.imageToolbar')"
        @pointerdown.stop
        @wheel.stop
      >
        <button
          type="button"
          class="viewer-icon-button"
          :disabled="!canZoomOutImage"
          :aria-label="t('viewer.zoomOut')"
          :data-tooltip="t('viewer.zoomOut')"
          @click="zoomImageOut"
        >
          <ZoomOut class="size-4" />
        </button>
        <button
          type="button"
          class="viewer-icon-button"
          :disabled="!canZoomInImage"
          :aria-label="t('viewer.zoomIn')"
          :data-tooltip="t('viewer.zoomIn')"
          @click="zoomImageIn"
        >
          <ZoomIn class="size-4" />
        </button>
        <button
          type="button"
          class="viewer-icon-button"
          :class="{ 'viewer-icon-button-active': isImageActualSize }"
          :aria-label="t('viewer.actualSize')"
          :data-tooltip="t('viewer.actualSize')"
          @click="showImageActualSize"
        >
          <Maximize2 class="size-4" />
        </button>
        <button
          type="button"
          class="viewer-icon-button"
          :aria-label="t('viewer.rotateClockwise')"
          :data-tooltip="t('viewer.rotateClockwise')"
          @click="rotateImageClockwise"
        >
          <RotateCw class="size-4" />
        </button>
        <button
          type="button"
          class="viewer-image-zoom-label"
          :aria-label="t('viewer.restore100')"
          :data-tooltip="t('viewer.restore100')"
          @click="showImageActualSize"
        >
          {{ imageZoomLabel }}
        </button>
        <button
          type="button"
          class="viewer-icon-button"
          :class="{ 'viewer-icon-button-active': Boolean(imageOcrResult) }"
          :disabled="isRecognizingImage"
          :aria-label="t('viewer.recognizeText')"
          :data-tooltip="t('viewer.recognizeText')"
          @click="recognizeImageText"
        >
          <LoaderCircle
            v-if="isRecognizingImage"
            class="size-4 update-spin"
          />
          <ScanText
            v-else
            class="size-4"
          />
        </button>
      </div>

      <button
        v-if="!isImage"
        type="button"
        class="viewer-action-button"
        :disabled="!hasChanged"
        @click="resetDraft"
      >
        <RotateCcw class="size-4" />
        <span>{{ t("viewer.reset") }}</span>
      </button>

      <button
        v-if="!isImage"
        type="button"
        class="viewer-action-button viewer-action-button-primary"
        :disabled="!hasChanged"
        @click="applyChanges"
      >
        <Save class="size-4" />
        <span>{{ t("viewer.applyChanges") }}</span>
      </button>

      <button
        type="button"
        class="viewer-icon-button"
        :aria-label="t('viewer.closeWindow')"
        :data-tooltip="t('viewer.closeWindow')"
        @click="closeWindow"
      >
        <X class="size-4" />
      </button>
    </header>

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
        <div
          ref="imageStageElement"
          class="viewer-image-stage"
          :class="{
            'viewer-image-stage-pannable': canPanImage,
            'viewer-image-stage-dragging': isImageDragging,
            'viewer-image-stage-recognizing': isRecognizingImage,
          }"
          @wheel="handleImageWheel"
          @pointerdown="startImagePan"
          @pointermove="moveImagePan"
          @pointerup="finishImagePan"
          @pointercancel="finishImagePan"
          @lostpointercapture="endImageDrag"
        >
          <div
            class="viewer-image-frame"
            :style="imageFrameStyle"
          >
            <img
              :src="imageSrc"
              :style="imageStyle"
              draggable="false"
              :alt="t('common.imagePreviewAlt')"
              @load="handleImageLoad"
            >
            <div
              v-if="imageOcrLines.length"
              class="viewer-image-ocr-layer"
              :style="ocrTextLayerStyle"
            >
              <span
                v-for="highlight in imageOcrSelectionHighlights"
                :key="highlight.key"
                class="viewer-image-ocr-highlight"
                :style="{
                  left: `${highlight.left}px`,
                  top: `${highlight.top}px`,
                  width: `${highlight.width}px`,
                  height: `${highlight.height}px`,
                }"
              />
              <span
                v-for="line in imageOcrLines"
                :key="line.key"
                class="viewer-image-ocr-line"
                :style="{
                  left: `${line.left}px`,
                  top: `${line.top}px`,
                  width: `${line.width}px`,
                  height: `${line.height}px`,
                  fontSize: `${Math.max(10, line.height * 0.84)}px`,
                }"
                aria-hidden="true"
              >
                {{ line.text }}
              </span>
              <button
                v-for="word in imageOcrWords"
                :key="`${word.lineKey}:${word.selectionIndex}`"
                type="button"
                class="viewer-image-ocr-word"
                :class="{ 'viewer-image-ocr-word-selected': selectedImageOcrWordIndexes.has(word.selectionIndex) }"
                :data-ocr-word-index="word.selectionIndex"
                :aria-label="word.text"
                :style="{
                  left: `${word.left}px`,
                  top: `${word.top}px`,
                  width: `${word.width}px`,
                  height: `${word.height}px`,
                }"
                @pointerdown="startImageOcrSelection($event, word.selectionIndex)"
                @pointermove="moveImageOcrSelection"
                @pointerup="finishImageOcrSelection"
                @pointercancel="finishImageOcrSelection"
                @lostpointercapture="endImageOcrSelection"
              />
            </div>
          </div>

          <div
            v-if="isRecognizingImage"
            class="viewer-image-scan-mask"
            aria-hidden="true"
          >
            <span />
          </div>

          <aside
            v-if="showImageOcrPanel"
            class="viewer-image-ocr-panel"
            :class="{ 'viewer-image-ocr-panel-collapsed': isImageOcrPanelCollapsed }"
            @wheel.stop
          >
            <button
              type="button"
              class="viewer-image-ocr-toggle"
              :aria-label="isImageOcrPanelCollapsed ? t('viewer.expandOcr') : t('viewer.collapseOcr')"
              :data-tooltip="isImageOcrPanelCollapsed ? t('viewer.expandOcr') : t('viewer.collapseOcr')"
              @pointerdown.stop
              @click="toggleImageOcrPanel"
            >
              <ChevronLeft
                v-if="isImageOcrPanelCollapsed"
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
                  <p v-if="imageOcrResult">
                    {{ imageOcrSummary }}
                  </p>
                  <p v-else-if="isRecognizingImage">
                    {{ t("viewer.ocrRecognizing") }}
                  </p>
                  <p v-else>
                    {{ t("viewer.ocrFailed") }}
                  </p>
                </div>
                <button
                  v-if="imageOcrResult?.text"
                  type="button"
                  class="viewer-paste-button"
                  @click="pasteImageOcrText"
                >
                  <Copy class="size-4" />
                  <span>{{ t("viewer.copyText") }}</span>
                </button>
              </div>

              <p
                v-if="imageOcrError"
                class="viewer-image-ocr-error"
              >
                {{ imageOcrError }}
              </p>
              <p
                v-else-if="isRecognizingImage"
                class="viewer-image-ocr-loading"
              >
                {{ imageOcrLoadingText }}
              </p>
              <textarea
                v-else-if="imageOcrResult"
                class="viewer-image-ocr-text subtle-scrollbar"
                :value="imageOcrText"
                readonly
                spellcheck="false"
                @focus="clearImageTextSelection"
                @pointerdown="clearImageTextSelection"
              />
            </div>
          </aside>
        </div>
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
