import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    setMainWindowDragging: vi.fn().mockResolvedValue(undefined),
    startMainWindowDrag: vi.fn().mockResolvedValue(true),
  },
}));
vi.mock("../lib/env", () => ({ isTauri: true }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: vi.fn().mockResolvedValue(undefined) }),
}));

// Vitest 跑在 node 环境（无 jsdom）：按仓库惯例 stub 掉 window 定时器全局。
// 用箭头包装而非直接传函数引用：调用时动态解析全局定时器实现，
// 配合 vi.useFakeTimers() 让 900ms 释放定时器落在假定时器队列里，
// 避免真实定时器悬挂到进程收尾产生 unhandled error（曾致 vitest 退出码 1）。
vi.stubGlobal("window", {
  setTimeout: (callback: () => void, ms?: number) => setTimeout(callback, ms),
  clearTimeout: (id: number) => clearTimeout(id),
});

const { useWindowDrag } = await import("./useWindowDrag");
const { ipasteApi } = await import("../lib/ipasteApi");

function leftClick() {
  return { button: 0, preventDefault: () => {} } as unknown as MouseEvent;
}

describe("useWindowDrag", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("主面板模式调用原生拖动并通知拖拽态", async () => {
    const { startWindowDrag } = useWindowDrag({ mainWindow: true });
    await startWindowDrag(leftClick());
    expect(ipasteApi.setMainWindowDragging).toHaveBeenCalledWith(true);
    expect(ipasteApi.startMainWindowDrag).toHaveBeenCalledWith();
  });

  it("非主键/非左键直接跳过", async () => {
    const { startWindowDrag } = useWindowDrag();
    await startWindowDrag({ button: 2, preventDefault: () => {} } as unknown as MouseEvent);
    expect(ipasteApi.setMainWindowDragging).not.toHaveBeenCalled();
  });
});
