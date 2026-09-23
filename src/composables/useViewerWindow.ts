import { onMounted, onUnmounted, ref, type ComputedRef, type Ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t } from "../i18n";
import { clipViewerStorageKey, ipasteApi } from "../lib/ipasteApi";
import { errorMessage } from "../lib/appError";
import { isTauri } from "../lib/env";
import { useWindowDrag } from "./useWindowDrag";
import type { ClipViewerPayload } from "../types";

type EditorBridge = {
  hasChanged: ComputedRef<boolean>;
  applyChanges: () => Promise<void>;
  hideSelectionAction: () => void;
  draftText: Ref<string>;
};

type ViewerWindowOptions = {
  error: Ref<string | null>;
  /** 编辑器初始草稿来自 payload，读取在 loadPayload 中完成 */
  payload: Ref<ClipViewerPayload | null>;
  isImage: ComputedRef<boolean>;
  /** 主面板「识别文字」一键入口：payload 带 auto-recognize 时触发 */
  autoRecognize: () => void | Promise<void>;
};

export function useViewerWindow(editor: EditorBridge, options: ViewerWindowOptions) {
  const windowLabel = ref("");
  const isPinned = ref(isTauri);
  const showClosePrompt = ref(false);
  const isSavingBeforeClose = ref(false);
  let isForceClosing = false;
  let unlistenCloseRequested: (() => void) | null = null;

  // 无边框窗口标题区左键拖动（简单变体，与其它辅助窗口共用）
  const { startWindowDrag } = useWindowDrag();

  async function togglePinned() {
    isPinned.value = !isPinned.value;
    if (isTauri) {
      await getCurrentWindow().setAlwaysOnTop(isPinned.value);
    }
  }

  async function closeWindow() {
    if (editor.hasChanged.value) {
      requestClose();
      return;
    }

    await forceCloseWindow();
  }

  function requestClose() {
    showClosePrompt.value = true;
    editor.hideSelectionAction();
  }

  function cancelClose() {
    showClosePrompt.value = false;
  }

  async function forceCloseWindow() {
    isForceClosing = true;
    if (isTauri) {
      try {
        await ipasteApi.closeClipViewer(windowLabel.value || getCurrentWindow().label);
      } catch (unknownError) {
        isForceClosing = false;
        options.error.value = errorMessage(unknownError);
      }
      return;
    }

    window.close();
  }

  async function saveAndClose() {
    if (!editor.hasChanged.value) {
      await forceCloseWindow();
      return;
    }

    isSavingBeforeClose.value = true;
    try {
      await editor.applyChanges();
    } finally {
      isSavingBeforeClose.value = false;
    }

    if (!editor.hasChanged.value) {
      showClosePrompt.value = false;
      await forceCloseWindow();
    }
  }

  async function discardAndClose() {
    showClosePrompt.value = false;
    await forceCloseWindow();
  }

  function loadPayload() {
    const params = new URLSearchParams(window.location.search);
    const label = params.get("label");
    if (!label) {
      options.error.value = t("viewer.payloadMissing");
      return;
    }
    windowLabel.value = label;

    const raw = localStorage.getItem(clipViewerStorageKey(label));
    if (!raw) {
      options.error.value = t("viewer.payloadExpired");
      return;
    }

    try {
      const next = JSON.parse(raw) as ClipViewerPayload;
      options.payload.value = next;
      editor.draftText.value = next.item.text;
    } catch {
      options.error.value = t("viewer.payloadInvalid");
      return;
    }

    // 主面板「识别文字」一键入口：打开即自动识别
    if (params.get("auto-recognize") === "1" && options.isImage.value) {
      void options.autoRecognize();
    }
  }

  function handleBeforeUnload(event: BeforeUnloadEvent) {
    if (isForceClosing || !editor.hasChanged.value) return;

    event.preventDefault();
    event.returnValue = "";
  }

  onMounted(async () => {
    if (isTauri) {
      try {
        isPinned.value = await getCurrentWindow().isAlwaysOnTop();
      } catch {
        isPinned.value = true;
      }
    }
    window.addEventListener("beforeunload", handleBeforeUnload);
    if (isTauri) {
      unlistenCloseRequested = await getCurrentWindow().onCloseRequested(async (event) => {
        event.preventDefault();
        if (isForceClosing) return;
        if (!editor.hasChanged.value) {
          await forceCloseWindow();
          return;
        }

        requestClose();
      });
    }
  });

  onUnmounted(() => {
    window.removeEventListener("beforeunload", handleBeforeUnload);
    unlistenCloseRequested?.();
    unlistenCloseRequested = null;
  });

  return {
    windowLabel,
    isPinned,
    showClosePrompt,
    isSavingBeforeClose,
    startWindowDrag,
    togglePinned,
    closeWindow,
    requestClose,
    cancelClose,
    forceCloseWindow,
    saveAndClose,
    discardAndClose,
    loadPayload,
    handleBeforeUnload,
  };
}
