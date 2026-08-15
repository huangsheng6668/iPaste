import { computed, ref } from "vue";
import { t } from "../i18n";
import { contextItemKey, originalClipId } from "../lib/clipKeys";
import { clipImageSrc } from "../lib/clipMedia";
import { clipMetricText, typeLabel } from "../lib/format";
import { ipasteApi } from "../lib/ipasteApi";
import type { ClipViewItem } from "../types";

type QuickPreviewOptions = {
  visibleItems: () => ClipViewItem[];
  isMenuOpen: () => boolean;
  isEditing: () => boolean;
  isMacOs: boolean;
};

/** 悬停预览（按住修饰键 140ms 弹出、可固定、可框选粘贴）的状态与交互逻辑。 */
export function useQuickPreview(options: QuickPreviewOptions) {
  const hoveredPreviewItemKey = ref<string | null>(null);
  const lockedPreviewItemKey = ref<string | null>(null);
  const isQuickPreviewPinned = ref(false);
  const isQuickPreviewKeyDown = ref(false);
  const isQuickPreviewActive = ref(false);
  const quickPreviewSelectedText = ref("");
  let quickPreviewOpenTimer: number | null = null;
  let suppressQuickPreviewUntilModifierUp = false;

  const quickPreviewItem = computed(() => {
    if (!isQuickPreviewActive.value || options.isMenuOpen() || options.isEditing()) return null;

    const itemKey = lockedPreviewItemKey.value ?? hoveredPreviewItemKey.value;
    if (!itemKey) return null;
    return options.visibleItems().find((item) => contextItemKey(item) === itemKey) ?? null;
  });
  const isQuickPreviewLocked = computed(() => isQuickPreviewPinned.value);
  const quickPreviewTitle = computed(() => {
    const item = quickPreviewItem.value;
    if (!item) return "";
    return item.displayName?.trim() || "";
  });
  const quickPreviewAriaLabel = computed(() => {
    const item = quickPreviewItem.value;
    if (!item) return "";
    return item.displayName?.trim() || t("clip.clipboardTitle", { type: typeLabel(item.clipType) });
  });
  const quickPreviewContent = computed(() => quickPreviewItem.value?.text || quickPreviewItem.value?.previewText || "");
  const quickPreviewImageSrc = computed(() => quickPreviewItem.value ? clipImageSrc(quickPreviewItem.value) : "");
  const quickPreviewTime = computed(() => {
    const item = quickPreviewItem.value;
    if (!item) return "";
    return item.collection === "history" ? item.lastCapturedAt : item.createdAt;
  });
  const quickPreviewSize = computed(() => {
    const item = quickPreviewItem.value;
    if (!item) return "";
    return clipMetricText(item.clipType, item.text, item.previewText);
  });
  const quickPreviewColorValue = computed(() => quickPreviewContent.value.trim());

  function isEditableTarget(target: EventTarget | null) {
    if (!(target instanceof HTMLElement)) return false;
    return Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
  }

  function hoverPreviewItem(item: ClipViewItem) {
    if (isQuickPreviewActive.value) return;

    hoveredPreviewItemKey.value = contextItemKey(item);
    if (isQuickPreviewKeyDown.value && !suppressQuickPreviewUntilModifierUp) {
      scheduleQuickPreview();
    }
  }

  function clearHoveredPreviewItem(item: ClipViewItem) {
    if (isQuickPreviewKeyDown.value) return;

    if (hoveredPreviewItemKey.value === contextItemKey(item)) {
      hoveredPreviewItemKey.value = null;
    }
    stopQuickPreview();
  }

  function clearQuickPreviewHover() {
    if (isQuickPreviewActive.value) return;

    hoveredPreviewItemKey.value = null;
    stopQuickPreview();
  }

  function scheduleQuickPreview() {
    if (!hoveredPreviewItemKey.value || options.isMenuOpen() || isEditableTarget(document.activeElement)) return;

    clearQuickPreviewTimer();
    const previewItemKey = hoveredPreviewItemKey.value;
    quickPreviewOpenTimer = window.setTimeout(() => {
      quickPreviewOpenTimer = null;
      if (isQuickPreviewKeyDown.value && hoveredPreviewItemKey.value && !suppressQuickPreviewUntilModifierUp) {
        lockedPreviewItemKey.value = previewItemKey;
        isQuickPreviewActive.value = true;
      }
    }, 140);
  }

  function stopQuickPreview(stopOptions: { force?: boolean } = {}) {
    clearQuickPreviewTimer();
    if (isQuickPreviewPinned.value && !stopOptions.force) return;

    lockedPreviewItemKey.value = null;
    isQuickPreviewPinned.value = false;
    isQuickPreviewActive.value = false;
  }

  function lockQuickPreview() {
    const item = quickPreviewItem.value;
    if (!item) return;
    lockedPreviewItemKey.value = contextItemKey(item);
    isQuickPreviewPinned.value = true;
    isQuickPreviewActive.value = true;
  }

  function closeQuickPreview() {
    lockedPreviewItemKey.value = null;
    isQuickPreviewPinned.value = false;
    isQuickPreviewKeyDown.value = false;
    suppressQuickPreviewUntilModifierUp = false;
    quickPreviewSelectedText.value = "";
    window.getSelection()?.removeAllRanges();
    stopQuickPreview({ force: true });
  }

  async function copyQuickPreviewItem() {
    const item = quickPreviewItem.value;
    if (!item) return;
    await ipasteApi.copyClip(item.clipType, item.text);
  }

  async function pasteQuickPreviewSelection() {
    const item = quickPreviewItem.value;
    const selectedText = quickPreviewSelectedText.value.trim();
    if (!item || !selectedText) return;

    await ipasteApi.applyClip(originalClipId(item), item.clipType, selectedText);
    closeQuickPreview();
  }

  function handleSelectionChange() {
    if (!quickPreviewItem.value) {
      quickPreviewSelectedText.value = "";
      return;
    }

    const selection = window.getSelection();
    const text = selection?.toString() ?? "";
    const anchorNode = selection?.anchorNode;
    const focusNode = selection?.focusNode;
    const previewElement = document.querySelector(".quick-preview-overlay");
    const selectionInPreview = Boolean(
      previewElement
        && anchorNode
        && focusNode
        && previewElement.contains(anchorNode)
        && previewElement.contains(focusNode),
    );

    quickPreviewSelectedText.value = selectionInPreview ? text : "";
  }

  function clearQuickPreviewTimer() {
    if (quickPreviewOpenTimer === null) return;
    window.clearTimeout(quickPreviewOpenTimer);
    quickPreviewOpenTimer = null;
  }

  function isQuickPreviewModifierKey(event: KeyboardEvent) {
    return options.isMacOs ? event.key === "Meta" || event.key === "Command" : event.key === "Control";
  }

  function hasQuickPreviewModifier(event: KeyboardEvent) {
    return options.isMacOs ? event.metaKey : event.ctrlKey;
  }

  function handleQuickPreviewKeydown(event: KeyboardEvent): boolean {
    if (quickPreviewItem.value) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeQuickPreview();
      }
      if (event.key === "Enter" && quickPreviewSelectedText.value.trim()) {
        event.preventDefault();
        void pasteQuickPreviewSelection();
      }
      return true;
    }

    if (isQuickPreviewModifierKey(event)) {
      isQuickPreviewKeyDown.value = true;
      suppressQuickPreviewUntilModifierUp = false;
      scheduleQuickPreview();
    } else if (hasQuickPreviewModifier(event)) {
      suppressQuickPreviewUntilModifierUp = true;
      stopQuickPreview();
    }
    return false;
  }

  function handleQuickPreviewKeyup(event: KeyboardEvent) {
    if (!isQuickPreviewModifierKey(event)) return;

    isQuickPreviewKeyDown.value = false;
    suppressQuickPreviewUntilModifierUp = false;
    stopQuickPreview();
  }

  function resetQuickPreviewState() {
    hoveredPreviewItemKey.value = null;
    isQuickPreviewKeyDown.value = false;
    suppressQuickPreviewUntilModifierUp = false;
    stopQuickPreview({ force: true });
  }

  return {
    quickPreviewItem,
    isQuickPreviewLocked,
    isQuickPreviewActive,
    quickPreviewSelectedText,
    hoveredPreviewItemKey,
    quickPreviewTitle,
    quickPreviewAriaLabel,
    quickPreviewContent,
    quickPreviewImageSrc,
    quickPreviewTime,
    quickPreviewSize,
    quickPreviewColorValue,
    isEditableTarget,
    hoverPreviewItem,
    clearHoveredPreviewItem,
    clearQuickPreviewHover,
    scheduleQuickPreview,
    stopQuickPreview,
    lockQuickPreview,
    closeQuickPreview,
    copyQuickPreviewItem,
    pasteQuickPreviewSelection,
    handleSelectionChange,
    isQuickPreviewModifierKey,
    hasQuickPreviewModifier,
    handleQuickPreviewKeydown,
    handleQuickPreviewKeyup,
    clearQuickPreviewTimer,
    resetQuickPreviewState,
  };
}
