import { describe, expect, it } from "vitest";

// ?raw 经 import.meta.glob 预取源码文本（vite 原生能力，无需 node fs）
const sources = import.meta.glob(["../App.vue", "./ClipListPane.vue"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const appVue = sources["../App.vue"];
const clipListPaneVue = sources["./ClipListPane.vue"];

// 触底加载依赖 App.vue 模板上的三处接线：解构 handleClipListScroll、
// 给 ClipListPane 传函数 ref、监听转发的 scroll 事件。9c3d4c8 重构双栏
// 布局时这三处被整体删除，列表从此停留在首屏 20 条。此测试断言接线存在，
// 防止再次在模板重构中静默丢失。

describe("clip list scroll wiring", () => {
  it("App.vue 解构并绑定了触底加载处理器", () => {
    expect(appVue).toMatch(/handleClipListScroll,\s*\n\s*showClipListScrollbar/u);
    expect(appVue).toContain(`@scroll="handleClipListScroll"`);
  });

  it("App.vue 通过 list-ref 把滚动容器 DOM 交给 useClipListScroll", () => {
    expect(appVue).toContain(`:list-ref="setClipListElement"`);
    expect(appVue).toMatch(/function setClipListElement\(el: unknown\)/u);
  });

  it("ClipListPane 在滚动容器根节点上转发 ref 与 scroll 事件", () => {
    expect(clipListPaneVue).toMatch(/:ref="forwardListRef"/u);
    expect(clipListPaneVue).toMatch(/@scroll="emit\('scroll', \$event\)"/u);
  });
});
