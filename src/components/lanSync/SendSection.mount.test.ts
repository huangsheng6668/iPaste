import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import type { ClipItem, ClipPage } from "../../types";
import type { DeviceInfo } from "../../types/generated/DeviceInfo";

// 发送区直调 ipasteApi（历史条目分页 / 分类 / 三条发送命令）：mock 域入口后
// 即可断言命令入参，并验证本窗口内的可见反馈（手法同 DeviceListSection 测试）。
vi.mock("../../lib/ipasteApi", () => ({
  ipasteApi: {
    listClips: vi.fn(),
    listCategories: vi.fn(),
    deviceSendClip: vi.fn(),
    deviceSendItems: vi.fn(),
    deviceSendCategory: vi.fn(),
  },
}));

const { ipasteApi } = await import("../../lib/ipasteApi");
const SendSection = (await import("./SendSection.vue")).default;

const listClipsMock = vi.mocked(ipasteApi.listClips);
const listCategoriesMock = vi.mocked(ipasteApi.listCategories);
const deviceSendClipMock = vi.mocked(ipasteApi.deviceSendClip);
const deviceSendItemsMock = vi.mocked(ipasteApi.deviceSendItems);

const nodeId = "ab".repeat(32);

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

function clip(id: string, previewText: string): ClipItem {
  return {
    id,
    clipType: "text",
    contentHash: `hash-${id}`,
    displayName: null,
    previewText,
    text: previewText,
    sourceApp: null,
    lastCapturedAt: "2026-01-01T00:00:00Z",
    favoriteCount: 0,
    isPinned: false,
  };
}

const clips: ClipItem[] = [clip("clip-1", "first snippet"), clip("clip-2", "second snippet")];

function page(items: ClipItem[]): ClipPage {
  return {
    clips: items,
    hasMore: false,
    totalCount: items.length,
    allCount: items.length,
    textCount: items.length,
    imageCount: 0,
  };
}

function mountSection() {
  return mount(SendSection, {
    props: { devices: [device] },
  });
}

async function openHistoryMode(wrapper: ReturnType<typeof mountSection>) {
  await wrapper.findAll(".lan-send-mode")[1]?.trigger("click");
  await flushPromises();
}

describe("SendSection 发送剪贴板", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listClipsMock.mockResolvedValue(page(clips));
    listCategoriesMock.mockResolvedValue([]);
    deviceSendClipMock.mockResolvedValue(undefined);
    deviceSendItemsMock.mockResolvedValue({ sent: clips.length, failed: 0 });
  });

  it("默认发送当前剪贴板，并在面板内回显送达目标", async () => {
    const wrapper = mountSection();
    await flushPromises();

    await wrapper.find("button.lan-button-primary").trigger("click");
    await flushPromises();

    expect(deviceSendClipMock).toHaveBeenCalledWith(null, { kind: "current" });
    const status = wrapper.find(".lan-send-status");
    expect(status.exists()).toBe(true);
    expect(status.text()).toBe("Sent the current clipboard to All online devices");
    expect(status.attributes("role")).toBe("status");
    wrapper.unmount();
  });

  it("历史条目模式可多选并一次发送所选条目", async () => {
    const wrapper = mountSection();
    await flushPromises();
    await openHistoryMode(wrapper);

    const rows = wrapper.findAll(".lan-picker-row");
    expect(rows).toHaveLength(2);
    await rows[0]?.trigger("click");
    expect(wrapper.text()).toContain("1 selected");
    await rows[1]?.trigger("click");
    expect(wrapper.text()).toContain("2 selected");

    await wrapper.find("button.lan-button-primary").trigger("click");
    await flushPromises();

    expect(deviceSendItemsMock).toHaveBeenCalledWith(null, ["clip-1", "clip-2"]);
    expect(wrapper.find(".lan-send-status").text()).toBe(
      "Sent 2 item(s) to All online devices",
    );
    wrapper.unmount();
  });

  it("未选条目时禁用发送，避免空发送", async () => {
    const wrapper = mountSection();
    await flushPromises();
    await openHistoryMode(wrapper);

    expect(wrapper.find("button.lan-button-primary").attributes("disabled")).toBeDefined();
    expect(deviceSendItemsMock).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("发送失败时就地显示错误，不再静默", async () => {
    deviceSendClipMock.mockRejectedValueOnce({ code: "internal", message: "没有在线的目标设备" });
    const wrapper = mountSection();
    await flushPromises();

    await wrapper.find("button.lan-button-primary").trigger("click");
    await flushPromises();

    const status = wrapper.find(".lan-send-status");
    expect(status.classes()).toContain("lan-send-status-error");
    expect(status.attributes("role")).toBe("alert");
    expect(status.text()).toBe("没有在线的目标设备");
    wrapper.unmount();
  });

  it("部分失败时显示已发送与失败条数", async () => {
    deviceSendItemsMock.mockResolvedValueOnce({ sent: 1, failed: 1 });
    const wrapper = mountSection();
    await flushPromises();
    await openHistoryMode(wrapper);

    await wrapper.findAll(".lan-picker-row")[0]?.trigger("click");
    await wrapper.findAll(".lan-picker-row")[1]?.trigger("click");
    await wrapper.find("button.lan-button-primary").trigger("click");
    await flushPromises();

    expect(wrapper.find(".lan-send-status").text()).toBe("Sent 1, 1 failed");
    wrapper.unmount();
  });
});
