import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { Category } from "../../types/generated/Category";
import type { DeviceInfo } from "../../types/generated/DeviceInfo";

// 组件内接收选择器对 ipasteApi 直调（fire-and-forget）：mock 域入口后即可
// 断言命令入参（手法同 CommandSearchBar.mount.test.ts）。
vi.mock("../../lib/ipasteApi", () => ({
  ipasteApi: {
    deviceSetRecvSync: vi.fn(),
  },
}));

const { ipasteApi } = await import("../../lib/ipasteApi");
const DeviceListSection = (await import("./DeviceListSection.vue")).default;

const deviceSetRecvSyncMock = vi.mocked(ipasteApi.deviceSetRecvSync);

const nodeId = "ab".repeat(32);

// 一台在线未撤销设备：发送与接收偏好均为默认 text_only。
const device: DeviceInfo = {
  device: {
    nodeId,
    deviceName: "Test Peer",
    relayUrl: null,
    directAddrs: [],
    autoSyncMode: "text_only",
    recvAutoSyncMode: "text_only",
    addedAt: "2026-01-01T00:00:00Z",
    lastSeenAt: null,
    revokedAt: null,
  },
  online: "connected",
};

function mountSection() {
  return mount(DeviceListSection, {
    props: {
      devices: [device],
      categories: [] as Category[],
      confirmingRevokeId: null,
      loadError: null,
      actionError: null,
    },
    global: { plugins: [createPinia()] },
  });
}

// 设备行内两个 select 的固定顺序：先发送偏好（autoSync），后接收偏好（recvSync）。
function rowSelects(wrapper: ReturnType<typeof mountSection>) {
  return wrapper.find("li.lan-device").findAll("select");
}

describe("DeviceListSection 接收模式选择器", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setActivePinia(createPinia());
  });

  it("渲染发送与接收两个 select，接收 select 当前值为 text_only", () => {
    const wrapper = mountSection();

    const selects = rowSelects(wrapper);
    expect(selects).toHaveLength(2);
    const recvSelect = selects[1];
    expect(recvSelect?.element.value).toBe("text_only");
    const optionValues = recvSelect?.findAll("option").map((option) => option.element.value);
    expect(optionValues).toEqual(["text_only", "all", "off"]);
    wrapper.unmount();
  });

  it("change 接收 select 为 off 时调用 device_set_recv_sync(nodeId, off)", async () => {
    const wrapper = mountSection();

    await rowSelects(wrapper)[1]?.setValue("off");

    expect(deviceSetRecvSyncMock).toHaveBeenCalledWith(nodeId, "off");
    wrapper.unmount();
  });
});
