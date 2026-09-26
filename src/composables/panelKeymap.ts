// 面板键位表（Task 21 从 usePanelKeyboard 抽出）：把"按了什么键 → 做什么"变成纯数据决策。
// 只做键位判定，不碰 DOM、不碰 store；副作用由 App.vue 注入的 dispatch 执行。
// 键位集合与原实现逐条对齐，**不得增删**；修饰键条件也照抄（方向键 / Tab / Enter /
// Escape / Space / Backspace 原实现均不限制修饰键，本表同样不限制）。

/** 每次按键实时求值的面板态（命令决策的唯一输入）。 */
export type PanelContext = {
  /** 当前页签是否为 automation（决定 Enter/E/Space/Backspace 与方向键落点）。 */
  isAutomationMode: boolean;
  /** 事件目标是否为搜索输入框（决定 Space/E/Backspace 是否生效、方向键是否放行）。 */
  isSearchTarget: boolean;
  /** 搜索框内已有非空查询（Escape 优先清空搜索）。 */
  hasSearchQuery: boolean;
  /** 搜索框内存在文本选区（Ctrl/Cmd+C 让浏览器自行复制）。 */
  hasSearchSelection: boolean;
};

export type PanelCommand =
  | { type: "move"; delta: number }
  | { type: "activate" }
  | { type: "copy" }
  | { type: "delete" }
  | { type: "openViewer" }
  | { type: "editAction" }
  | { type: "toggleCategory" }
  | { type: "cycleCategory"; delta: number }
  | { type: "selectCategoryIndex"; index: number }
  | { type: "focusSearch" }
  | { type: "escape" }
  /** 已消费但不动作：保留原实现的 preventDefault 语义（如 automation 页签下的 Space）。 */
  | { type: "none" };

export type PanelDispatch = (command: PanelCommand, ctx: PanelContext) => void;

function hasPrimaryModifier(event: KeyboardEvent): boolean {
  return event.metaKey || event.ctrlKey;
}

/**
 * Ctrl/Cmd+1..9 分类直跳。
 * 单独成函数是因为它在原实现里**先于**"可编辑目标"判定执行：
 * 焦点在输入框内时数字快捷键仍然生效，位置不能挪进主键位表。
 * 保留 ctx 形参只为与 commandFor 的签名对称（本快捷键不依赖面板态）。
 */
export function commandForCategoryShortcut(event: KeyboardEvent, _ctx: PanelContext): PanelCommand | null {
  if (!hasPrimaryModifier(event) || event.altKey) return null;
  if (!/^[1-9]$/.test(event.key)) return null;
  return { type: "selectCategoryIndex", index: Number(event.key) - 1 };
}

/** 主键位表：返回 null 表示"不消费"（放行给浏览器或其他处理器）。 */
export function commandFor(event: KeyboardEvent, ctx: PanelContext): PanelCommand | null {
  // Ctrl/Cmd+K：automation ↔ history 之间切换
  if (hasPrimaryModifier(event) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
    return { type: "toggleCategory" };
  }

  // Ctrl/Cmd+F：聚焦搜索
  if (hasPrimaryModifier(event) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "f") {
    return { type: "focusSearch" };
  }

  // Ctrl/Cmd+C：复制选中条目/动作命令；搜索框内有选区时让浏览器复制
  if (hasPrimaryModifier(event) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "c") {
    if (ctx.isSearchTarget && ctx.hasSearchSelection) return null;
    return { type: "copy" };
  }

  // Tab / Shift+Tab：循环切换分类
  if (event.key === "Tab") {
    return { type: "cycleCategory", delta: event.shiftKey ? -1 : 1 };
  }

  // Escape：清空搜索或关闭面板
  if (event.key === "Escape") {
    return { type: "escape" };
  }

  // Enter：粘贴选中条目 / 运行选中动作
  if (event.key === "Enter") {
    return { type: "activate" };
  }

  // Space：展开查看器（搜索框内不生效；automation 页签下消费但不动作）
  if (event.key === " " || event.key === "Spacebar") {
    if (ctx.isSearchTarget) return null;
    return ctx.isAutomationMode ? { type: "none" } : { type: "openViewer" };
  }

  // E：编辑选中动作（仅 automation 页签且不在搜索框内）
  if (event.key.toLowerCase() === "e" && !hasPrimaryModifier(event) && !event.altKey) {
    if (ctx.isSearchTarget || !ctx.isAutomationMode) return null;
    return { type: "editAction" };
  }

  // Backspace / Delete：删除条目或动作（搜索框内不生效）
  if (event.key === "Backspace" || event.key === "Delete") {
    return ctx.isSearchTarget ? null : { type: "delete" };
  }

  // 方向键：搜索框内只接管上下（左右放行给光标移动）
  if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "ArrowLeft" || event.key === "ArrowRight") {
    const forward = event.key === "ArrowDown" || event.key === "ArrowRight";
    if (ctx.isSearchTarget && (event.key === "ArrowLeft" || event.key === "ArrowRight")) return null;
    return { type: "move", delta: forward ? 1 : -1 };
  }

  return null;
}
