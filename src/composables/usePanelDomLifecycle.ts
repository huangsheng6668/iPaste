import { onMounted, onUnmounted } from "vue";

export type PanelDomLifecycleHandlers = {
  onKeydown: (event: KeyboardEvent) => void;
  onKeyup: (event: KeyboardEvent) => void;
  onSelectionChange: () => void;
  /** 窗口失焦（关闭浮层）。 */
  onWindowBlur: () => void;
  onVisibilityChange: () => void;
};

type PanelDomLifecycleOptions = {
  /** 仅主面板注册全局监听；辅助窗口共用同一份 App.vue 代码，需显式关闭。 */
  enabled: boolean;
};

/**
 * 主面板的全局 DOM 监听生命周期（Task 23 从 App.vue 抽出）。
 * 成对注册/注销：keydown/keyup 走捕获阶段（面板内输入框也能拦到快捷键），
 * 外加选区变化、窗口失焦与可见性变化。其余面板级清理仍由 App.vue 负责。
 */
export function usePanelDomLifecycle(
  handlers: PanelDomLifecycleHandlers,
  options: PanelDomLifecycleOptions,
) {
  const { onKeydown, onKeyup, onSelectionChange, onWindowBlur, onVisibilityChange } = handlers;

  onMounted(() => {
    if (!options.enabled) return;

    document.addEventListener("keydown", onKeydown, true);
    document.addEventListener("keyup", onKeyup, true);
    document.addEventListener("selectionchange", onSelectionChange);
    window.addEventListener("blur", onWindowBlur);
    document.addEventListener("visibilitychange", onVisibilityChange);
  });

  onUnmounted(() => {
    if (!options.enabled) return;

    document.removeEventListener("keydown", onKeydown, true);
    document.removeEventListener("keyup", onKeyup, true);
    document.removeEventListener("selectionchange", onSelectionChange);
    window.removeEventListener("blur", onWindowBlur);
    document.removeEventListener("visibilitychange", onVisibilityChange);
  });
}
