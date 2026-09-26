import { describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import { usePanelKeyboard } from "./usePanelKeyboard";
import type { PanelCommand, PanelContext } from "./panelKeymap";

// 路由层测试：断言"什么时候接管按键"与"派发了哪条命令"。
// 键位本身的判定由 panelKeymap.test.ts 覆盖；命令的副作用由 App.vue 的 dispatch 承担。

// 注意：搜索框选择器（.raycast-search-input, .search-box input）本身含 "input" 子串，
// 必须先判搜索选择器，否则 isInput 替身会被误判成搜索框。
function createFakeElement(options: { isSearch?: boolean; isInput?: boolean } = {}): EventTarget {
  return {
    closest(sel: string) {
      if (sel.includes("raycast-search-input")) return options.isSearch ? {} : null;
      if (sel.includes("input")) return options.isInput ? {} : null;
      return null;
    },
  } as unknown as EventTarget;
}

function createSearchInput(selection: { start: number; end: number } | null): EventTarget {
  return {
    closest: (sel: string) => (sel.includes("raycast-search-input") ? {} : null),
    selectionStart: selection?.start ?? 0,
    selectionEnd: selection?.end ?? 0,
  } as unknown as EventTarget;
}

function createFakeEvent(init: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  target?: EventTarget;
  defaultPrevented?: boolean;
}): KeyboardEvent {
  let prevented = init.defaultPrevented ?? false;
  return {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    metaKey: init.metaKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    target: init.target ?? createFakeElement(),
    get defaultPrevented() {
      return prevented;
    },
    preventDefault() {
      prevented = true;
    },
  } as unknown as KeyboardEvent;
}

function setupDeps(overrides: Record<string, unknown> = {}) {
  const quickPreview = {
    handleQuickPreviewKeydown: vi.fn(() => false),
    handleQuickPreviewKeyup: vi.fn(),
    isEditableTarget: vi.fn((target: unknown) => {
      const element = target as { closest?: (sel: string) => Element | null } | null;
      if (typeof element?.closest === "function") {
        return Boolean(element.closest("input, textarea, select"));
      }
      return false;
    }),
  };

  const clipMenu = {
    contextMenu: ref<unknown | null>(null),
    pendingDeleteByKey: ref<string | null>(null),
    close: vi.fn(),
    deleteSelectedItem: vi.fn(),
  };

  const dispatch = vi.fn<(command: PanelCommand, ctx: PanelContext) => void>();
  const closeFloatingLayers = vi.fn();

  const deps = {
    context: () => ({ isAutomationMode: false, hasSearchQuery: false }),
    dispatch,
    quickPreview,
    clipMenu,
    closeFloatingLayers,
    ...overrides,
  } as unknown as Parameters<typeof usePanelKeyboard>[0];

  return { deps, dispatch, clipMenu, quickPreview, closeFloatingLayers };
}

function lastCommand(dispatch: ReturnType<typeof vi.fn>): PanelCommand {
  return dispatch.mock.calls[dispatch.mock.calls.length - 1][0] as PanelCommand;
}

function lastContext(dispatch: ReturnType<typeof vi.fn>): PanelContext {
  return dispatch.mock.calls[dispatch.mock.calls.length - 1][1] as PanelContext;
}

