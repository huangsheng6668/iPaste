import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { hasTauriRuntime } from "./runtime";

export type { UnlistenFn };

// 全仓唯一事件订阅出口：Tauri 下注册监听并解包 payload；
// 浏览器 dev / 测试环境返回 no-op 反注册（与调用方原先的 isTauri 早退等价）。
// 测试通过 vi.mock 本模块注入内存事件总线，替代逐文件 mock @tauri-apps/api/event。
export function subscribe<T>(eventName: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!hasTauriRuntime()) return Promise.resolve(() => {});
  return listen<T>(eventName, (emitted) => handler(emitted.payload));
}
