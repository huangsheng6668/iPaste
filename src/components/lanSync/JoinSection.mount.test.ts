import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import JoinSection from "./JoinSection.vue";
import type { NearbyDevice } from "../../types/generated/NearbyDevice";

const devices: NearbyDevice[] = [
  { deviceName: "MacBook Pro", endpointId: "a".repeat(64) },
];

// 确认层用例：两台设备指纹可区分，选中第二台验证指纹取自所选条目。
const confirmDevices: NearbyDevice[] = [
  { deviceName: "Alice 的 iMac", endpointId: `aaaaaaaa${"0".repeat(56)}` },
  { deviceName: "Bob 的 Mac mini", endpointId: `feedface${"1".repeat(56)}` },
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
    // 点击行为改经确认层，由下方「配对确认层」用例覆盖。
    wrapper.unmount();
  });

  it("票据粘贴框仍保留在折叠区内", () => {
    const wrapper = mountSection();
    expect(wrapper.find("#lan-join-input").exists()).toBe(true);
    wrapper.unmount();
  });
});

describe("JoinSection 配对确认层", () => {
  it("点击配对先弹确认层，展示所选设备名与指纹且不 emit joinNearby", async () => {
    const wrapper = mountSection(confirmDevices, confirmDevices[1]?.endpointId ?? null, "123456");
    await wrapper.find("#lan-pin-join").trigger("click");
    const dialog = wrapper.find(".dialog-panel");
    expect(dialog.exists()).toBe(true);
    // 指纹 = 所选条目 endpointId 前 8 字符（选中的是第二台设备）。
    expect(dialog.text()).toContain("feedface");
    expect(dialog.text()).not.toContain("aaaaaaaa");
    expect(dialog.text()).toContain("Bob 的 Mac mini");
    expect(wrapper.emitted("joinNearby")).toBeUndefined();
    wrapper.unmount();
  });

  it("确认层点确认后 emit joinNearby 一次并关闭", async () => {
    const wrapper = mountSection(confirmDevices, confirmDevices[1]?.endpointId ?? null, "123456");
    await wrapper.find("#lan-pin-join").trigger("click");
    await wrapper.find(".dialog-panel .btn-primary").trigger("click");
    expect(wrapper.emitted("joinNearby")).toHaveLength(1);
    expect(wrapper.find(".dialog-panel").exists()).toBe(false);
    wrapper.unmount();
  });

  it("确认层点取消不 emit，关闭后可重新发起", async () => {
    const wrapper = mountSection(confirmDevices, confirmDevices[1]?.endpointId ?? null, "123456");
    await wrapper.find("#lan-pin-join").trigger("click");
    await wrapper.find(".dialog-panel .btn-ghost").trigger("click");
    expect(wrapper.emitted("joinNearby")).toBeUndefined();
    expect(wrapper.find(".dialog-panel").exists()).toBe(false);
    // 可重新发起：再次点击配对仍能打开确认层。
    await wrapper.find("#lan-pin-join").trigger("click");
    expect(wrapper.find(".dialog-panel").exists()).toBe(true);
    wrapper.unmount();
  });
});