describe("usePanelKeyboard 路由：命令派发", () => {
  it("Enter：派发 activate，并带上当前页签上下文", () => {
    const { deps, dispatch } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Enter" });
    keyboard.handleKeydown(event);

    expect(lastCommand(dispatch)).toEqual({ type: "activate" });
    expect(lastContext(dispatch).isAutomationMode).toBe(false);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Enter：automation 页签下上下文标记为 automation", () => {
    const { deps, dispatch } = setupDeps({
      context: () => ({ isAutomationMode: true, hasSearchQuery: false }),
    });
    const keyboard = usePanelKeyboard(deps);

    keyboard.handleKeydown(createFakeEvent({ key: "Enter" }));

    expect(lastCommand(dispatch)).toEqual({ type: "activate" });
    expect(lastContext(dispatch).isAutomationMode).toBe(true);
  });

  it("Ctrl+C：派发 copy", () => {
    const { deps, dispatch } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "c", ctrlKey: true });
    keyboard.handleKeydown(event);

    expect(lastCommand(dispatch)).toEqual({ type: "copy" });
    expect(event.defaultPrevented).toBe(true);
  });

  it("Ctrl+C：搜索框内有选区时不接管（放行给浏览器复制）", () => {
    const { deps, dispatch } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({
      key: "c",
      ctrlKey: true,
      target: createSearchInput({ start: 1, end: 4 }),
    });
    keyboard.handleKeydown(event);

    expect(dispatch).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it("Space：非搜索框派发 openViewer；automation 页签派发 none；搜索框内不接管", () => {
    const history = setupDeps();
    usePanelKeyboard(history.deps).handleKeydown(createFakeEvent({ key: " " }));
    expect(lastCommand(history.dispatch)).toEqual({ type: "openViewer" });

    const automation = setupDeps({ context: () => ({ isAutomationMode: true, hasSearchQuery: false }) });
    usePanelKeyboard(automation.deps).handleKeydown(createFakeEvent({ key: " " }));
    expect(lastCommand(automation.dispatch)).toEqual({ type: "none" });

    const search = setupDeps();
    const searchEvent = createFakeEvent({ key: " ", target: createSearchInput(null) });
    usePanelKeyboard(search.deps).handleKeydown(searchEvent);
    expect(search.dispatch).not.toHaveBeenCalled();
    expect(searchEvent.defaultPrevented).toBe(false);
  });

  it("Ctrl+K / Tab / Shift+Tab：分别派发切换与循环命令", () => {
    const keyboardSetup = setupDeps();
    const keyboard = usePanelKeyboard(keyboardSetup.deps);

    keyboard.handleKeydown(createFakeEvent({ key: "k", ctrlKey: true }));
    expect(lastCommand(keyboardSetup.dispatch)).toEqual({ type: "toggleCategory" });

    keyboard.handleKeydown(createFakeEvent({ key: "Tab" }));
    expect(lastCommand(keyboardSetup.dispatch)).toEqual({ type: "cycleCategory", delta: 1 });

    keyboard.handleKeydown(createFakeEvent({ key: "Tab", shiftKey: true }));
    expect(lastCommand(keyboardSetup.dispatch)).toEqual({ type: "cycleCategory", delta: -1 });
  });

  it("Backspace / Delete：派发 delete（搜索框内不接管）", () => {
    const backspace = setupDeps();
    usePanelKeyboard(backspace.deps).handleKeydown(createFakeEvent({ key: "Backspace" }));
    expect(lastCommand(backspace.dispatch)).toEqual({ type: "delete" });

    const inSearch = setupDeps();
    usePanelKeyboard(inSearch.deps).handleKeydown(createFakeEvent({ key: "Delete", target: createSearchInput(null) }));
    expect(inSearch.dispatch).not.toHaveBeenCalled();
  });

  it("E：automation 页签派发 editAction，普通页签不接管", () => {
    const automation = setupDeps({ context: () => ({ isAutomationMode: true, hasSearchQuery: false }) });
    usePanelKeyboard(automation.deps).handleKeydown(createFakeEvent({ key: "e" }));
    expect(lastCommand(automation.dispatch)).toEqual({ type: "editAction" });

    const history = setupDeps();
    usePanelKeyboard(history.deps).handleKeydown(createFakeEvent({ key: "e" }));
    expect(history.dispatch).not.toHaveBeenCalled();
  });

  it("Escape：派发 escape，搜索态在上下文里标记", () => {
    const { deps, dispatch } = setupDeps({
      context: () => ({ isAutomationMode: false, hasSearchQuery: true }),
    });
    const keyboard = usePanelKeyboard(deps);

    keyboard.handleKeydown(createFakeEvent({ key: "Escape", target: createSearchInput(null) }));

    expect(lastCommand(dispatch)).toEqual({ type: "escape" });
    expect(lastContext(dispatch)).toMatchObject({ isSearchTarget: true, hasSearchQuery: true });
  });

  it("Ctrl+数字：先于可编辑目标守卫派发分类直跳", () => {
    const { deps, dispatch } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    // 焦点在普通输入框（非搜索框）里：数字快捷键仍应生效
    const event = createFakeEvent({ key: "2", ctrlKey: true, target: createFakeElement({ isInput: true }) });
    keyboard.handleKeydown(event);

    expect(lastCommand(dispatch)).toEqual({ type: "selectCategoryIndex", index: 1 });
    expect(event.defaultPrevented).toBe(true);
  });

  it("方向键：派发 move 命令（搜索框内左右键放行）", () => {
    const { deps, dispatch } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    keyboard.handleKeydown(createFakeEvent({ key: "ArrowDown" }));
    expect(lastCommand(dispatch)).toEqual({ type: "move", delta: 1 });

    keyboard.handleKeydown(createFakeEvent({ key: "ArrowUp" }));
    expect(lastCommand(dispatch)).toEqual({ type: "move", delta: -1 });

    const inSearch = setupDeps();
    usePanelKeyboard(inSearch.deps).handleKeydown(createFakeEvent({ key: "ArrowLeft", target: createSearchInput(null) }));
    expect(inSearch.dispatch).not.toHaveBeenCalled();
  });

  it("Ctrl+F：派发 focusSearch", () => {
    const { deps, dispatch } = setupDeps();
    usePanelKeyboard(deps).handleKeydown(createFakeEvent({ key: "f", ctrlKey: true }));
    expect(lastCommand(dispatch)).toEqual({ type: "focusSearch" });
  });
});

