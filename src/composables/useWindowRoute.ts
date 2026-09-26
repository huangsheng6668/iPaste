// 多窗口路由（Task 22 从 App.vue 抽出）：5 个辅助窗口共用同一份 App.vue 代码，
// 靠 ?window=<kind> 查询参数选择渲染分支；无参数或不认识的值一律视为主面板。
// 这是窗口类型的唯一权威来源，模板与脚本都从这里取值。

export type WindowKind = "main" | "settings" | "clip-viewer" | "lan-sync" | "ocr-overlay" | "ocr-result";

const AUXILIARY_WINDOW_KINDS: ReadonlyArray<Exclude<WindowKind, "main">> = [
  "settings",
  "clip-viewer",
  "lan-sync",
  "ocr-overlay",
  "ocr-result",
];

function isAuxiliaryWindowKind(value: string): value is Exclude<WindowKind, "main"> {
  return (AUXILIARY_WINDOW_KINDS as ReadonlyArray<string>).includes(value);
}

/** 解析窗口类型；search 可注入（测试用），默认取当前地址栏查询串。 */
export function currentWindowKind(search: string = window.location.search): WindowKind {
  const value = new URLSearchParams(search).get("window");
  return value && isAuxiliaryWindowKind(value) ? value : "main";
}

/** 组合式入口：一次解析，供脚本与模板共用。 */
export function useWindowRoute() {
  const windowKind = currentWindowKind();
  return {
    windowKind,
    isMainWindow: windowKind === "main",
  };
}
