import { computed, ref, type ComputedRef, type Ref } from "vue";
import { t } from "../i18n";
import {
  loadOcrLanguage,
  normalizeOcrLanguage,
  ocrLanguageLabel,
  saveOcrLanguage,
  type OcrLanguageId,
} from "../lib/ocrLanguages";
import { ipasteApi } from "../lib/ipasteApi";
import { errorMessage } from "../lib/appError";
import { isMacOs } from "../lib/env";
import {
  buildOcrLineSeeds,
  compareOcrWordsInLine,
  joinOcrWords,
  type OcrSourceWord,
} from "../lib/ocr/textLayout";
import {
  clientPointToImagePoint as toImagePoint,
  unionRects,
  wordIndexFromPoint,
} from "../lib/ocr/hitTest";
import type { ClipViewItem, ImageOcrResult } from "../types";
import type { useImageViewer } from "./useImageViewer";
import type { ClipEditorHandle } from "./useClipEditor";

type OcrSelectableWord = OcrSourceWord & {
  selectionIndex: number;
  lineKey: string;
  lineOrder: number;
};

type OcrLine = {
  key: string;
  text: string;
  left: number;
  top: number;
  width: number;
  height: number;
  order: number;
  words: OcrSelectableWord[];
};

type OcrSelectionRange = {
  startIndex: number;
  endIndex: number;
};

type OcrSelectionHighlight = {
  key: string;
  left: number;
  top: number;
  width: number;
  height: number;
};

type ImageOcrOptions = {
  item: ComputedRef<ClipViewItem | undefined>;
  isImage: ComputedRef<boolean>;
  /** 编辑器句柄由调用方在 useClipEditor 创建后一次赋值，仅在交互期读取 */
  editor: Ref<ClipEditorHandle | null>;
};

