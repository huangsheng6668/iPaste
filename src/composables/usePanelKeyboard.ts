import {
  commandFor,
  commandForCategoryShortcut,
  type PanelContext,
  type PanelDispatch,
} from "./panelKeymap";
import type { useClipContextMenu } from "./useClipContextMenu";
import type { useQuickPreview } from "./useQuickPreview";

type QuickPreviewApi = ReturnType<typeof useQuickPreview>;
type ClipMenuApi = ReturnType<typeof useClipContextMenu>;

export type PanelKeyboardDeps = {
  /** store 派生的面板态（每次按键实时求值）；事件目标相关的搜索框判定由本层补全。 */
  context: () => Pick<PanelContext, "isAutomationMode" | "hasSearchQuery">;
  /** 命令落点：App.vue 注入，逐条对应拆分前各分支体的实现。 */
  dispatch: PanelDispatch;
  /** 快速预览与右键菜单的按键优先级（先于键位表处理）。 */
  quickPreview: QuickPreviewApi;
  clipMenu: ClipMenuApi;
  closeFloatingLayers: () => void;
  isModalOpen?: () => boolean;
  isEditingName?: () => boolean;
};

/**
 * 面板级键盘路由：只负责"什么时候接管按键"（守卫 + 优先级钩子），
 * "按了什么键做什么"由 panelKeymap 的纯表决定，副作用由 dispatch 执行。
 *
 * 守卫与钩子顺序与原实现逐条对齐，尤其是：
 * ① 分类直跳（Ctrl/Cmd+1..9）先于"可编辑目标"判定——焦点在输入框内也生效；
 * ② 快速预览与右键菜单打开时优先吞掉按键；
 * ③ 模态/改名态直接放行；非 Backspace/Delete 按键先复位两击删除确认。
 */
export function usePanelKeyboard(deps: PanelKeyboardDeps) {
  const { dispatch, quickPreview, clipMenu, closeFloatingLayers, isModalOpen, isEditingName } = deps;

  function isSearchTarget(target: EventTarget | null): boolean {
    if (!target || typeof (target as HTMLElement).closest !== "function") return false;
    return Boolean((target as HTMLElement).closest(".raycast-search-input, .search-box input"));
  }

  /** 搜索框内是否选中了文本（此时 Ctrl/Cmd+C 放行给浏览器）。 */
  function hasTextSelection(target: EventTarget | null): boolean {
    if (!target) return false;
    const input = target as HTMLInputElement;
    if (typeof input.selectionStart !== "number") return false;
    return typeof input.selectionEnd === "number" && input.selectionStart !== input.selectionEnd;
  }

  function panelContext(event: KeyboardEvent): PanelContext {
    const isSearch = isSearchTarget(event.target);
    return {
      ...deps.context(),
      isSearchTarget: isSearch,
      hasSearchSelection: isSearch ? hasTextSelection(event.target) : false,
    };
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

    const ctx = panelContext(event);

    // 分类直跳先于"可编辑目标"守卫（原实现顺序，不得下移）。
    const categoryCommand = commandForCategoryShortcut(event, ctx);
    if (categoryCommand) {
      event.preventDefault();
      dispatch(categoryCommand, ctx);
      return;
    }

    // 非搜索框的可编辑目标：不接管按键（搜索框内的上下键仍由键位表接管）。
    if (!ctx.isSearchTarget && quickPreview.isEditableTarget(event.target)) return;

    const command = commandFor(event, ctx);
    if (!command) return;

    event.preventDefault();
    dispatch(command, ctx);
  }

  function handleKeyup(event: KeyboardEvent) {
    quickPreview.handleQuickPreviewKeyup(event);
  }

  return {
    handleKeydown,
    handleKeyup,
  };
}
