import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import ErrorToast from "../components/ErrorToast.vue";
import { useUiStore } from "../stores/uiStore";

// 环境冒烟：证明 jsdom + @vue/test-utils + Pinia + 真实 SFC 编译全链路可用。
// 只验证基建，不测业务逻辑；后续组件级测试（计划 Task 26 等）依赖这条链路。
describe("jsdom environment smoke", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("mounts a real SFC wired to a pinia store", async () => {
    const ui = useUiStore();
    const wrapper = mount(ErrorToast);

    // 无 toast 时为空态（根节点 v-if 不渲染）。
    expect(wrapper.find("button").exists()).toBe(false);

    ui.pushToast("boom");
    await wrapper.vm.$nextTick();

    const button = wrapper.find("button");
    expect(button.exists()).toBe(true);
    expect(button.text()).toContain("boom");

    await button.trigger("click");
    expect(ui.toasts).toHaveLength(0);
  });
});
