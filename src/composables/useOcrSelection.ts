import { computed, ref, type ComputedRef, type Ref } from "vue";
import { joinOcrWords, type OcrLine } from "../lib/ocr/textLayout";
import { unionRects } from "../lib/ocr/hitTest";

export type OcrSelectionRange = { startIndex: number; endIndex: number };

export type OcrSelectionHighlight = {
  key: string;
  left: number;
  top: number;
  width: number;
  height: number;
};

export type OcrSelectionAction = {
  left: number;
  top: number;
  text: string;
  mode: "copy";
};

type OcrSelectionDeps = {
  /** 文本层行（已排序、已编号）。 */
  lines: ComputedRef<OcrLine[]>;
  /** 词元素容器：查询 .viewer-image-ocr-word 的 getBoundingClientRect。 */
  stageElement: Ref<HTMLElement | null>;
  /** 客户坐标 → 词下标（宿主注入命中测试适配器）。 */
  pointToIndex: (event: PointerEvent, allowNearest: boolean) => number | null;
  /** 拖选开始前的宿主清理（如图片平移拖拽收尾）。 */
  onBeforeDragStart?: () => void;
  /** 浮层操作条落点（宿主负责写入编辑器句柄）。 */
  setSelectionAction: (action: OcrSelectionAction) => void;
  hideSelectionAction: () => void;
};

/**
 * OCR 文本层的指针拖选状态机（Task 20 从 useImageOcr 搬出）。
 * 只负责选区范围、派生高亮/文本与操作条落点；命中测试、编辑器句柄、
 * 文本层行全部由宿主注入，因此不依赖 DOM 结构细节与响应式图其余部分。
 */
export function useOcrSelection(deps: OcrSelectionDeps) {
  const selection = ref<OcrSelectionRange | null>(null);
  let dragState: {
    pointerId: number;
    startIndex: number;
    captureElement: HTMLElement;
  } | null = null;

  const words = computed(() => deps.lines.value.flatMap((line) => line.words));

  const selectionBounds = computed(() => {
    const range = selection.value;
    if (!range) return null;
    return {
      start: Math.min(range.startIndex, range.endIndex),
      end: Math.max(range.startIndex, range.endIndex),
    };
  });

  const selectedWordIndexes = computed(() => {
    const bounds = selectionBounds.value;
    if (!bounds) return new Set<number>();
    return new Set(
      words.value
        .filter((word) => word.selectionIndex >= bounds.start && word.selectionIndex <= bounds.end)
        .map((word) => word.selectionIndex),
    );
  });

  const selectionHighlights = computed<OcrSelectionHighlight[]>(() => {
    const selected = selectedWordIndexes.value;
    if (!selected.size) return [];

    return deps.lines.value.flatMap((line) => {
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

  const selectionText = computed(() => {
    const selected = selectedWordIndexes.value;
    if (!selected.size) return "";

    const lineTexts = deps.lines.value
      .map((line) => line.words.filter((word) => selected.has(word.selectionIndex)))
      .filter((lineWords) => lineWords.length)
      .map(joinOcrWords);
    return lineTexts.join("\n");
  });

  function endDrag() {
    dragState = null;
  }

  /** 清空选区并收起操作条（不做 isImage 判定，由宿主包装保留原守卫）。 */
  function clearSelection() {
    selection.value = null;
    endDrag();
    deps.hideSelectionAction();
  }

  function updateSelectionAction() {
    const selectedText = selectionText.value;
    if (!selectedText.trim()) {
      deps.hideSelectionAction();
      return;
    }

    const selectedIndexes = selectedWordIndexes.value;
    const wordElements = deps.stageElement.value?.querySelectorAll<HTMLElement>(".viewer-image-ocr-word") ?? [];
    const rects = [...wordElements]
      .filter((element) => selectedIndexes.has(Number(element.dataset.ocrWordIndex)))
      .map((element) => element.getBoundingClientRect())
      .filter((rect) => rect.width || rect.height);
    const rect = unionRects(rects);
    if (!rect) {
      deps.hideSelectionAction();
      return;
    }

    deps.setSelectionAction({
      left: Math.min(window.innerWidth - 132, Math.max(16, rect.right - 112)),
      top: Math.min(window.innerHeight - 56, rect.bottom + 8),
      text: selectedText,
      mode: "copy",
    });
  }

  function startSelection(event: PointerEvent, selectionIndex: number) {
    if (event.button !== 0) return;

    event.preventDefault();
    event.stopPropagation();
    deps.onBeforeDragStart?.();

    const captureElement = event.currentTarget as HTMLElement;
    dragState = {
      pointerId: event.pointerId,
      startIndex: selectionIndex,
      captureElement,
    };
    selection.value = {
      startIndex: selectionIndex,
      endIndex: selectionIndex,
    };
    captureElement.setPointerCapture(event.pointerId);
    updateSelectionAction();
  }

  function moveSelection(event: PointerEvent) {
    if (!dragState || event.pointerId !== dragState.pointerId) return;

    event.preventDefault();
    event.stopPropagation();

    const selectionIndex = deps.pointToIndex(event, true);
    if (selectionIndex === null) return;
    selection.value = {
      startIndex: dragState.startIndex,
      endIndex: selectionIndex,
    };
    updateSelectionAction();
  }

  function finishSelection(event: PointerEvent) {
    if (!dragState || event.pointerId !== dragState.pointerId) return;

    event.preventDefault();
    event.stopPropagation();

    const selectionIndex = deps.pointToIndex(event, true);
    if (selectionIndex !== null) {
      selection.value = {
        startIndex: dragState.startIndex,
        endIndex: selectionIndex,
      };
    }

    if (dragState.captureElement.hasPointerCapture(event.pointerId)) {
      dragState.captureElement.releasePointerCapture(event.pointerId);
    }
    endDrag();
    updateSelectionAction();
  }

  return {
    selection,
    selectionHighlights,
    selectionText,
    selectedWordIndexes,
    endDrag,
    clearSelection,
    updateSelectionAction,
    startSelection,
    moveSelection,
    finishSelection,
  };
}
