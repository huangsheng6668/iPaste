import { describe, expect, it } from "vitest";
import { commandFor, commandForCategoryShortcut, type PanelContext } from "./panelKeymap";

function keyEvent(init: {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}): KeyboardEvent {
  return {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    metaKey: init.metaKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
  } as unknown as KeyboardEvent;
}

function context(overrides: Partial<PanelContext> = {}): PanelContext {
  return {
    isAutomationMode: false,
    isSearchTarget: false,
    hasSearchQuery: false,
    hasSearchSelection: false,
    ...overrides,
  };
}

const primary = { ctrlKey: true };

describe("commandFor：组合键（Ctrl/Cmd）", () => {
  it("K：切换 automation 与普通页签（含 Cmd 变体）", () => {
    expect(commandFor(keyEvent({ key: "k", ...primary }), context())).toEqual({ type: "toggleCategory" });
    expect(commandFor(keyEvent({ key: "K", metaKey: true }), context())).toEqual({ type: "toggleCategory" });
  });

  it("K：带 Shift / Alt 不匹配（原实现同样要求无这两者）", () => {
    expect(commandFor(keyEvent({ key: "k", ctrlKey: true, shiftKey: true }), context())).toBeNull();
    expect(commandFor(keyEvent({ key: "k", ctrlKey: true, altKey: true }), context())).toBeNull();
  });

  it("F：聚焦搜索", () => {
    expect(commandFor(keyEvent({ key: "f", ...primary }), context())).toEqual({ type: "focusSearch" });
    expect(commandFor(keyEvent({ key: "f", ...primary, shiftKey: true }), context())).toBeNull();
  });

  it("C：复制；搜索框内有选区时放行给浏览器", () => {
    expect(commandFor(keyEvent({ key: "c", ...primary }), context())).toEqual({ type: "copy" });
    expect(commandFor(keyEvent({ key: "c", ...primary }), context({ isSearchTarget: true }))).toEqual({ type: "copy" });
    expect(
      commandFor(keyEvent({ key: "c", ...primary }), context({ isSearchTarget: true, hasSearchSelection: true })),
    ).toBeNull();
  });

  it("G：循环切换历史类型筛选；Shift 反向，搜索框内同样生效", () => {
    expect(commandFor(keyEvent({ key: "g", ...primary }), context())).toEqual({ type: "cycleTypeFilter", delta: 1 });
    expect(commandFor(keyEvent({ key: "G", metaKey: true }), context())).toEqual({ type: "cycleTypeFilter", delta: 1 });
    expect(commandFor(keyEvent({ key: "g", ...primary, shiftKey: true }), context())).toEqual({ type: "cycleTypeFilter", delta: -1 });
    expect(commandFor(keyEvent({ key: "g", ...primary }), context({ isSearchTarget: true }))).toEqual({ type: "cycleTypeFilter", delta: 1 });
    expect(commandFor(keyEvent({ key: "g", ...primary, altKey: true }), context())).toBeNull();
  });
});

describe("commandFor：Tab / Escape / Enter", () => {
  it("Tab 与 Shift+Tab：循环切换分类（方向相反）", () => {
    expect(commandFor(keyEvent({ key: "Tab" }), context())).toEqual({ type: "cycleCategory", delta: 1 });
    expect(commandFor(keyEvent({ key: "Tab", shiftKey: true }), context())).toEqual({ type: "cycleCategory", delta: -1 });
  });

  it("Escape：交给 dispatch 决定清搜索还是关面板", () => {
    expect(commandFor(keyEvent({ key: "Escape" }), context())).toEqual({ type: "escape" });
    expect(commandFor(keyEvent({ key: "Escape" }), context({ isSearchTarget: true, hasSearchQuery: true }))).toEqual({ type: "escape" });
  });

  it("Enter：激活（搜索框内同样生效，与原实现一致）", () => {
    expect(commandFor(keyEvent({ key: "Enter" }), context())).toEqual({ type: "activate" });
    expect(commandFor(keyEvent({ key: "Enter" }), context({ isSearchTarget: true }))).toEqual({ type: "activate" });
  });
});

