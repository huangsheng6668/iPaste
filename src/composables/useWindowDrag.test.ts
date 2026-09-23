import { beforeEach, describe, expect, it, vi } from "vitest";

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
vi.stubGlobal("window", { setTimeout, clearTimeout });

const { useWindowDrag } = await import("./useWindowDrag");
const { ipasteApi } = await import("../lib/ipasteApi");

function leftClick() {
  return { button: 0, preventDefault: () => {} } as unknown as MouseEvent;
}

describe("useWindowDrag", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
