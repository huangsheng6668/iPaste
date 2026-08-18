import { contextItemKey } from "../lib/clipKeys";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { ClipViewItem } from "../types";
import type { useAutomationFlow } from "./useAutomationFlow";
import type { useClipContextMenu } from "./useClipContextMenu";
import type { useQuickPreview } from "./useQuickPreview";

type IpasteStore = ReturnType<typeof useIpasteStore>;
type QuickPreviewApi = ReturnType<typeof useQuickPreview>;
type ClipMenuApi = ReturnType<typeof useClipContextMenu>;
type AutomationFlowApi = ReturnType<typeof useAutomationFlow>;

export type PanelKeyboardDeps = {
  store: IpasteStore;
  quickPreview: QuickPreviewApi;
  clipMenu: ClipMenuApi;
  automationFlow: AutomationFlowApi;
  closeFloatingLayers: () => void;
  hidePanelFromUi: () => Promise<void> | void;
  finishEditingCategory: () => void;
  openClipViewer?: (item: ClipViewItem) => Promise<void> | void;
  isModalOpen?: () => boolean;
  isEditingName?: () => boolean;
};

/**
 * 面板级键盘路由：
 * 处理底部操作栏提示的所有快捷键（Enter 粘贴/运行、Ctrl+C 复制、Space 放大查看、
 * Ctrl+K 动作、Backspace/Delete 删除、Tab/Shift+Tab 切换分类、Esc 关闭面板/清空搜索、
 * E 编辑动作、方向键导航、Ctrl+F 聚焦搜索、Ctrl+1~9 快捷切换分类）。
 */
