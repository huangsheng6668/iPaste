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
  type OcrLine,
} from "../lib/ocr/textLayout";
import { useOcrSelection } from "./useOcrSelection";
import {
  clientPointToImagePoint as toImagePoint,
  wordIndexFromPoint,
} from "../lib/ocr/hitTest";
import type { ClipViewItem, ImageOcrResult } from "../types";
import type { useImageViewer } from "./useImageViewer";
import type { ClipEditorHandle } from "./useClipEditor";

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
  const isImageOcrPanelCollapsed = ref(false);
  const selectedOcrLanguage = ref<OcrLanguageId>(loadOcrLanguage());

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

  // —— 选区状态机（实现在 useOcrSelection；此处只做依赖注入与命名保持）——

  const ocrSelection = useOcrSelection({
    lines: imageOcrLines,
    stageElement: viewer.imageStageElement,
    pointToIndex: imageOcrWordIndexFromPoint,
    onBeforeDragStart: () => viewer.endImageDrag(),
    setSelectionAction: (action) => {
      const editor = options.editor.value;
      if (!editor) return;
      editor.selectionAction.value = action;
    },
    hideSelectionAction: () => options.editor.value?.hideSelectionAction(),
  });

  const selectedImageOcrWordIndexes = ocrSelection.selectedWordIndexes;
  const imageOcrSelectionHighlights = ocrSelection.selectionHighlights;
  const imageOcrSelectionText = ocrSelection.selectionText;
  const startImageOcrSelection = ocrSelection.startSelection;
  const moveImageOcrSelection = ocrSelection.moveSelection;
  const finishImageOcrSelection = ocrSelection.finishSelection;
  const endImageOcrSelection = ocrSelection.endDrag;
  const updateImageOcrSelectionAction = ocrSelection.updateSelectionAction;

  /** 保持原守卫：非图片条目不清理选区。 */
  function clearImageTextSelection() {
    if (!options.isImage.value) return;
    ocrSelection.clearSelection();
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
