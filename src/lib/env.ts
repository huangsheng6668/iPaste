/** 运行环境判定唯一来源（Tauri 桌面 vs 浏览器开发模式）。 */
export const isTauri = "__TAURI_INTERNALS__" in window;
