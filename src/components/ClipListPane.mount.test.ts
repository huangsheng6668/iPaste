import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ClipListPane from "./ClipListPane.vue";
import type { ClipViewItem } from "../types";

// 由原来的源码字符串断言（clipListScrollWiring.test.ts）改写而来。
// 原测试用 import.meta.glob(?raw) 断言 App.vue / ClipListPane.vue 的源码里
// "必须出现某段字符串"——它锁的是实现写法而非行为，模板一重构就会假红。
// 这里改为真实挂载：断言"滚动容器交给父级"与"scroll 事件向上转发"两件行为。
// 被守护的 bug 背景：9c3d4c8 重构双栏布局时三处接线被整体删除，列表从此停在首屏 20 条。

function mountPane(overrides: { listRef?: (el: HTMLElement | null) => void } = {}) {
  // 始终返回 vi.fn 以便断言调用；overrides.listRef 只用于替换组件收到的回调。
  const listRef = vi.fn<(el: HTMLElement | null) => void>();
  const wrapper = mount(ClipListPane, {
    props: {
      listRef: overrides.listRef ?? listRef,
      items: [] as ClipViewItem[],
      selectedIndex: 0,
      selectedCategoryId: "history",
      isLoadingMore: false,
      canReorder: false,
      editingClipKey: null,
      editingClipName: "",
      pendingDeleteKey: null,
      draggingItemKey: null,
      itemDropTargetKey: null,
      itemDropSide: null,
      visibleActions: [],
      fallbackGroups: [],
      itemCategoryTags: () => [],
      itemDragStyle: () => undefined,
      toCategoryClipViewItem: (item) => ({ ...item, collection: "category" as const }),
    },
  });
  return { wrapper, listRef };
}

describe("ClipListPane 滚动接线（挂载行为）", () => {
  it("挂载后把滚动容器 DOM 交给父级（listRef 收到根节点元素）", () => {
    const { wrapper, listRef } = mountPane();

    expect(listRef).toHaveBeenCalled();
    const element = listRef.mock.calls[0][0] as HTMLElement | null;
    expect(element).toBeInstanceOf(HTMLElement);
    expect(element?.classList.contains("raycast-left-pane")).toBe(true);
    // 根节点即滚动容器：交给父级的元素必须与渲染出来的根节点是同一个
    expect(element).toBe(wrapper.element);

    wrapper.unmount();
  });

  it("卸载后向父级交还 null（避免父级持有已销毁的 DOM）", () => {
    const { wrapper, listRef } = mountPane();
    wrapper.unmount();

    expect(listRef).toHaveBeenLastCalledWith(null);
  });

  it("容器滚动时向上转发 scroll 事件（带原生事件对象）", async () => {
    const { wrapper } = mountPane();

    await wrapper.element.dispatchEvent(new Event("scroll"));

    const scrollEvents = wrapper.emitted("scroll");
    expect(scrollEvents).toHaveLength(1);
    expect(scrollEvents?.[0]?.[0]).toBeInstanceOf(Event);

    wrapper.unmount();
  });
});

describe("ClipListPane 虚拟化（窗口化渲染）", () => {
  function mountWithItems(count: number) {
    const items = Array.from({ length: count }, (_, index) => ({
      id: `clip-${index}`,
      collection: "history" as const,
      clipType: "text" as const,
      contentHash: `hash-${index}`,
      previewText: `preview ${index}`,
      text: `text ${index}`,
      displayName: null,
      sourceApp: "Test",
      isPinned: false,
      lastCapturedAt: new Date(index).toISOString(),
      favoriteCount: 0,
    }));
    const wrapper = mount(ClipListPane, {
      props: {
        listRef: () => {},
        items,
        selectedIndex: 0,
        selectedCategoryId: "history",
        isLoadingMore: false,
        canReorder: false,
        editingClipKey: null,
        editingClipName: "",
        pendingDeleteKey: null,
        draggingItemKey: null,
        itemDropTargetKey: null,
        itemDropSide: null,
        visibleActions: [],
        fallbackGroups: [],
        itemCategoryTags: () => [],
        itemDragStyle: () => undefined,
        toCategoryClipViewItem: (item) => ({ ...item, collection: "category" as const }),
      },
    });
    return wrapper;
  }

  it("上千条历史只挂载视口窗口内的卡片（jsdom 无布局，窗口为 Overscan 兜底值）", () => {
    const wrapper = mountWithItems(2000);

    // jsdom 下 clientHeight / getBoundingClientRect 均为 0，visibleCount 退化为
    // 1，窗口 = 首条 + 上下各 OVERSCAN(6) = 7 张；关键是绝不渲染全量 2000 张。
    const cards = wrapper.findAllComponents({ name: "ClipCard" });
    expect(cards.length).toBeGreaterThan(0);
    expect(cards.length).toBeLessThan(20);
    expect(cards.length).toBe(7);

    // 顶部 spacer 不存在（窗口从首条开始）；底部 spacer 撑起剩余高度。
    const spacers = wrapper.element.querySelectorAll("div[aria-hidden='true']");
    expect(spacers.length).toBe(1);

    wrapper.unmount();
  });

  it("卡片的 index prop 与 select 事件都是绝对索引（窗口起点已加回）", async () => {
    const wrapper = mountWithItems(50);

    const cards = wrapper.findAllComponents({ name: "ClipCard" });
    // 窗口从首条开始：index prop 即 v-for 位置。
    expect(cards.map((card) => card.props("index"))).toEqual(
      Array.from({ length: cards.length }, (_, index) => index),
    );

    await cards[3].vm.$emit("select", 3);

    const selectEvents = wrapper.emitted("select");
    expect(selectEvents?.[selectEvents.length - 1]?.[0]).toBe(3);

    wrapper.unmount();
  });
});
