import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hasTauriRuntime } from "./runtime";

export type { UnlistenFn };

// 全仓唯一事件订阅出口：Tauri 下注册监听并解包 payload；
// 浏览器 dev / 测试环境返回 no-op 反注册（与调用方原先的 isTauri 早退等价）。
// 测试通过 vi.mock 本模块注入内存事件总线，替代逐文件 mock @tauri-apps/api/event。
export function subscribe<T>(eventName: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) return Promise.resolve(() => {});
  return listen<T>(eventName, (emitted) => handler(emitted.payload));
}

// 全仓唯一出站事件出口：向所有窗口广播（如查看器窗口保存后广播 clipUpdated，
// 主窗口据此更新列表）。浏览器 dev / 测试环境为 no-op。
export function publish<T>(eventName: string, payload: T): Promise<void> {
  if (!hasTauriRuntime()) return Promise.resolve();
  return emit<T>(eventName, payload);
}
