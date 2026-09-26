import { afterEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";
import { mount } from "@vue/test-utils";
import { usePanelDomLifecycle, type PanelDomLifecycleHandlers } from "./usePanelDomLifecycle";

function makeHandlers(): PanelDomLifecycleHandlers {
  return {
    onKeydown: vi.fn(),
    onKeyup: vi.fn(),
    onSelectionChange: vi.fn(),
    onWindowBlur: vi.fn(),
    onVisibilityChange: vi.fn(),
  };
}

function mountWithLifecycle(handlers: PanelDomLifecycleHandlers, enabled: boolean) {
  const Host = defineComponent({
    setup() {
      usePanelDomLifecycle(handlers, { enabled });
      return () => h("div");
    },
  });
  return mount(Host);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("usePanelDomLifecycle", () => {
  it("主面板：挂载时注册 5 个监听（keydown/keyup 走捕获阶段）", () => {
    const documentAdd = vi.spyOn(document, "addEventListener");
    const windowAdd = vi.spyOn(window, "addEventListener");
    const handlers = makeHandlers();

    const wrapper = mountWithLifecycle(handlers, true);

    expect(documentAdd).toHaveBeenCalledWith("keydown", handlers.onKeydown, true);
    expect(documentAdd).toHaveBeenCalledWith("keyup", handlers.onKeyup, true);
    expect(documentAdd).toHaveBeenCalledWith("selectionchange", handlers.onSelectionChange);
    expect(documentAdd).toHaveBeenCalledWith("visibilitychange", handlers.onVisibilityChange);
    expect(windowAdd).toHaveBeenCalledWith("blur", handlers.onWindowBlur);

    wrapper.unmount();
  });

  it("卸载时逐项注销（函数引用与捕获标志成对匹配）", () => {
    const documentRemove = vi.spyOn(document, "removeEventListener");
    const windowRemove = vi.spyOn(window, "removeEventListener");
    const handlers = makeHandlers();

    const wrapper = mountWithLifecycle(handlers, true);
    documentRemove.mockClear();
    windowRemove.mockClear();
    wrapper.unmount();

    expect(documentRemove).toHaveBeenCalledWith("keydown", handlers.onKeydown, true);
    expect(documentRemove).toHaveBeenCalledWith("keyup", handlers.onKeyup, true);
    expect(documentRemove).toHaveBeenCalledWith("selectionchange", handlers.onSelectionChange);
    expect(documentRemove).toHaveBeenCalledWith("visibilitychange", handlers.onVisibilityChange);
    expect(windowRemove).toHaveBeenCalledWith("blur", handlers.onWindowBlur);
  });

  it("辅助窗口（enabled=false）：既不注册也不注销", () => {
    const documentAdd = vi.spyOn(document, "addEventListener");
    const documentRemove = vi.spyOn(document, "removeEventListener");
    const handlers = makeHandlers();

    const wrapper = mountWithLifecycle(handlers, false);
    wrapper.unmount();

    expect(documentAdd).not.toHaveBeenCalled();
    expect(documentRemove).not.toHaveBeenCalled();
  });

  it("监听确实接到事件上（keydown 捕获 + 窗口失焦）", () => {
    const handlers = makeHandlers();
    const wrapper = mountWithLifecycle(handlers, true);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    window.dispatchEvent(new Event("blur"));

    expect(handlers.onKeydown).toHaveBeenCalledTimes(1);
    expect(handlers.onWindowBlur).toHaveBeenCalledTimes(1);

    wrapper.unmount();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
    expect(handlers.onKeydown).toHaveBeenCalledTimes(1);
  });
});
