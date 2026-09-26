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