export function usePanelKeyboard(deps: PanelKeyboardDeps) {
  const {
    store,
    quickPreview,
    clipMenu,
    automationFlow,
    closeFloatingLayers,
    hidePanelFromUi,
    finishEditingCategory,
    openClipViewer,
    isModalOpen,
    isEditingName,
  } = deps;

  function isSearchTarget(target: EventTarget | null): boolean {
    if (!target || typeof (target as HTMLElement).closest !== "function") return false;
    return Boolean((target as HTMLElement).closest(".raycast-search-input, .search-box input"));
  }

  function cycleCategory(delta: number) {
    const allCategoryIds = ["history", ...store.categories.map((category) => category.id), "automation"];
    const currentIndex = allCategoryIds.indexOf(store.selectedCategoryId);
    const length = allCategoryIds.length;
    if (length <= 1) return;
    const nextIndex = currentIndex === -1 ? 0 : (currentIndex + delta + length) % length;
    const targetCategoryId = allCategoryIds[nextIndex];
    if (targetCategoryId && targetCategoryId !== store.selectedCategoryId) {
      closeFloatingLayers();
      finishEditingCategory();
      store.selectCategory(targetCategoryId);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;

    if (isModalOpen?.()) return;

    if (isEditingName?.()) return;

    if (event.key !== "Backspace" && event.key !== "Delete") {
      clipMenu.pendingDeleteByKey.value = null;
    }

    if (quickPreview.handleQuickPreviewKeydown(event)) return;

    if (clipMenu.contextMenu.value) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeFloatingLayers();
      }
      return;
    }

    if (handleCategoryShortcut(event)) return;

    const isSearch = isSearchTarget(event.target);
    const isOtherEditable = !isSearch && quickPreview.isEditableTarget(event.target);
    if (isOtherEditable) return;

    // Ctrl+K / Cmd+K: Toggle between Automation and History tabs
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
      event.preventDefault();
      closeFloatingLayers();
      finishEditingCategory();
      if (store.selectedCategoryId === "automation") {
        store.selectCategory("history");
      } else {
        store.selectCategory("automation");
      }
      return;
    }

    // Ctrl+F / Cmd+F: Focus search input
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      focusSearch();
      return;
    }

    // Ctrl+C / Cmd+C: Copy selected item / command
    if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "c") {
      if (isSearch) {
        const input = event.target as HTMLInputElement;
        if (typeof input.selectionStart === "number" && typeof input.selectionEnd === "number" && input.selectionStart !== input.selectionEnd) {
          return; // Let browser copy selected text in search input
        }
      }
      event.preventDefault();
      if (store.selectedCategoryId === "automation") {
        const action = store.visibleActions[store.selectedActionIndex];
        if (action) void automationFlow.copyAutomationCommand(action);
      } else {
        const item = store.visibleItems[store.selectedIndex];
        if (item) void store.copyItem(item);
      }
      return;
    }

    // Tab / Shift+Tab: Cycle categories
    if (event.key === "Tab") {
      event.preventDefault();
      cycleCategory(event.shiftKey ? -1 : 1);
      return;
    }

    // Escape: Clear search or close panel
    if (event.key === "Escape") {
      event.preventDefault();
      if (isSearch && store.search.trim()) {
        store.clearSearch();
        return;
      }
      void hidePanelFromUi();
      return;
    }

    // Enter: Paste selected clip or run selected automation action
    if (event.key === "Enter") {
      event.preventDefault();
      if (store.selectedCategoryId === "automation") {
        const action = store.visibleActions[store.selectedActionIndex];
        if (action) automationFlow.runSelectedAction(action);
      } else {
        void store.applySelected();
      }
      return;
    }

    // Space: Expand/open clip viewer (only when not typing in search box)
    if (event.key === " " || event.key === "Spacebar") {
      if (!isSearch) {
        event.preventDefault();
        if (store.selectedCategoryId !== "automation") {
          const item = store.visibleItems[store.selectedIndex];
          if (item) void openClipViewer?.(item);
        }
        return;
      }
    }

    // E: Edit automation action (when in automation mode and not in search input)
    if (event.key.toLowerCase() === "e" && !event.metaKey && !event.ctrlKey && !event.altKey) {
      if (!isSearch && store.selectedCategoryId === "automation") {
        event.preventDefault();
        const action = store.visibleActions[store.selectedActionIndex];
        if (action) automationFlow.openAutomationEditor(action);
        return;
      }
    }

    // Backspace / Delete: Delete item / action (when not in search input)
    if (event.key === "Backspace" || event.key === "Delete") {
      if (!isSearch) {
        event.preventDefault();
        if (store.selectedCategoryId === "automation") {
          const action = store.visibleActions[store.selectedActionIndex];
          if (action) void automationFlow.deleteAutomationAction(action);
        } else {
          const item = store.visibleItems[store.selectedIndex];
          if (!item) return;

          const key = contextItemKey(item);
          if (clipMenu.pendingDeleteByKey.value === key) {
            void clipMenu.deleteSelectedItem(item);
          } else {
            clipMenu.pendingDeleteByKey.value = key;
          }
        }
        return;
      }
    }

    // Arrow keys: Navigation
    if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "ArrowLeft" || event.key === "ArrowRight") {
      if (isSearch) {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          if (store.selectedCategoryId === "automation") {
            store.selectedActionIndex = Math.min(
              store.selectedActionIndex + 1,
              Math.max(store.visibleActions.length - 1, 0),
            );
          } else {
            store.moveSelection(1);
          }
          return;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          if (store.selectedCategoryId === "automation") {
            store.selectedActionIndex = Math.max(store.selectedActionIndex - 1, 0);
          } else {
            store.moveSelection(-1);
          }
          return;
        }
        return; // Allow cursor movement within search input for ArrowLeft / ArrowRight
      }

      event.preventDefault();
      if (store.selectedCategoryId === "automation") {
        if (event.key === "ArrowDown" || event.key === "ArrowRight") {
          store.selectedActionIndex = Math.min(
            store.selectedActionIndex + 1,
            Math.max(store.visibleActions.length - 1, 0),
          );
        } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
          store.selectedActionIndex = Math.max(store.selectedActionIndex - 1, 0);
        }
      } else {
        if (event.key === "ArrowDown" || event.key === "ArrowRight") {
          store.moveSelection(1);
        } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
          store.moveSelection(-1);
        }
      }
      return;
    }
  }

  function handleKeyup(event: KeyboardEvent) {
    quickPreview.handleQuickPreviewKeyup(event);
  }

  function handleCategoryShortcut(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey) || event.altKey || !/^[1-9]$/.test(event.key)) {
      return false;
    }

    event.preventDefault();

    const categoryIds = ["history", ...store.categories.map((category) => category.id), "automation"];
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
    const input = document.querySelector<HTMLInputElement>(".raycast-search-input, .search-box input");
    input?.focus();
    input?.select();
  }

  return {
    handleKeydown,
    handleKeyup,
    handleCategoryShortcut,
    focusSearch,
    cycleCategory,
  };
}