export function useImageOcr(viewer: ReturnType<typeof useImageViewer>, options: ImageOcrOptions) {
  const isRecognizingImage = ref(false);
  const imageOcrResult = ref<ImageOcrResult | null>(null);
  const imageOcrError = ref<string | null>(null);
  const imageOcrSelection = ref<OcrSelectionRange | null>(null);
  const isImageOcrPanelCollapsed = ref(false);
  const selectedOcrLanguage = ref<OcrLanguageId>(loadOcrLanguage());
  let imageOcrDragState: {
    pointerId: number;
    startIndex: number;
    captureElement: HTMLElement;
  } | null = null;

  const showImageOcrPanel = computed(() => options.isImage.value && (isRecognizingImage.value || Boolean(imageOcrResult.value) || Boolean(imageOcrError.value)));
  const ocrTextLayerStyle = computed(() => {
    const { width, height } = viewer.imageNaturalSize.value;
    return {
      width: width ? `${width}px` : "0",
      height: height ? `${height}px` : "0",
      marginLeft: width ? `${-width / 2}px` : "0",
      marginTop: height ? `${-height / 2}px` : "0",
      transform: `rotate(${viewer.imageRotation.value}deg) scale(${viewer.imageScale.value})`,
    };
  });
  const imageOcrSummary = computed(() => {
    if (!imageOcrResult.value) return "";
    return t("viewer.ocrSummary", {
      count: imageOcrResult.value.words.length,
      language: ocrLanguageLabel(imageOcrResult.value.language),
    });
  });
  const imageOcrLoadingText = computed(() =>
    isMacOs ? t("viewer.ocrLoading.macos") : t("viewer.ocrLoading.engine"),
  );
  const imageOcrLines = computed<OcrLine[]>(() => {
    const words = imageOcrResult.value?.words ?? [];
    if (!words.length) return [];

    const sourceWords = words
      .map((word, sourceIndex) => ({ ...word, sourceIndex }))
      .filter((word) => word.text.trim() && word.width > 0 && word.height > 0);
    const seeds = buildOcrLineSeeds(sourceWords);
    let selectionIndex = 0;

    return seeds.map((line, order) => {
      const ordered = [...line.words]
        .sort(compareOcrWordsInLine)
        .map((word) => ({
          ...word,
          lineKey: line.key,
          lineOrder: order,
          selectionIndex: selectionIndex++,
        }));
      const left = Math.min(...ordered.map((word) => word.left));
      const top = Math.min(...ordered.map((word) => word.top));
      const right = Math.max(...ordered.map((word) => word.left + word.width));
      const bottom = Math.max(...ordered.map((word) => word.top + word.height));
      return {
        key: line.key,
        text: joinOcrWords(ordered),
        left,
        top,
        width: right - left,
        height: bottom - top,
        order,
        words: ordered,
      };
    });
  });
  const imageOcrWords = computed(() => imageOcrLines.value.flatMap((line) => line.words));
  const imageOcrSelectionBounds = computed(() => {
    const range = imageOcrSelection.value;
    if (!range) return null;
    return {
      start: Math.min(range.startIndex, range.endIndex),
      end: Math.max(range.startIndex, range.endIndex),
    };
  });
  const selectedImageOcrWordIndexes = computed(() => {
    const bounds = imageOcrSelectionBounds.value;
    if (!bounds) return new Set<number>();
    return new Set(
      imageOcrWords.value
        .filter((word) => word.selectionIndex >= bounds.start && word.selectionIndex <= bounds.end)
        .map((word) => word.selectionIndex),
    );
  });
  const imageOcrSelectionHighlights = computed<OcrSelectionHighlight[]>(() => {
    const selected = selectedImageOcrWordIndexes.value;
    if (!selected.size) return [];

    return imageOcrLines.value.flatMap((line) => {
      const selectedWords = line.words.filter((word) => selected.has(word.selectionIndex));
      if (!selectedWords.length) return [];

      const left = Math.min(...selectedWords.map((word) => word.left));
      const right = Math.max(...selectedWords.map((word) => word.left + word.width));
      const top = Math.min(...selectedWords.map((word) => word.top));
      const bottom = Math.max(...selectedWords.map((word) => word.top + word.height));
      return [{
        key: `${line.key}:${selectedWords[0].selectionIndex}:${selectedWords[selectedWords.length - 1].selectionIndex}`,
        left: Math.max(0, left - 2),
        top: Math.max(0, top - 2),
        width: right - left + 4,
        height: Math.max(1, bottom - top + 4),
      }];
    });
  });
  const imageOcrSelectionText = computed(() => {
    const selected = selectedImageOcrWordIndexes.value;
    if (!selected.size) return "";

    const lineTexts = imageOcrLines.value
      .map((line) => line.words.filter((word) => selected.has(word.selectionIndex)))
      .filter((lineWords) => lineWords.length)
      .map(joinOcrWords);
    return lineTexts.join("\n");
  });
  const imageOcrText = computed(() => {
    const lineText = imageOcrLines.value.map((line) => line.text).filter(Boolean).join("\n");
    return lineText || imageOcrResult.value?.text || "";
  });

  // —— 命中测试适配层：收集响应式输入后交给 lib/ocr/hitTest 的纯函数 ——

  function clientPointToImagePoint(clientX: number, clientY: number) {
    const stage = viewer.imageStageElement.value;
    const { width, height } = viewer.imageNaturalSize.value;
    if (!stage || !width || !height || viewer.imageScale.value <= 0) return null;

    return toImagePoint(clientX, clientY, {
      stageRect: stage.getBoundingClientRect(),
      naturalWidth: width,
      naturalHeight: height,
      scale: viewer.imageScale.value,
      pan: viewer.imagePan.value,
      normalizedRotation: viewer.normalizedImageRotation.value,
    });
  }

  function imageOcrWordIndexFromPoint(event: PointerEvent, allowNearest: boolean) {
    const point = clientPointToImagePoint(event.clientX, event.clientY);
    if (!point) return null;

    return wordIndexFromPoint(point, imageOcrWords.value, imageOcrLines.value, {
      scale: viewer.imageScale.value,
      allowNearest,
    });
  }

  function endImageOcrSelection() {
    imageOcrDragState = null;
  }

  function updateImageOcrSelectionAction() {
    const selectedText = imageOcrSelectionText.value;
    if (!selectedText.trim()) {
      options.editor.value?.hideSelectionAction();
      return;
    }

    const selectedIndexes = selectedImageOcrWordIndexes.value;
    const wordElements = viewer.imageStageElement.value?.querySelectorAll<HTMLElement>(".viewer-image-ocr-word") ?? [];
    const rects = [...wordElements]
      .filter((element) => selectedIndexes.has(Number(element.dataset.ocrWordIndex)))
      .map((element) => element.getBoundingClientRect())
      .filter((rect) => rect.width || rect.height);
    const rect = unionRects(rects);
    if (!rect) {
      options.editor.value?.hideSelectionAction();
      return;
    }

    const editor = options.editor.value;
    if (!editor) return;

    editor.selectionAction.value = {
      left: Math.min(window.innerWidth - 132, Math.max(16, rect.right - 112)),
      top: Math.min(window.innerHeight - 56, rect.bottom + 8),
      text: selectedText,
      mode: "copy",
    };
  }

  function startImageOcrSelection(event: PointerEvent, selectionIndex: number) {
    if (event.button !== 0) return;

    event.preventDefault();
    event.stopPropagation();
    viewer.endImageDrag();

    const captureElement = event.currentTarget as HTMLElement;
    imageOcrDragState = {
      pointerId: event.pointerId,
      startIndex: selectionIndex,
      captureElement,
    };
    imageOcrSelection.value = {
      startIndex: selectionIndex,
      endIndex: selectionIndex,
    };
    captureElement.setPointerCapture(event.pointerId);
    updateImageOcrSelectionAction();
  }

  function moveImageOcrSelection(event: PointerEvent) {
    if (!imageOcrDragState || event.pointerId !== imageOcrDragState.pointerId) return;

    event.preventDefault();
    event.stopPropagation();

    const selectionIndex = imageOcrWordIndexFromPoint(event, true);
    if (selectionIndex === null) return;
    imageOcrSelection.value = {
      startIndex: imageOcrDragState.startIndex,
      endIndex: selectionIndex,
    };
    updateImageOcrSelectionAction();
  }

  function finishImageOcrSelection(event: PointerEvent) {
    if (!imageOcrDragState || event.pointerId !== imageOcrDragState.pointerId) return;

    event.preventDefault();
    event.stopPropagation();

    const selectionIndex = imageOcrWordIndexFromPoint(event, true);
    if (selectionIndex !== null) {
      imageOcrSelection.value = {
        startIndex: imageOcrDragState.startIndex,
        endIndex: selectionIndex,
      };
    }

    if (imageOcrDragState.captureElement.hasPointerCapture(event.pointerId)) {
      imageOcrDragState.captureElement.releasePointerCapture(event.pointerId);
    }
    endImageOcrSelection();
    updateImageOcrSelectionAction();
  }

  function clearImageTextSelection() {
    if (!options.isImage.value) return;
    imageOcrSelection.value = null;
    endImageOcrSelection();
    options.editor.value?.hideSelectionAction();
  }

  function resetOcrState() {
    imageOcrResult.value = null;
    imageOcrError.value = null;
    isImageOcrPanelCollapsed.value = false;
    clearImageTextSelection();
  }

  async function recognizeImageText() {
    if (!options.item.value || !options.isImage.value || isRecognizingImage.value) return;

    isRecognizingImage.value = true;
    imageOcrError.value = null;
    isImageOcrPanelCollapsed.value = false;
    clearImageTextSelection();
    try {
      imageOcrResult.value = await ipasteApi.recognizeImageText(
        options.item.value.text,
        undefined,
        selectedOcrLanguage.value,
      );
    } catch (unknownError) {
      imageOcrError.value = errorMessage(unknownError);
    } finally {
      isRecognizingImage.value = false;
    }
    // 识别成功后默认把全文复制到剪贴板；复制失败不影响已展示的识别结果
    await pasteImageOcrText().catch(() => undefined);
  }

  async function changeOcrLanguage(language: string) {
    if (isRecognizingImage.value) return;
    const next = normalizeOcrLanguage(language);
    if (!next || next === selectedOcrLanguage.value) return;
    selectedOcrLanguage.value = next;
    saveOcrLanguage(next);
    await recognizeImageText();
  }

  async function pasteImageOcrText() {
    const text = imageOcrText.value;
    if (!text.trim()) return;
    await ipasteApi.copyClip("text", text);
  }

  function toggleImageOcrPanel() {
    isImageOcrPanelCollapsed.value = !isImageOcrPanelCollapsed.value;
  }

  return {
    isRecognizingImage,
    imageOcrResult,
    imageOcrError,
    isImageOcrPanelCollapsed,
    showImageOcrPanel,
    ocrTextLayerStyle,
    imageOcrSummary,
    imageOcrLoadingText,
    imageOcrLines,
    imageOcrWords,
    imageOcrSelectionHighlights,
    imageOcrSelectionText,
    imageOcrText,
    recognizeImageText,
    selectedOcrLanguage,
    changeOcrLanguage,
    pasteImageOcrText,
    toggleImageOcrPanel,
    startImageOcrSelection,
    moveImageOcrSelection,
    finishImageOcrSelection,
    clearImageTextSelection,
    resetOcrState,
    selectedImageOcrWordIndexes,
    endImageOcrSelection,
    updateImageOcrSelectionAction,
  };
}
