import { getCurrentWindow } from "@tauri-apps/api/window";
import { hasTauriRuntime } from "./runtime";

// 窗口能力最小面：调用方实际用到的成员集合。真实实现来自 Tauri 当前窗口；
// 浏览器 dev / 测试环境返回 no-op 替身（保持调用方 .catch 静默语义不变）。
export interface PlatformWindow {
  readonly label: string;
  startDragging(): Promise<void>;
  close(): Promise<void>;
  hide(): Promise<void>;
  show(): Promise<void>;
  setFocus(): Promise<void>;
  setAlwaysOnTop(value: boolean): Promise<void>;
  isAlwaysOnTop(): Promise<boolean>;
  onCloseRequested(handler: (event: { preventDefault(): void }) => void | Promise<void>): Promise<() => void>;
}

// 全仓唯一窗口句柄出口；测试通过 vi.mock 本模块注入记录型替身，
// 不再需要逐文件 mock @tauri-apps/api/window。
export function windowHandle(): PlatformWindow {
  return hasTauriRuntime() ? getCurrentWindow() : browserWindowStub();
}

const settle = () => Promise.resolve();

function browserWindowStub(): PlatformWindow {
  return {
    label: "browser",
    startDragging: settle,
    close: settle,
    hide: settle,
    show: settle,
    setFocus: settle,
    setAlwaysOnTop: settle,
    isAlwaysOnTop: () => Promise.resolve(false),
    onCloseRequested: () => Promise.resolve(() => {}),
  };
}
