import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    openLanSync: vi.fn(),
    startScreenshotOcr: vi.fn(),
  },
}));

const { ipasteApi } = await import("../lib/ipasteApi");
const { useUiStore } = await import("../stores/uiStore");
const CommandSearchBar = (await import("./CommandSearchBar.vue")).default;

const openLanSyncMock = vi.mocked(ipasteApi.openLanSync);
const startScreenshotOcrMock = vi.mocked(ipasteApi.startScreenshotOcr);

// 被守护的 bug：这两个按钮此前是裸的 `void ipasteApi.x()`，命令失败时点下去毫无反应。
// 设备同步尤其致命——面板一失焦就自动隐藏，用户只看到「面板没了、窗口也没出现」，
// 没有任何线索指向「命令失败了」。这里断言失败必须变成一条可见的 toast。

function mountBar() {
  return mount(CommandSearchBar, {
    props: {
      searchQuery: "",
      shortcut: "Ctrl+Shift+V",
      categories: [],
      selectedCategoryId: "history",
      editingCategoryId: null,
      historyCount: 0,
      categoryCounts: {},
      settingsOpen: false,
      appendCopyEnabled: false,
      appendCopyTimeoutMinutes: 1,
    },
    global: { plugins: [createPinia()] },
  });
}

function buttonByClass(wrapper: ReturnType<typeof mountBar>, index: number) {
  return wrapper.findAll("button.icon-button")[index];
}

describe("CommandSearchBar 静默失败出口", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setActivePinia(createPinia());
  });

  it("open_lan_sync 失败时弹 toast，而不是静默无反应", async () => {
    openLanSyncMock.mockRejectedValue({ code: "internal", message: "启动失败" });
    const wrapper = mountBar();

    // 头部图标按钮顺序：追加复制 / 截图 OCR / 设备同步
    await buttonByClass(wrapper, 2).trigger("click");
    await flushPromises();

    const toasts = useUiStore().toasts.map((toast) => toast.message);
    expect(toasts).toContain("启动失败");
    wrapper.unmount();
  });

  it("start_screenshot_ocr 抛出意外错误时同样弹 toast", async () => {
    startScreenshotOcrMock.mockRejectedValue(new Error("锁中毒"));
    const wrapper = mountBar();

    await buttonByClass(wrapper, 1).trigger("click");
    await flushPromises();

    const toasts = useUiStore().toasts.map((toast) => toast.message);
    expect(toasts).toContain("锁中毒");
    wrapper.unmount();
  });

  it("命令成功时不产生任何 toast", async () => {
    openLanSyncMock.mockResolvedValue(undefined);
    const wrapper = mountBar();

    await buttonByClass(wrapper, 2).trigger("click");
    await flushPromises();

    expect(useUiStore().toasts).toHaveLength(0);
    wrapper.unmount();
  });
});
