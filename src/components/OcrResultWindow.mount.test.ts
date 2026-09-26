import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    snapshot: vi.fn(),
    listClips: vi.fn(),
    getOcrResultPayload: vi.fn(),
    recognizeImageText: vi.fn(),
    copyClip: vi.fn(),
    openClipViewer: vi.fn(),
  },
}));

const { ipasteApi } = await import("../lib/ipasteApi");
const OcrResultWindow = (await import("./OcrResultWindow.vue")).default;

const payloadMock = vi.mocked(ipasteApi.getOcrResultPayload);
const recognizeMock = vi.mocked(ipasteApi.recognizeImageText);

// 被守护的 bug：截图 OCR 结果窗在 catch 里把错误整个丢掉、只显示
// 「识别失败，请在设置中检查 OCR 资源。」。于是「额度用尽 / 限流 / 凭据过期」
// 这类与设置无关的失败，全都被说成配置问题——用户会去查一个本来没问题的配置。
// 这里断言真实原因必须到达 DOM。

function mountWindow() {
  return mount(OcrResultWindow, { global: { plugins: [createPinia()] } });
}

describe("OcrResultWindow 失败原因回显", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setActivePinia(createPinia());
    // 组件从 window.location.search 取 token，jsdom 默认没有查询串
    window.history.replaceState({}, "", "/?window=ocr-result&token=probe");
    payloadMock.mockResolvedValue({
      imagePath: "C:/frames/probe.png",
      itemId: "clip-1",
      monitorIndex: 0,
    });
  });

  it("识别失败时把服务商返回的原因原样显示", async () => {
    const reason =
      "OpenAI 兼容接口调用失败：已达到 5 小时的使用限额。限额重置将在 2026-09-27 03:48:58 启用。";
    recognizeMock.mockRejectedValue(reason);

    const wrapper = mountWindow();
    await flushPromises();

    const detail = wrapper.find(".ocr-result-state-detail");
    expect(detail.exists()).toBe(true);
    expect(detail.text()).toContain("已达到 5 小时的使用限额");
    // 通用文案保留（它仍是对「本地资源缺失」这类失败的正当引导），
    // 且真实原因必须是同一失败区块里的第二条——不依赖当前界面语言做断言。
    const errorBlock = wrapper.find(".ocr-result-state-error");
    expect(errorBlock.exists()).toBe(true);
    expect(errorBlock.findAll("p")).toHaveLength(2);
    wrapper.unmount();
  });

  it("识别成功时不显示失败原因区块", async () => {
    recognizeMock.mockResolvedValue({ text: "hello", language: "en", engine: "local", words: [] });

    const wrapper = mountWindow();
    await flushPromises();

    expect(wrapper.find(".ocr-result-state-detail").exists()).toBe(false);
    wrapper.unmount();
  });
});