describe("usePanelKeyboard 路由：守卫与优先级", () => {
  it("已被 preventDefault 的事件不再处理", () => {
    const { deps, dispatch } = setupDeps();
    usePanelKeyboard(deps).handleKeydown(createFakeEvent({ key: "Enter", defaultPrevented: true }));
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("模态打开或改名中直接放行", () => {
    const modal = setupDeps({ isModalOpen: () => true });
    usePanelKeyboard(modal.deps).handleKeydown(createFakeEvent({ key: "Enter" }));
    expect(modal.dispatch).not.toHaveBeenCalled();

    const editing = setupDeps({ isEditingName: () => true });
    usePanelKeyboard(editing.deps).handleKeydown(createFakeEvent({ key: "Enter" }));
    expect(editing.dispatch).not.toHaveBeenCalled();
  });

  it("快速预览接管时不再派发命令", () => {
    const { deps, dispatch, quickPreview } = setupDeps();
    quickPreview.handleQuickPreviewKeydown.mockReturnValue(true);

    usePanelKeyboard(deps).handleKeydown(createFakeEvent({ key: "Enter" }));

    expect(dispatch).not.toHaveBeenCalled();
  });

  it("右键菜单打开时：Escape 关闭浮层，其余按键只吞掉", () => {
    const { deps, dispatch, clipMenu, closeFloatingLayers } = setupDeps();
    clipMenu.contextMenu.value = { item: {}, index: 0, x: 0, y: 0 };
    const keyboard = usePanelKeyboard(deps);

    keyboard.handleKeydown(createFakeEvent({ key: "Escape" }));
    expect(closeFloatingLayers).toHaveBeenCalledTimes(1);
    expect(dispatch).not.toHaveBeenCalled();

    keyboard.handleKeydown(createFakeEvent({ key: "Enter" }));
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("非搜索框的可编辑目标（input/textarea）不接管按键", () => {
    const { deps, dispatch } = setupDeps();
    const event = createFakeEvent({ key: "Enter", target: createFakeElement({ isInput: true }) });

    usePanelKeyboard(deps).handleKeydown(event);

    expect(dispatch).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
  });

  it("非删除键会复位两击删除确认", () => {
    const { deps, clipMenu } = setupDeps();
    clipMenu.pendingDeleteByKey.value = "history-1";

    usePanelKeyboard(deps).handleKeydown(createFakeEvent({ key: "ArrowDown" }));

    expect(clipMenu.pendingDeleteByKey.value).toBeNull();
  });

  it("删除键保留两击确认状态", () => {
    const { deps, clipMenu } = setupDeps();
    clipMenu.pendingDeleteByKey.value = "history-1";

    usePanelKeyboard(deps).handleKeydown(createFakeEvent({ key: "Backspace" }));

    expect(clipMenu.pendingDeleteByKey.value).toBe("history-1");
  });

  it("keyup 透传给快速预览", () => {
    const { deps, quickPreview } = setupDeps();
    const event = createFakeEvent({ key: "Control" });

    usePanelKeyboard(deps).handleKeyup(event);

    expect(quickPreview.handleQuickPreviewKeyup).toHaveBeenCalledWith(event);
  });
});
