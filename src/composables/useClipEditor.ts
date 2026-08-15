import { computed, onMounted, onUnmounted, ref, watch, type ComputedRef, type Ref } from "vue";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { clipViewerStorageKey, ipasteApi } from "../lib/ipasteApi";
import { clipMetricText, textStats } from "../lib/format";
import { errorMessage } from "../lib/appError";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { ClipUpdatedEvent, ClipViewItem, ClipViewerPayload } from "../types";
import type { useImageOcr } from "./useImageOcr";

const isTauri = "__TAURI_INTERNALS__" in window;

type SelectionAction = {
  left: number;
  top: number;
  text: string;
  mode: "paste" | "copy";
};

type EditorOptions = {
  payload: Ref<ClipViewerPayload | null>;
  isPinned: Ref<boolean>;
  isImage: ComputedRef<boolean>;
  ocr: ReturnType<typeof useImageOcr>;
  error: Ref<string | null>;
};

export function useClipEditor(item: ComputedRef<ClipViewItem | undefined>, options: EditorOptions) {
  const draftText = ref("");
  const editorElement = ref<HTMLTextAreaElement | null>(null);
  const selectionAction = ref<SelectionAction | null>(null);
  let selectionTimer: number | null = null;

  const hasChanged = computed(() => Boolean(item.value && draftText.value !== item.value.text));
  const stats = computed(() => (item.value ? textStats(draftText.value) : ""));
  const metricText = computed(() => (item.value ? clipMetricText(item.value.clipType, draftText.value, item.value.previewText) : ""));
  const lines = computed(() => draftText.value.split(/\r?\n/).length);

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
      options.payload.value = {
        ...options.payload.value!,
        item: nextItem,
      };
      localStorage.setItem(clipViewerStorageKey(options.payload.value.label), JSON.stringify(options.payload.value));
      draftText.value = next.text;
      if (isTauri) {
        await emit<ClipUpdatedEvent>(IPASTE_EVENTS.clipUpdated, {
          collection: item.value.collection,
          item: next,
          mergedFromId: next.id === item.value.id ? undefined : item.value.id,
        });
      }
    } catch (unknownError) {
      options.error.value = errorMessage(unknownError);
    }
  }

  async function pasteDraft() {
    if (!options.payload.value || !item.value) return;
    await pasteFromViewer(draftText.value);
  }

  async function pasteSelection() {
    if (!options.payload.value || !item.value || !selectionAction.value?.text) return;
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
    if (!options.payload.value || !item.value) return;

    const viewerWindow = isTauri ? getCurrentWindow() : null;
    if (viewerWindow) {
      await viewerWindow.hide();
    }

    try {
      await ipasteApi.applyClip(options.payload.value.originalClipId, item.value.clipType, text);
    } finally {
      if (viewerWindow) {
        await viewerWindow.show();
        await viewerWindow.setAlwaysOnTop(options.isPinned.value);
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
    if (options.isImage.value && options.ocr.imageOcrSelectionText.value.trim()) {
      options.ocr.updateImageOcrSelectionAction();
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

  function focusEditorAtStart() {
    const editor = editorElement.value;
    if (!editor) return;

    editor.focus();
    editor.setSelectionRange(0, 0);
    editor.scrollTop = 0;
    editor.scrollLeft = 0;
  }

  watch(draftText, () => {
    hideSelectionAction();
  });

  onMounted(() => {
    document.addEventListener("selectionchange", scheduleSelectionAction);
  });

  onUnmounted(() => {
    document.removeEventListener("selectionchange", scheduleSelectionAction);
    clearSelectionTimer();
  });

  return {
    draftText,
    editorElement,
    selectionAction,
    hasChanged,
    stats,
    metricText,
    lines,
    resetDraft,
    applyChanges,
    pasteDraft,
    pasteSelection,
    pasteFromViewer,
    scheduleSelectionAction,
    updateSelectionAction,
    hideSelectionAction,
    selectionCoordinates,
    clearSelectionTimer,
    focusEditorAtStart,
  };
}
