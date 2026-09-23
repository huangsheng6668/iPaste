import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../env";
import { clipsApi } from "./clips";
import { categoriesApi } from "./categories";
import { settingsApi } from "./settings";
import { ocrApi } from "./ocr";
import { deviceSyncApi } from "./deviceSync";
import { automationsApi } from "./automations";

/**
 * 命令调用统一封装：Tauri 下走 invoke；浏览器 dev 下返回 fallback（无 fallback 返回 undefined）。
 * 重载让“带 fallback”的调用诚实收敛为 Promise<T>（两条路径都必然返回 T），
 * “无 fallback”的调用保持 Promise<T | undefined>（浏览器 dev 下确实是 undefined）。
 */
export function call<T>(command: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T>;
export function call<T>(command: string, args?: Record<string, unknown>): Promise<T | undefined>;
export async function call<T>(command: string, args?: Record<string, unknown>, fallback?: T): Promise<T | undefined> {
  if (isTauri) return invoke<T>(command, args);
  if (fallback !== undefined) return structuredClone(fallback);
  return undefined;
}

export const ipasteApi = {
  ...clipsApi,
  ...categoriesApi,
  ...settingsApi,
  ...ocrApi,
  ...deviceSyncApi,
  ...automationsApi,
  setMainWindowDragging(dragging: boolean) {
    return call<void>("set_main_window_dragging", { dragging });
  },
  startMainWindowDrag() {
    return call<boolean>("start_main_window_drag", undefined, false);
  },
};

export { clipViewerStorageKey } from "./clips";
export type { LanClipSource } from "./deviceSync";
