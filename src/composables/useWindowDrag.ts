import { getCurrentWindow } from "@tauri-apps/api/window";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";

interface WindowDragOptions {
  /** 主面板拖动：通知 Rust 抑制面板交互，优先走原生 NSPanel 拖动，回落 getCurrentWindow。 */
  mainWindow?: boolean;
}

/** 无边框窗口标题区左键拖动。主面板变体保持 900ms 拖拽态释放定时器语义。 */
export function useWindowDrag(options: WindowDragOptions = {}) {
  let releaseTimer: number | null = null;

  function clearReleaseTimer() {
    if (releaseTimer !== null) {
      window.clearTimeout(releaseTimer);
      releaseTimer = null;
    }
  }

  async function startWindowDrag(event: MouseEvent) {
    if (!isTauri || event.button !== 0) return;
    event.preventDefault();
    if (!options.mainWindow) {
      await getCurrentWindow().startDragging().catch(() => {});
      return;
    }
    clearReleaseTimer();
    void ipasteApi.setMainWindowDragging(true).catch(() => {});
    try {
      const nativeDragStarted = await ipasteApi.startMainWindowDrag().catch(() => false);
      if (!nativeDragStarted) {
        await getCurrentWindow().startDragging().catch(() => {});
      }
    } finally {
      releaseTimer = window.setTimeout(() => {
        void ipasteApi.setMainWindowDragging(false).catch(() => {});
        releaseTimer = null;
      }, 900);
    }
  }

  return { startWindowDrag };
}
