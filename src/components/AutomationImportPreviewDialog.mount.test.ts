import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AutomationInput } from "../types";

const AutomationImportPreviewDialog = (await import("./AutomationImportPreviewDialog.vue")).default;

function item(name: string, command: string): AutomationInput {
  return { name, command, cwd: null, confirmBeforeRun: true, closePanelOnSuccess: false };
}

function mountDialog(open = true, items: AutomationInput[] = [item("pull", "git pull"), item("ping", "ping example.com")]) {
  return mount(AutomationImportPreviewDialog, {
    props: { open, items },
    // 对话框内容 Teleport 到 body；VTU 的 findAll 只查 wrapper.element，
    // stub 掉 teleport 让内容原地渲染，断言才能命中。
    global: { plugins: [createPinia()], stubs: { teleport: true } },
  });
}

describe("AutomationImportPreviewDialog 导入预览", () => {
  it("逐条渲染名称与完整命令", () => {
    setActivePinia(createPinia());
    const wrapper = mountDialog();
    const items = wrapper.findAll("li");
    expect(items).toHaveLength(2);
    expect(items[0]?.text()).toContain("pull");
    expect(items[0]?.text()).toContain("git pull");
    wrapper.unmount();
  });

  it("open=false 时不渲染", () => {
    setActivePinia(createPinia());
    const wrapper = mountDialog(false);
    expect(wrapper.find("li").exists()).toBe(false);
    wrapper.unmount();
  });

  it("取消与确认分别发出 cancel 与 confirm 事件", async () => {
    setActivePinia(createPinia());
    const wrapper = mountDialog();
    const buttons = wrapper.findAll("button");
    await buttons[0]?.trigger("click");
    await buttons[1]?.trigger("click");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("confirm")).toHaveLength(1);
    wrapper.unmount();
  });
});
