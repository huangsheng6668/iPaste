<script setup lang="ts">
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
import { clipImageSrc } from "../lib/clipMedia";
import { t } from "../i18n";
import { clipViewerStorageKey, ipasteApi } from "../lib/ipasteApi";
import { clipMetricText, formatTime, textStats, typeLabel } from "../lib/format";
import type { ClipUpdatedEvent, ClipViewerPayload } from "../types";

const isTauri = "__TAURI_INTERNALS__" in window;
const payload = ref<ClipViewerPayload | null>(null);
const windowLabel = ref("");
const draftText = ref("");
const isPinned = ref(isTauri);
const error = ref<string | null>(null);
const selectionAction = ref<{ left: number; top: number; text: string; mode: "paste" | "copy" } | null>(null);
const editorElement = ref<HTMLTextAreaElement | null>(null);
const showClosePrompt = ref(false);
const isSavingBeforeClose = ref(false);
let selectionTimer: number | null = null;
let unlistenCloseRequested: (() => void) | null = null;
let isForceClosing = false;

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
const editorHandle = { hideSelectionAction: () => hideSelectionAction(), selectionAction };
const ocr = useImageOcr(viewer, { item, isImage, editor: editorHandle });
const {
  isRecognizingImage, imageOcrResult, imageOcrError, isImageOcrPanelCollapsed,
  showImageOcrPanel, ocrTextLayerStyle, imageOcrSummary, imageOcrLoadingText,
  imageOcrLines, imageOcrWords, selectedImageOcrWordIndexes, imageOcrSelectionHighlights, imageOcrSelectionText,
  imageOcrText, recognizeImageText, pasteImageOcrText, toggleImageOcrPanel,
  startImageOcrSelection, moveImageOcrSelection, finishImageOcrSelection,
  endImageOcrSelection, updateImageOcrSelectionAction,
  clearImageTextSelection, resetOcrState,
} = ocr;
const hasChanged = computed(() => Boolean(item.value && draftText.value !== item.value.text));
const stats = computed(() => (item.value ? textStats(draftText.value) : ""));
const metricText = computed(() => (item.value ? clipMetricText(item.value.clipType, draftText.value, item.value.previewText) : ""));
const lines = computed(() => draftText.value.split(/\r?\n/).length);
const displayTime = computed(() => {
  const current = item.value;
  if (!current) return "";
  return current.collection === "history" ? current.lastCapturedAt : current.createdAt;
});

onMounted(async () => {
  loadPayload();
  if (isTauri) {
    try {
      isPinned.value = await getCurrentWindow().isAlwaysOnTop();
    } catch {
      isPinned.value = true;
    }
  }
  document.addEventListener("selectionchange", scheduleSelectionAction);
  document.addEventListener("keydown", handleViewerKeydown, true);
  window.addEventListener("resize", handleViewerResize);
  window.addEventListener("beforeunload", handleBeforeUnload);
  if (isTauri) {
    unlistenCloseRequested = await getCurrentWindow().onCloseRequested(async (event) => {
      event.preventDefault();
      if (isForceClosing) return;
      if (!hasChanged.value) {
        await forceCloseWindow();
        return;
      }

      requestClose();
    });
  }
  void nextTick(focusEditorAtStart);
});

onUnmounted(() => {
  document.removeEventListener("selectionchange", scheduleSelectionAction);
  document.removeEventListener("keydown", handleViewerKeydown, true);
  window.removeEventListener("resize", handleViewerResize);
  window.removeEventListener("beforeunload", handleBeforeUnload);
  clearSelectionTimer();
  unlistenCloseRequested?.();
  unlistenCloseRequested = null;
});

watch(draftText, () => {
  hideSelectionAction();
});

watch(imageSrc, () => {
  resetImageViewState();
  resetOcrState();
});

function loadPayload() {
  const label = new URLSearchParams(window.location.search).get("label");
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
  }
}

function focusEditorAtStart() {
  const editor = editorElement.value;
  if (!editor) return;

  editor.focus();
  editor.setSelectionRange(0, 0);
  editor.scrollTop = 0;
  editor.scrollLeft = 0;
}

async function startWindowDrag(event: MouseEvent) {
  if (!isTauri || event.button !== 0) return;

  event.preventDefault();
  await getCurrentWindow().startDragging();
}

async function togglePinned() {
  isPinned.value = !isPinned.value;
  if (isTauri) {
    await getCurrentWindow().setAlwaysOnTop(isPinned.value);
  }
}

async function closeWindow() {
  if (hasChanged.value) {
    requestClose();
    return;
  }

  await forceCloseWindow();
}

function requestClose() {
  showClosePrompt.value = true;
  hideSelectionAction();
}

