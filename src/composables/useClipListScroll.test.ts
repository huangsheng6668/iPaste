import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useClipListScroll } from "./useClipListScroll";
import type { useIpasteStore } from "../stores/ipasteStore";

// 行为测试：替掉原 clipListScrollWiring.test.ts 对 App.vue 源码字符串的断言。
// 触底加载真正依赖的是"处理器在什么条件下调用 loadMoreClips"，这里直接验证它。
// （App.vue 模板是否把它接到 @scroll 上，改由 ClipListPane.mount.test.ts 保证左端，
//  两端的组合即是原测试想守护的调用链。）

type IpasteStore = ReturnType<typeof useIpasteStore>;

function createStore(overrides: Partial<IpasteStore> = {}) {
  return {
    selectedCategoryId: "history",
    hasMoreClips: true,
    loadMoreClips: vi.fn().mockResolvedValue(undefined),
    reloadClips: vi.fn().mockResolvedValue(undefined),
    clampSelection: vi.fn(),
    selectedIndex: 0,
    search: "",
    ...overrides,
  } as unknown as IpasteStore;
}

function createScroller(state: { scrollHeight: number; scrollTop: number; clientHeight: number }) {
  return {
    scrollHeight: state.scrollHeight,
    scrollTop: state.scrollTop,
    clientHeight: state.clientHeight,
    scrollBy: vi.fn(),
    querySelector: () => null,
    getBoundingClientRect: () => ({ top: 0, bottom: 0, left: 0, right: 0 }) as DOMRect,
  } as unknown as HTMLElement;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useClipListScroll 触底加载", () => {
  it("距底部不足 160px 时请求加载下一页", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });
    scroll.clipListElement.value = createScroller({ scrollHeight: 1000, scrollTop: 900, clientHeight: 50 });

    scroll.handleClipListScroll();

    expect(store.loadMoreClips).toHaveBeenCalledTimes(1);
  });

  it("距底部较远时不加载", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });
    scroll.clipListElement.value = createScroller({ scrollHeight: 1000, scrollTop: 0, clientHeight: 50 });

    scroll.handleClipListScroll();

    expect(store.loadMoreClips).not.toHaveBeenCalled();
  });

  it("非 history 页签或已无更多时不加载（但仍显示滚动条）", () => {
    const otherTab = createStore({ selectedCategoryId: "cat-1" });
    const otherScroll = useClipListScroll({ store: otherTab });
    otherScroll.clipListElement.value = createScroller({ scrollHeight: 1000, scrollTop: 990, clientHeight: 50 });
    otherScroll.handleClipListScroll();
    expect(otherTab.loadMoreClips).not.toHaveBeenCalled();
    expect(otherScroll.isClipListScrolling.value).toBe(true);

    const exhausted = createStore({ hasMoreClips: false });
    const exhaustedScroll = useClipListScroll({ store: exhausted });
    exhaustedScroll.clipListElement.value = createScroller({ scrollHeight: 1000, scrollTop: 990, clientHeight: 50 });
    exhaustedScroll.handleClipListScroll();
    expect(exhausted.loadMoreClips).not.toHaveBeenCalled();
  });

  it("列表元素尚未就绪时安全返回", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });

    expect(() => scroll.handleClipListScroll()).not.toThrow();
    expect(store.loadMoreClips).not.toHaveBeenCalled();
  });
});

describe("useClipListScroll 滚动条与复位", () => {
  it("显示滚动条并在 780ms 后自动淡出", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });

    scroll.showClipListScrollbar();
    expect(scroll.isClipListScrolling.value).toBe(true);

    vi.advanceTimersByTime(779);
    expect(scroll.isClipListScrolling.value).toBe(true);

    vi.advanceTimersByTime(1);
    expect(scroll.isClipListScrolling.value).toBe(false);
  });

  it("复位：滚动位置归零并立即收起滚动条", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });
    const element = createScroller({ scrollHeight: 1000, scrollTop: 400, clientHeight: 50 });
    element.scrollTop = 400;
    scroll.clipListElement.value = element;
    scroll.showClipListScrollbar();

    scroll.resetClipListScroll();

    expect(element.scrollTop).toBe(0);
    expect(scroll.isClipListScrolling.value).toBe(false);
  });

  it("cleanup 清掉挂起的定时器（不再触发副作用）", () => {
    const store = createStore();
    const scroll = useClipListScroll({ store });
    const element = createScroller({ scrollHeight: 1000, scrollTop: 400, clientHeight: 50 });
    element.scrollTop = 400;
    scroll.clipListElement.value = element;
    scroll.showClipListScrollbar();

    scroll.cleanup();
    vi.advanceTimersByTime(1000);

    // 定时器已被清理：滚动条标志保持调用时的状态，不再被回调改写
    expect(scroll.isClipListScrolling.value).toBe(true);
  });
});
