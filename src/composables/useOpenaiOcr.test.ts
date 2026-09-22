import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    updateOpenaiOcrConfig: vi.fn(),
    clearOpenaiOcrConfig: vi.fn(),
    testOpenaiOcr: vi.fn(),
  },
}));

vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
});
vi.stubGlobal("document", {
  documentElement: { lang: "zh-CN" },
  createElement: () => ({}),
});

const { useIpasteStore } = await import("../stores/ipasteStore");
const { useOpenaiOcr } = await import("./useOpenaiOcr");

describe("useOpenaiOcr", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("当 store.cloudOcr 已经加载时，初始化立即读取已保存的配置，表单不为空", () => {
    const store = useIpasteStore();
    store.cloudOcr = {
      openaiBaseUrl: "https://api.openai.com/v1",
      openaiModel: "glm-4v-flash",
      openaiApiKey: "sk-test-key-123456",
      openaiPrompts: [{ role: "system", content: "Extract text." }],
    };

    const {
      openaiBaseUrl,
      openaiModel,
      openaiApiKey,
      openaiPrompts,
      formComplete,
      openaiConfigured,
    } = useOpenaiOcr();

    expect(openaiConfigured.value).toBe(true);
    expect(openaiBaseUrl.value).toBe("https://api.openai.com/v1");
    expect(openaiModel.value).toBe("glm-4v-flash");
    expect(openaiApiKey.value).toBe("sk-test-key-123456");
    expect(openaiPrompts.value).toEqual([{ role: "system", content: "Extract text." }]);
    expect(formComplete.value).toBe(true);
  });

  it("当 store.cloudOcr 在初始化之后更新时，表单自动响应同步", async () => {
    const store = useIpasteStore();
    const {
      openaiBaseUrl,
      openaiModel,
      openaiApiKey,
      openaiPrompts,
      formComplete,
    } = useOpenaiOcr();

    expect(openaiBaseUrl.value).toBe("");
    expect(formComplete.value).toBe(false);

    store.cloudOcr = {
      openaiBaseUrl: "https://api.deepseek.com/v1",
      openaiModel: "deepseek-chat",
      openaiApiKey: "sk-loaded-later",
      openaiPrompts: [{ role: "user", content: "OCR please." }],
    };
    await nextTick();

    expect(openaiBaseUrl.value).toBe("https://api.deepseek.com/v1");
    expect(openaiModel.value).toBe("deepseek-chat");
    expect(openaiApiKey.value).toBe("sk-loaded-later");
    expect(openaiPrompts.value).toEqual([{ role: "user", content: "OCR please." }]);
    expect(formComplete.value).toBe(true);
  });
});
