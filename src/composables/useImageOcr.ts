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
import type { ClipViewItem, ImageOcrResult, ImageOcrWord } from "../types";
import type { useImageViewer } from "./useImageViewer";

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);

type OcrSourceWord = ImageOcrWord & {
  sourceIndex: number;
};

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

type SelectionAction = {
  left: number;
  top: number;
  text: string;
  mode: "paste" | "copy";
};

type EditorHandle = {
  hideSelectionAction: () => void;
  selectionAction: Ref<SelectionAction | null>;
};

type ImageOcrOptions = {
  item: ComputedRef<ClipViewItem | undefined>;
  isImage: ComputedRef<boolean>;
  editor: EditorHandle;
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

  function buildOcrLineSeeds(words: OcrSourceWord[]) {
    if (!words.length) return [];

    const hasStructuredLines = words.every((word) => (
      Number.isFinite(word.blockIndex)
      && Number.isFinite(word.paragraphIndex)
      && Number.isFinite(word.lineIndex)
    ));

    if (hasStructuredLines) {
      const groups = new Map<string, {
        key: string;
        words: OcrSourceWord[];
        blockIndex: number;
        paragraphIndex: number;
        lineIndex: number;
        firstSourceIndex: number;
        top: number;
        left: number;
      }>();

      for (const word of words) {
        const blockIndex = word.blockIndex ?? 0;
        const paragraphIndex = word.paragraphIndex ?? 0;
        const lineIndex = word.lineIndex ?? 0;
        const key = `${blockIndex}:${paragraphIndex}:${lineIndex}`;
        const group = groups.get(key);
        if (group) {
          group.words.push(word);
          group.firstSourceIndex = Math.min(group.firstSourceIndex, word.sourceIndex);
          group.top = Math.min(group.top, word.top);
          group.left = Math.min(group.left, word.left);
        } else {
          groups.set(key, {
            key,
            words: [word],
            blockIndex,
            paragraphIndex,
            lineIndex,
            firstSourceIndex: word.sourceIndex,
            top: word.top,
            left: word.left,
          });
        }
      }

      return [...groups.values()].sort((a, b) => (
        a.blockIndex - b.blockIndex
        || a.paragraphIndex - b.paragraphIndex
        || a.lineIndex - b.lineIndex
        || a.firstSourceIndex - b.firstSourceIndex
        || a.top - b.top
        || a.left - b.left
      ));
    }

    const sorted = [...words].sort((a, b) => (a.top - b.top) || (a.left - b.left) || (a.sourceIndex - b.sourceIndex));
    const lines: Array<{
      key: string;
      words: OcrSourceWord[];
      top: number;
      bottom: number;
      left: number;
    }> = [];

    for (const word of sorted) {
      const centerY = word.top + word.height / 2;
      const bestLine = lines
        .map((line) => ({
          line,
          distance: centerY < line.top ? line.top - centerY : Math.max(0, centerY - line.bottom),
        }))
        .sort((a, b) => a.distance - b.distance)[0];
      const tolerance = Math.max(4, word.height * 0.55);

      if (bestLine && bestLine.distance <= tolerance) {
        bestLine.line.words.push(word);
        bestLine.line.top = Math.min(bestLine.line.top, word.top);
        bestLine.line.bottom = Math.max(bestLine.line.bottom, word.top + word.height);
        bestLine.line.left = Math.min(bestLine.line.left, word.left);
      } else {
        lines.push({
          key: `geometry:${lines.length}`,
          words: [word],
          top: word.top,
          bottom: word.top + word.height,
          left: word.left,
        });
      }
    }

    return lines.sort((a, b) => (a.top - b.top) || (a.left - b.left));
  }

  function compareOcrWordsInLine(a: OcrSourceWord, b: OcrSourceWord) {
    if (Number.isFinite(a.wordIndex) && Number.isFinite(b.wordIndex) && a.wordIndex !== b.wordIndex) {
      return (a.wordIndex ?? 0) - (b.wordIndex ?? 0);
    }
    return (a.left - b.left) || (a.sourceIndex - b.sourceIndex);
  }

  function joinOcrWords(words: Array<Pick<OcrSelectableWord, "text">>) {
    return words.reduce((result, word) => {
      const text = word.text.trim();
      if (!text) return result;
      if (!result) return text;
      const previous = result[result.length - 1] ?? "";
      const separator = shouldInsertOcrSpace(previous, text[0]) ? " " : "";
      return `${result}${separator}${text}`;
    }, "");
  }

  function shouldInsertOcrSpace(previous: string, next: string) {
    if (!previous || !next) return false;
    const cjkPattern = /[\u3040-\u30ff\u3400-\u9fff\uf900-\ufaff]/u;
    if (cjkPattern.test(previous) || cjkPattern.test(next)) return false;
    if (/^[,.;:!?%)}\]，。；：！？、）】》]/u.test(next)) return false;
    if (/[([{$（【《]$/u.test(previous)) return false;
    return /[A-Za-z0-9)\]}]$/u.test(previous) && /^[A-Za-z0-9({[]/u.test(next);
  }

  function unionDomRects(rects: DOMRect[]) {
    if (!rects.length) return null;

    const left = Math.min(...rects.map((rect) => rect.left));
    const top = Math.min(...rects.map((rect) => rect.top));
    const right = Math.max(...rects.map((rect) => rect.right));
    const bottom = Math.max(...rects.map((rect) => rect.bottom));
    return { left, top, right, bottom };
  }

  function nearestImageOcrWordIndex(x: number, y: number) {
    const lines = imageOcrLines.value;
    if (!lines.length) return null;

    if (y <= lines[0].top) return lines[0].words[0]?.selectionIndex ?? null;

    const lastLine = lines[lines.length - 1];
    if (lastLine && y >= lastLine.top + lastLine.height) {
      return lastLine.words[lastLine.words.length - 1]?.selectionIndex ?? null;
    }

    const line = lines
      .map((entry) => ({
        line: entry,
        distance: y < entry.top ? entry.top - y : Math.max(0, y - entry.top - entry.height),
      }))
      .sort((a, b) => a.distance - b.distance)[0]?.line;
    if (!line) return null;

    const words = line.words;
    if (!words.length) return null;
    const firstWord = words[0];
    const lastWord = words[words.length - 1];
    if (x <= firstWord.left + firstWord.width / 2) return firstWord.selectionIndex;
    if (x >= lastWord.left + lastWord.width / 2) return lastWord.selectionIndex;

    return words
      .map((word) => ({
        word,
        distance: Math.abs(x - (word.left + word.width / 2)),
      }))
      .sort((a, b) => a.distance - b.distance)[0]?.word.selectionIndex ?? null;
  }

  function clientPointToImagePoint(clientX: number, clientY: number) {
    const stage = viewer.imageStageElement.value;
    const { width, height } = viewer.imageNaturalSize.value;
    if (!stage || !width || !height || viewer.imageScale.value <= 0) return null;

    const rect = stage.getBoundingClientRect();
    const centeredX = clientX - rect.left - rect.width / 2 - viewer.imagePan.value.x;
    const centeredY = clientY - rect.top - rect.height / 2 - viewer.imagePan.value.y;
    const radians = -viewer.normalizedImageRotation.value * Math.PI / 180;
    const rotatedX = centeredX * Math.cos(radians) - centeredY * Math.sin(radians);
    const rotatedY = centeredX * Math.sin(radians) + centeredY * Math.cos(radians);

    return {
      x: rotatedX / viewer.imageScale.value + width / 2,
      y: rotatedY / viewer.imageScale.value + height / 2,
    };
  }

  function imageOcrWordIndexFromPoint(event: PointerEvent, allowNearest: boolean) {
    const point = clientPointToImagePoint(event.clientX, event.clientY);
    if (!point) return null;

    const scale = Math.max(0.001, viewer.imageScale.value);
    const tolerance = Math.max(2, Math.min(18, 6 / scale));
    const exactWord = imageOcrWords.value.find((word) => (
      point.x >= word.left - tolerance
      && point.x <= word.left + word.width + tolerance
      && point.y >= word.top - tolerance
      && point.y <= word.top + word.height + tolerance
    ));
    if (exactWord) return exactWord.selectionIndex;
    if (!allowNearest) return null;

    return nearestImageOcrWordIndex(point.x, point.y);
  }

  function endImageOcrSelection() {
    imageOcrDragState = null;
  }

  function updateImageOcrSelectionAction() {
    const selectedText = imageOcrSelectionText.value;
    if (!selectedText.trim()) {
      options.editor.hideSelectionAction();
      return;
    }

    const selectedIndexes = selectedImageOcrWordIndexes.value;
    const wordElements = viewer.imageStageElement.value?.querySelectorAll<HTMLElement>(".viewer-image-ocr-word") ?? [];
    const rects = [...wordElements]
      .filter((element) => selectedIndexes.has(Number(element.dataset.ocrWordIndex)))
      .map((element) => element.getBoundingClientRect())
      .filter((rect) => rect.width || rect.height);
    const rect = unionDomRects(rects);
    if (!rect) {
      options.editor.hideSelectionAction();
      return;
    }

    options.editor.selectionAction.value = {
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
    options.editor.hideSelectionAction();
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
