import { describe, expect, it, vi } from "vitest";
import { reactive, ref } from "vue";
import { usePanelKeyboard } from "./usePanelKeyboard";
import type { AutomationAction, Category, ClipViewItem } from "../types";

function createFakeElement(options: { isSearch?: boolean; isInput?: boolean } = {}): EventTarget {
  return {
    closest(sel: string) {
      if (options.isSearch && sel.includes("raycast-search-input")) return {};
      if (options.isInput && sel.includes("input")) return {};
      return null;
    },
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
  const clip1: ClipViewItem = {
    id: "1",
    clipType: "text",
    contentHash: "hash1",
    displayName: null,
    sourceApp: null,
    text: "clip one",
    previewText: "clip one",
    collection: "history",
    lastCapturedAt: "10:00",
    favoriteCount: 0,
    isPinned: false,
  };
  const clip2: ClipViewItem = {
    id: "2",
    clipType: "text",
    contentHash: "hash2",
    displayName: null,
    sourceApp: null,
    text: "clip two",
    previewText: "clip two",
    collection: "history",
    lastCapturedAt: "10:01",
    favoriteCount: 0,
    isPinned: false,
  };

  const action1: AutomationAction = {
    id: "action-1",
    name: "Action 1",
    command: "echo 1",
    confirmBeforeRun: false,
    sortOrder: 0,
    cwd: null,
    runMode: "terminal",
    closePanelOnSuccess: false,
    lastRun: null,
    createdAt: "10:00",
    updatedAt: "10:00",
  };

  const store = reactive({
    selectedCategoryId: "history",
    categories: [{ id: "cat-1", name: "Dev" }] as Category[],
    visibleItems: [clip1, clip2] as ClipViewItem[],
    selectedIndex: 0,
    get selectedItem(): ClipViewItem | null {
      return this.visibleItems[this.selectedIndex] ?? null;
    },
    visibleActions: [action1] as AutomationAction[],
    selectedActionIndex: 0,
    search: "",
    applySelected: vi.fn(),
    copyItem: vi.fn(),
    moveSelection: vi.fn((delta: number) => {
      store.selectedIndex += delta;
    }),
    selectCategory: vi.fn((id: string) => {
      store.selectedCategoryId = id;
    }),
    clearSearch: vi.fn(() => {
      store.search = "";
    }),
  });

  const quickPreview = {
    quickPreviewItem: ref<ClipViewItem | null>(null),
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
    deleteSelectedItem: vi.fn(),
  };

  const automationFlow = {
    runSelectedAction: vi.fn(),
    openAutomationEditor: vi.fn(),
    copyAutomationCommand: vi.fn(),
    deleteAutomationAction: vi.fn(),
    handleActionsKey: vi.fn(() => false),
  };

  const closeFloatingLayers = vi.fn();
  const hidePanelFromUi = vi.fn();
  const finishEditingCategory = vi.fn();
  const openClipViewer = vi.fn();

  const deps = {
    store,
    quickPreview,
    clipMenu,
    automationFlow,
    closeFloatingLayers,
    hidePanelFromUi,
    finishEditingCategory,
    openClipViewer,
    ...overrides,
  } as unknown as Parameters<typeof usePanelKeyboard>[0];

  return { deps, store, clipMenu, automationFlow, openClipViewer, hidePanelFromUi };
}

describe("usePanelKeyboard shortcuts", () => {
  it("Enter: applies selected clip in history mode", () => {
    const { deps, store } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Enter" });
    keyboard.handleKeydown(event);

    expect(store.applySelected).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Enter: runs automation action in automation mode", () => {
    const { deps, store, automationFlow } = setupDeps();
    store.selectedCategoryId = "automation";
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Enter" });
    keyboard.handleKeydown(event);

    expect(automationFlow.runSelectedAction).toHaveBeenCalledWith(store.visibleActions[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Ctrl+C: copies selected clip in history mode", () => {
    const { deps, store } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "c", ctrlKey: true });
    keyboard.handleKeydown(event);

    expect(store.copyItem).toHaveBeenCalledWith(store.visibleItems[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Ctrl+C: copies automation command in automation mode", () => {
    const { deps, store, automationFlow } = setupDeps();
    store.selectedCategoryId = "automation";
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "c", ctrlKey: true });
    keyboard.handleKeydown(event);

    expect(automationFlow.copyAutomationCommand).toHaveBeenCalledWith(store.visibleActions[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Space: opens clip viewer in history mode when not in search input", () => {
    const { deps, store, openClipViewer } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: " " });
    keyboard.handleKeydown(event);

    expect(openClipViewer).toHaveBeenCalledWith(store.visibleItems[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Ctrl+K: toggles to automation category and back", () => {
    const { deps, store } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event1 = createFakeEvent({ key: "k", ctrlKey: true });
    keyboard.handleKeydown(event1);
    expect(store.selectCategory).toHaveBeenLastCalledWith("automation");

    store.selectedCategoryId = "automation";
    const event2 = createFakeEvent({ key: "k", ctrlKey: true });
    keyboard.handleKeydown(event2);
    expect(store.selectCategory).toHaveBeenLastCalledWith("history");
  });

  it("Backspace: two-stage delete in history mode", () => {
    const { deps, store, clipMenu } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event1 = createFakeEvent({ key: "Backspace" });
    keyboard.handleKeydown(event1);
    expect(clipMenu.pendingDeleteByKey.value).toBe("history-1");
    expect(clipMenu.deleteSelectedItem).not.toHaveBeenCalled();

    const event2 = createFakeEvent({ key: "Backspace" });
    keyboard.handleKeydown(event2);
    expect(clipMenu.deleteSelectedItem).toHaveBeenCalledWith(store.visibleItems[0]);
  });

  it("Backspace: deletes action in automation mode", () => {
    const { deps, store, automationFlow } = setupDeps();
    store.selectedCategoryId = "automation";
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Backspace" });
    keyboard.handleKeydown(event);

    expect(automationFlow.deleteAutomationAction).toHaveBeenCalledWith(store.visibleActions[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("E: opens automation editor in automation mode", () => {
    const { deps, store, automationFlow } = setupDeps();
    store.selectedCategoryId = "automation";
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "e" });
    keyboard.handleKeydown(event);

    expect(automationFlow.openAutomationEditor).toHaveBeenCalledWith(store.visibleActions[0]);
    expect(event.defaultPrevented).toBe(true);
  });

  it("Tab: cycles categories forward and Shift+Tab cycles backward", () => {
    const { deps, store } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    // Categories: history -> cat-1 -> automation
    keyboard.handleKeydown(createFakeEvent({ key: "Tab" }));
    expect(store.selectCategory).toHaveBeenLastCalledWith("cat-1");

    store.selectedCategoryId = "cat-1";
    keyboard.handleKeydown(createFakeEvent({ key: "Tab" }));
    expect(store.selectCategory).toHaveBeenLastCalledWith("automation");

    store.selectedCategoryId = "automation";
    keyboard.handleKeydown(createFakeEvent({ key: "Tab" }));
    expect(store.selectCategory).toHaveBeenLastCalledWith("history");

    // Shift+Tab backward
    store.selectedCategoryId = "history";
    keyboard.handleKeydown(createFakeEvent({ key: "Tab", shiftKey: true }));
    expect(store.selectCategory).toHaveBeenLastCalledWith("automation");
  });

  it("Esc: closes panel when search is empty", () => {
    const { deps, hidePanelFromUi } = setupDeps();
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Escape" });
    keyboard.handleKeydown(event);

    expect(hidePanelFromUi).toHaveBeenCalledTimes(1);
  });

  it("Esc: clears search when search is not empty in search input", () => {
    const { deps, store, hidePanelFromUi } = setupDeps();
    store.search = "hello";
    const searchTarget = createFakeElement({ isSearch: true });
    const keyboard = usePanelKeyboard(deps);

    const event = createFakeEvent({ key: "Escape", target: searchTarget });
    keyboard.handleKeydown(event);

    expect(store.clearSearch).toHaveBeenCalledTimes(1);
    expect(hidePanelFromUi).not.toHaveBeenCalled();
  });
});