describe("commandFor：Space / E / 删除", () => {
  it("Space：普通页签打开查看器；automation 页签消费但不动作；搜索框内放行", () => {
    expect(commandFor(keyEvent({ key: " " }), context())).toEqual({ type: "openViewer" });
    expect(commandFor(keyEvent({ key: "Spacebar" }), context())).toEqual({ type: "openViewer" });
    expect(commandFor(keyEvent({ key: " " }), context({ isAutomationMode: true }))).toEqual({ type: "none" });
    expect(commandFor(keyEvent({ key: " " }), context({ isSearchTarget: true }))).toBeNull();
  });

  it("E：仅 automation 页签且不在搜索框内可编辑动作", () => {
    expect(commandFor(keyEvent({ key: "e" }), context({ isAutomationMode: true }))).toEqual({ type: "editAction" });
    expect(commandFor(keyEvent({ key: "E" }), context({ isAutomationMode: true }))).toEqual({ type: "editAction" });
    expect(commandFor(keyEvent({ key: "e" }), context())).toBeNull();
    expect(commandFor(keyEvent({ key: "e" }), context({ isAutomationMode: true, isSearchTarget: true }))).toBeNull();
    expect(commandFor(keyEvent({ key: "e", ctrlKey: true }), context({ isAutomationMode: true }))).toBeNull();
    expect(commandFor(keyEvent({ key: "e", altKey: true }), context({ isAutomationMode: true }))).toBeNull();
  });

  it("Backspace / Delete：不在搜索框内时删除", () => {
    expect(commandFor(keyEvent({ key: "Backspace" }), context())).toEqual({ type: "delete" });
    expect(commandFor(keyEvent({ key: "Delete" }), context())).toEqual({ type: "delete" });
    expect(commandFor(keyEvent({ key: "Backspace" }), context({ isSearchTarget: true }))).toBeNull();
    expect(commandFor(keyEvent({ key: "Delete" }), context({ isSearchTarget: true }))).toBeNull();
  });
});

describe("commandFor：方向键", () => {
  it("非搜索框：上下左右都映射为移动（左右与上下同义）", () => {
    expect(commandFor(keyEvent({ key: "ArrowDown" }), context())).toEqual({ type: "move", delta: 1 });
    expect(commandFor(keyEvent({ key: "ArrowRight" }), context())).toEqual({ type: "move", delta: 1 });
    expect(commandFor(keyEvent({ key: "ArrowUp" }), context())).toEqual({ type: "move", delta: -1 });
    expect(commandFor(keyEvent({ key: "ArrowLeft" }), context())).toEqual({ type: "move", delta: -1 });
  });

  it("搜索框内：上下仍接管，左右放行给光标移动", () => {
    expect(commandFor(keyEvent({ key: "ArrowDown" }), context({ isSearchTarget: true }))).toEqual({ type: "move", delta: 1 });
    expect(commandFor(keyEvent({ key: "ArrowUp" }), context({ isSearchTarget: true }))).toEqual({ type: "move", delta: -1 });
    expect(commandFor(keyEvent({ key: "ArrowLeft" }), context({ isSearchTarget: true }))).toBeNull();
    expect(commandFor(keyEvent({ key: "ArrowRight" }), context({ isSearchTarget: true }))).toBeNull();
  });
});

describe("commandFor：未映射按键", () => {
  it("普通字符与无修饰键的数字不产生命令", () => {
    expect(commandFor(keyEvent({ key: "x" }), context())).toBeNull();
    expect(commandFor(keyEvent({ key: "1" }), context())).toBeNull();
    expect(commandFor(keyEvent({ key: "F5" }), context())).toBeNull();
  });
});

describe("commandForCategoryShortcut：Ctrl/Cmd+1..9 直跳分类", () => {
  it("数字 1..9 映射为从 0 开始的分类下标", () => {
    expect(commandForCategoryShortcut(keyEvent({ key: "1", ...primary }), context())).toEqual({ type: "selectCategoryIndex", index: 0 });
    expect(commandForCategoryShortcut(keyEvent({ key: "9", metaKey: true }), context())).toEqual({ type: "selectCategoryIndex", index: 8 });
  });

  it("无主修饰键、带 Alt、0 与多位数都不匹配", () => {
    expect(commandForCategoryShortcut(keyEvent({ key: "1" }), context())).toBeNull();
    expect(commandForCategoryShortcut(keyEvent({ key: "1", ctrlKey: true, altKey: true }), context())).toBeNull();
    expect(commandForCategoryShortcut(keyEvent({ key: "0", ...primary }), context())).toBeNull();
    expect(commandForCategoryShortcut(keyEvent({ key: "10", ...primary }), context())).toBeNull();
  });

  it("Shift 不参与判定（与原实现一致）", () => {
    expect(commandForCategoryShortcut(keyEvent({ key: "3", ctrlKey: true, shiftKey: true }), context())).toEqual({
      type: "selectCategoryIndex",
      index: 2,
    });
  });
});
