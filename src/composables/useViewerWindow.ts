import { onMounted, onUnmounted, ref, type ComputedRef, type Ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ipasteApi } from "../lib/ipasteApi";

const isTauri = "__TAURI_INTERNALS__" in window;

type WindowOptions = {
  error: Ref<string | null>;
  hideSelectionAction: () => void;
};

export function useViewerWindow(hasChanged: ComputedRef<boolean>, options: WindowOptions) {
  const windowLabel = ref("");
  const isPinned = ref(isTauri);
  const showClosePrompt = ref(false);
  const isSavingBeforeClose = ref(false);
  let isForceClosing = false;
  let unlistenCloseRequested: (() => void) | null = null;

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
    options.hideSelectionAction();
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
        options.error.value = String(unknownError);
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
        if (!hasChanged.value) {
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
    handleBeforeUnload,
  };
}
