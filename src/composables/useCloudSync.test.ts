import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    updateCloudSettings: vi.fn(),
    disableCloudSync: vi.fn(),
    testCloudSettings: vi.fn(),
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
const { useCloudSync } = await import("./useCloudSync");
const { t } = await import("../i18n");

describe("useCloudSync", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("当 store.cloud 已经加载时，初始化立即读取已保存的配置，表单不为空", () => {
    const store = useIpasteStore();
    store.cloud = {
      enabled: true,
      apiAddress: "https://sync.example.com",
      apiKey: "sync-secret-key",
      lastConnectedAt: null,
    };

    const {
      cloudApiAddress,
      cloudApiKey,
      cloudStatusText,
    } = useCloudSync();

    expect(cloudApiAddress.value).toBe("https://sync.example.com");
    expect(cloudApiKey.value).toBe("sync-secret-key");
    expect(cloudStatusText.value).toBe(t("settings.cloud.enabled"));
  });

  it("当 store.cloud 在初始化之后更新时，表单自动响应同步", async () => {
    const store = useIpasteStore();
    const {
      cloudApiAddress,
      cloudApiKey,
    } = useCloudSync();

    expect(cloudApiAddress.value).toBe("");
    expect(cloudApiKey.value).toBe("");

    store.cloud = {
      enabled: true,
      apiAddress: "https://new.example.com",
      apiKey: "new-key",
      lastConnectedAt: null,
    };
    await nextTick();

    expect(cloudApiAddress.value).toBe("https://new.example.com");
    expect(cloudApiKey.value).toBe("new-key");
  });
});
