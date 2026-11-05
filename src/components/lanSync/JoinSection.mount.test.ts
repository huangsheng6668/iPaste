import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import JoinSection from "./JoinSection.vue";
import type { NearbyDevice } from "../../types/generated/NearbyDevice";

const devices: NearbyDevice[] = [
  { deviceName: "MacBook Pro", endpointId: "a".repeat(64) },
];

function mountSection(
  nearby: NearbyDevice[] = [],
  selectedEndpointId: string | null = null,
  pin = "",
) {
  return mount(JoinSection, {
    props: {
      errorText: null,
      modelValue: "",
      nearby,
      selectedEndpointId,
      pin,
    },
  });
}

describe("JoinSection 附近设备 + PIN", () => {
  it("无设备时渲染空态且无条目", () => {
    const wrapper = mountSection();
    expect(wrapper.find(".lan-empty").exists()).toBe(true);
    expect(wrapper.findAll(".lan-nearby-item")).toHaveLength(0);
    wrapper.unmount();
  });

  it("渲染设备条目并在点击时上抛选中 id", async () => {
    const wrapper = mountSection(devices);
    expect(wrapper.findAll(".lan-nearby-item")).toHaveLength(1);
    await wrapper.find(".lan-nearby-item").trigger("click");
    expect(wrapper.emitted("update:selectedEndpointId")?.[0]).toEqual([devices[0]?.endpointId]);
    wrapper.unmount();
  });

  it("选中设备带激活态与 aria-selected", () => {
    const wrapper = mountSection(devices, devices[0]?.endpointId ?? null);
    const item = wrapper.find(".lan-nearby-item");
    expect(item.classes()).toContain("lan-nearby-item-selected");
    expect(item.attributes("aria-selected")).toBe("true");
    wrapper.unmount();
  });

  it("PIN 按钮仅在选中设备且 6 位数字时可用", async () => {
    const wrapper = mountSection(devices, devices[0]?.endpointId ?? null, "12345");
    expect(wrapper.find("#lan-pin-join").attributes("disabled")).toBeDefined();
    await wrapper.find("#lan-pin-input").setValue("123456");
    expect(wrapper.emitted("update:pin")?.[0]).toEqual(["123456"]);
    // 子组件是受控的：由父级回写 props，按钮才会重新计算禁用态。
    await wrapper.setProps({ pin: "123456" });
    expect(wrapper.find("#lan-pin-join").attributes("disabled")).toBeUndefined();
    await wrapper.find("#lan-pin-join").trigger("click");
    expect(wrapper.emitted("joinNearby")).toHaveLength(1);
    wrapper.unmount();
  });

  it("票据粘贴框仍保留在折叠区内", () => {
    const wrapper = mountSection();
    expect(wrapper.find("#lan-join-input").exists()).toBe(true);
    wrapper.unmount();
  });
});
