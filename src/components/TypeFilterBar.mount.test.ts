import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { ClipTypeFilter } from "../types";

const TypeFilterBar = (await import("./TypeFilterBar.vue")).default;

function mountBar(filter: ClipTypeFilter = "all") {
  return mount(TypeFilterBar, {
    props: {
      filter,
      totalCount: 42,
      textCount: 40,
      imageCount: 2,
    },
    global: { plugins: [createPinia()] },
  });
}

describe("TypeFilterBar 分段控件", () => {
  it("渲染全部/文本/图片三段与各自计数徽章", () => {
    setActivePinia(createPinia());
    const wrapper = mountBar();

    const options = wrapper.findAll("button.type-filter-option");
    expect(options).toHaveLength(3);
    expect(options[0]?.text()).toContain("42");
    expect(options[1]?.text()).toContain("40");
    expect(options[2]?.text()).toContain("2");
    wrapper.unmount();
  });

  it("当前段带激活态与 aria-pressed", () => {
    setActivePinia(createPinia());
    const wrapper = mountBar("image");

    const options = wrapper.findAll("button.type-filter-option");
    expect(options[2]?.classes()).toContain("type-filter-option-active");
    expect(options[2]?.attributes("aria-pressed")).toBe("true");
    expect(options[0]?.attributes("aria-pressed")).toBe("false");
    wrapper.unmount();
  });

  it("点击某段发出 select 事件（由 store 决定是否重载）", async () => {
    setActivePinia(createPinia());
    const wrapper = mountBar();

    await wrapper.findAll("button.type-filter-option")[2]?.trigger("click");

    expect(wrapper.emitted("select")).toEqual([["image"]]);
    wrapper.unmount();
  });
});
