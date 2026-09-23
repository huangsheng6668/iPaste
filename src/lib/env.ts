/** 运行环境判定唯一来源（Tauri 桌面 vs 浏览器开发模式）。 */
export const isTauri = "__TAURI_INTERNALS__" in window;

/** 平台判定（macOS 快捷键修饰键展示等）。 */
export const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
