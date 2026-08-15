import { contextItemKey } from "../lib/clipKeys";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { useAutomationFlow } from "./useAutomationFlow";
import type { useClipContextMenu } from "./useClipContextMenu";
import type { useQuickPreview } from "./useQuickPreview";

type IpasteStore = ReturnType<typeof useIpasteStore>;
type QuickPreviewApi = ReturnType<typeof useQuickPreview>;
type ClipMenuApi = ReturnType<typeof useClipContextMenu>;
type AutomationFlowApi = ReturnType<typeof useAutomationFlow>;

type PanelKeyboardDeps = {
  store: IpasteStore;
  quickPreview: QuickPreviewApi;
  clipMenu: ClipMenuApi;
  automationFlow: AutomationFlowApi;
  closeFloatingLayers: () => void;
  hidePanelFromUi: () => Promise<void> | void;
  finishEditingCategory: () => void;
};

/**
 * 面板级键盘路由（原 App.vue keyboard 段）：keydown 级联（defaultPrevented →
 * Backspace 待删复位 → 快速预览 → 分类快捷键 → Tab → 可编辑目标守卫 → 右键菜单
 * Escape → Backspace 两段删除 → 面板方向键 → Ctrl/Cmd+F 聚焦搜索）与 keyup
 * （委派快速预览修饰键复位）。store/各 composable 与浮层关闭经 deps 注入。
 */
export function usePanelKeyboard(deps: PanelKeyboardDeps) {
  const { store, quickPreview, clipMenu, automationFlow, closeFloatingLayers, hidePanelFromUi, finishEditingCategory } =
    deps;

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;

    if (event.key !== "Backspace") {
      clipMenu.pendingDeleteByKey.value = null;
    }

    if (quickPreview.handleQuickPreviewKeydown(event)) return;

    if (handleCategoryShortcut(event)) return;

    if (event.key === "Tab") {
      event.preventDefault();
      return;
    }

    if (quickPreview.isEditableTarget(event.target)) return;

    if (clipMenu.contextMenu.value) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeFloatingLayers();
      }
      return;
    }

    if (event.key === "Backspace") {
      event.preventDefault();
      const item = store.visibleItems[store.selectedIndex];
      if (!item) return;

      const key = contextItemKey(item);
      if (clipMenu.pendingDeleteByKey.value === key) {
        void clipMenu.deleteSelectedItem(item);
        return;
      }

      clipMenu.pendingDeleteByKey.value = key;
      return;
    }

    if (handlePanelKey(event.key)) {
      event.preventDefault();
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      focusSearch();
    }
  }

  function handleKeyup(event: KeyboardEvent) {
    quickPreview.handleQuickPreviewKeyup(event);
  }

  function handlePanelKey(key: string) {
    if (store.selectedCategoryId === "actions") {
      return automationFlow.handleActionsKey(key);
    }
    if (clipMenu.contextMenu.value) {
      if (key === "Escape") {
        closeFloatingLayers();
        return true;
      }
      return false;
    }

    if (key === "ArrowDown") {
      store.moveSelection(2);
      return true;
    }

    if (key === "ArrowUp") {
      store.moveSelection(-2);
      return true;
    }

    if (key === "ArrowRight") {
      store.moveSelection(1);
      return true;
    }

    if (key === "ArrowLeft") {
      store.moveSelection(-1);
      return true;
    }

    if (key === "Enter") {
      void store.applySelected();
      return true;
    }

    if (key === "Escape") {
      void hidePanelFromUi();
      return true;
    }

    return false;
  }

  function handleCategoryShortcut(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || event.altKey || !/^[1-9]$/.test(event.key)) {
      return false;
    }

    event.preventDefault();

    const categoryIds = ["history", ...store.categories.map((category) => category.id)];
    const targetCategoryId = categoryIds[Number(event.key) - 1];
    if (!targetCategoryId) return true;

    closeFloatingLayers();
    finishEditingCategory();
    if (targetCategoryId !== store.selectedCategoryId) {
      store.selectCategory(targetCategoryId);
    }
    return true;
  }

  function focusSearch() {
    const input = document.querySelector<HTMLInputElement>(".search-box input");
    input?.focus();
    input?.select();
  }

  return {
    handleKeydown,
    handleKeyup,
    handleCategoryShortcut,
    focusSearch,
  };
}