function cancelClose() {
  showClosePrompt.value = false;
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

async function forceCloseWindow() {
  isForceClosing = true;
  if (isTauri) {
    try {
      await ipasteApi.closeClipViewer(windowLabel.value || getCurrentWindow().label);
    } catch (unknownError) {
      isForceClosing = false;
      error.value = String(unknownError);
    }
    return;
  }

  window.close();
}

function handleBeforeUnload(event: BeforeUnloadEvent) {
  if (isForceClosing || !hasChanged.value) return;

  event.preventDefault();
  event.returnValue = "";
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

function resetDraft() {
  if (!item.value) return;
  draftText.value = item.value.text;
  hideSelectionAction();
}

async function applyChanges() {
  if (!item.value || !hasChanged.value) return;

  try {
    const next = await ipasteApi.updateClipContent(item.value.id, item.value.collection, draftText.value);
    const nextItem = { ...next, collection: item.value.collection } as typeof item.value;
    payload.value = {
      ...payload.value!,
      item: nextItem,
    };
    localStorage.setItem(clipViewerStorageKey(payload.value.label), JSON.stringify(payload.value));
    draftText.value = next.text;
    if (isTauri) {
      await emit<ClipUpdatedEvent>("ipaste://clip-updated", {
        collection: item.value.collection,
        item: next,
        mergedFromId: next.id === item.value.id ? undefined : item.value.id,
      });
    }
  } catch (unknownError) {
    error.value = String(unknownError);
  }
}

async function pasteDraft() {
  if (!payload.value || !item.value) return;
  await pasteFromViewer(draftText.value);
}

async function pasteSelection() {
  if (!payload.value || !item.value || !selectionAction.value?.text) return;
  const selectedText = selectionAction.value.text;
  const mode = selectionAction.value.mode;
  hideSelectionAction();
  if (mode === "copy") {
    await ipasteApi.copyClip("text", selectedText);
    return;
  }
  await pasteFromViewer(selectedText);
}

async function pasteFromViewer(text: string) {
  if (!payload.value || !item.value) return;

  const viewerWindow = isTauri ? getCurrentWindow() : null;
  if (viewerWindow) {
    await viewerWindow.hide();
  }

  try {
    await ipasteApi.applyClip(payload.value.originalClipId, item.value.clipType, text);
  } finally {
    if (viewerWindow) {
      await viewerWindow.show();
      await viewerWindow.setAlwaysOnTop(isPinned.value);
      await viewerWindow.setFocus();
    }
  }
}

function scheduleSelectionAction() {
  clearSelectionTimer();
  selectionTimer = window.setTimeout(updateSelectionAction, 80);
}

function updateSelectionAction() {
  selectionTimer = null;
  if (isImage.value && imageOcrSelectionText.value.trim()) {
    updateImageOcrSelectionAction();
    return;
  }

  const textarea = editorElement.value;
  if (!textarea || document.activeElement !== textarea) {
    hideSelectionAction();
    return;
  }

  const selectedText = draftText.value.slice(textarea.selectionStart, textarea.selectionEnd);
  if (!selectedText.trim()) {
    hideSelectionAction();
    return;
  }

  const coords = selectionCoordinates(textarea, textarea.selectionEnd);
  const fallbackRect = textarea.getBoundingClientRect();
  selectionAction.value = {
    left: Math.min(fallbackRect.right - 128, Math.max(fallbackRect.left + 16, coords.left - 48)),
    top: Math.min(window.innerHeight - 56, coords.top + coords.height + 8),
    text: selectedText,
    mode: "paste",
  };
}

function selectionCoordinates(textarea: HTMLTextAreaElement, position: number) {
  const rect = textarea.getBoundingClientRect();
  const style = window.getComputedStyle(textarea);
  const mirror = document.createElement("div");
  const marker = document.createElement("span");

  [
    "boxSizing",
    "borderTopWidth",
    "borderRightWidth",
    "borderBottomWidth",
    "borderLeftWidth",
    "fontFamily",
    "fontSize",
    "fontWeight",
    "letterSpacing",
    "lineHeight",
    "paddingTop",
    "paddingRight",
    "paddingBottom",
    "paddingLeft",
    "textTransform",
    "textIndent",
    "wordSpacing",
    "wordBreak",
  ].forEach((property) => {
    mirror.style.setProperty(property, style.getPropertyValue(property));
  });

  Object.assign(mirror.style, {
    position: "fixed",
    left: `${rect.left - textarea.scrollLeft}px`,
    top: `${rect.top - textarea.scrollTop}px`,
    width: `${textarea.offsetWidth}px`,
    height: "auto",
    minHeight: "0",
    overflow: "hidden",
    overflowWrap: "break-word",
    pointerEvents: "none",
    visibility: "hidden",
    whiteSpace: "pre-wrap",
    zIndex: "-1",
  });

  mirror.append(
    document.createTextNode(draftText.value.slice(0, position)),
    marker,
    document.createTextNode(draftText.value.slice(position) || "\u200b"),
  );
  marker.textContent = "\u200b";
  document.body.appendChild(mirror);
  const markerRect = marker.getBoundingClientRect();
  document.body.removeChild(mirror);

  return {
    left: markerRect.left,
    top: markerRect.top,
    height: markerRect.height || Number.parseFloat(style.lineHeight) || 22,
  };
}

function hideSelectionAction() {
  selectionAction.value = null;
}

function clearSelectionTimer() {
  if (selectionTimer === null) return;
  window.clearTimeout(selectionTimer);
  selectionTimer = null;
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
    <header class="clip-viewer-toolbar" :class="{ 'clip-viewer-toolbar-image': isImage }">
      <button
        type="button"
        class="viewer-icon-button"
        :class="{ 'viewer-icon-button-active': isPinned }"
        :aria-label="isPinned ? t('viewer.unpin') : t('viewer.pin')"
        :data-tooltip="isPinned ? t('viewer.unpin') : t('viewer.pin')"
        @click="togglePinned"
      >
        <PinOff v-if="isPinned" class="size-4" />
        <Pin v-else class="size-4" />
      </button>

      <div class="clip-viewer-drag-zone min-w-0 flex-1" @mousedown="startWindowDrag">
        <h1 class="truncate text-base font-semibold text-slate-950">{{ title }}</h1>
        <p v-if="item" class="truncate text-xs text-slate-500">
          {{ typeLabel(item.clipType) }} · {{ formatTime(displayTime) }}
        </p>
      </div>

      <div v-if="isImage" class="viewer-image-toolbox" role="toolbar" :aria-label="t('viewer.imageToolbar')" @pointerdown.stop @wheel.stop>
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
          <LoaderCircle v-if="isRecognizingImage" class="size-4 update-spin" />
          <ScanText v-else class="size-4" />
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

      <button type="button" class="viewer-icon-button" :aria-label="t('viewer.closeWindow')" :data-tooltip="t('viewer.closeWindow')" @click="closeWindow">
        <X class="size-4" />
      </button>
    </header>

    <div v-if="error" class="viewer-error">{{ error }}</div>

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
          <div class="viewer-image-frame" :style="imageFrameStyle">
            <img
              :src="imageSrc"
              :style="imageStyle"
              draggable="false"
              :alt="t('common.imagePreviewAlt')"
              @load="handleImageLoad"
            />
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

          <div v-if="isRecognizingImage" class="viewer-image-scan-mask" aria-hidden="true">
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
              <ChevronLeft v-if="isImageOcrPanelCollapsed" class="size-4" />
              <ChevronRight v-else class="size-4" />
            </button>

            <div class="viewer-image-ocr-panel-body" @pointerdown.stop @wheel.stop>
              <div class="viewer-image-ocr-heading">
                <div class="min-w-0">
                  <h2>{{ t("viewer.ocrTitle") }}</h2>
                  <p v-if="imageOcrResult">{{ imageOcrSummary }}</p>
                  <p v-else-if="isRecognizingImage">{{ t("viewer.ocrRecognizing") }}</p>
                  <p v-else>{{ t("viewer.ocrFailed") }}</p>
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

              <p v-if="imageOcrError" class="viewer-image-ocr-error">{{ imageOcrError }}</p>
              <p v-else-if="isRecognizingImage" class="viewer-image-ocr-loading">{{ imageOcrLoadingText }}</p>
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
        <Copy v-if="selectionAction.mode === 'copy'" class="size-3.5" />
        <ClipboardPaste v-else class="size-3.5" />
        <span>{{ selectionAction.mode === "copy" ? t("viewer.copySelection") : t("viewer.pasteSelection") }}</span>
      </button>
    </section>

    <footer v-if="item" class="clip-viewer-footer">
      <span>{{ isImage ? metricText : stats }}</span>
      <span v-if="!isImage">{{ t("common.lineCount", { count: lines }) }}</span>
      <button type="button" class="viewer-paste-button" @click="pasteDraft">
        <ImageIcon v-if="isImage" class="size-4" />
        <CornerDownLeft v-else class="size-4" />
        <span>{{ isImage ? t("viewer.pasteImage") : t("viewer.pasteCurrent") }}</span>
      </button>
    </footer>

    <div v-if="showClosePrompt" class="viewer-close-backdrop" @mousedown.self="cancelClose">
      <section class="viewer-close-dialog" role="alertdialog" aria-modal="true" aria-labelledby="viewer-close-title">
        <h2 id="viewer-close-title">{{ t("viewer.saveChangesTitle") }}</h2>
        <p>{{ t("viewer.saveChangesDescription") }}</p>
        <div class="viewer-close-actions">
          <button type="button" class="viewer-action-button" :disabled="isSavingBeforeClose" @click="cancelClose">
            <span>{{ t("common.cancel") }}</span>
          </button>
          <button type="button" class="viewer-action-button viewer-action-button-danger" :disabled="isSavingBeforeClose" @click="discardAndClose">
            <X class="size-4" />
            <span>{{ t("viewer.discard") }}</span>
          </button>
          <button type="button" class="viewer-action-button viewer-action-button-primary" :disabled="isSavingBeforeClose" @click="saveAndClose">
            <Save class="size-4" />
            <span>{{ isSavingBeforeClose ? t("common.saving") : t("viewer.saveAndClose") }}</span>
          </button>
        </div>
      </section>
    </div>
  </main>
</template>
