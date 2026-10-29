import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { AppSettings } from "../types";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    snapshot: vi.fn(),
    updateOcrEngine: vi.fn(),
    setOpenaiOcrActiveProfile: vi.fn(),
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

const { ipasteApi } = await import("../lib/ipasteApi");
const { useSettingsStore } = await import("../stores/settingsStore");
const { useOcrEngineSelect } = await import("./useOcrEngineSelect");

function buildSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    shortcut: "CommandOrControl+Shift+V",
    ocrShortcut: "CommandOrControl+Shift+O",
    retentionDays: 30,
    appendCopyTimeoutMinutes: 1,
    panelOpenBehavior: "history",
    panelLayout: "top",
    ocrMode: "fast",
    ocrEngine: "local",
    language: "zh-CN",
    cloud: { apiAddress: "", apiKey: "", enabled: false, lastConnectedAt: null },
    cloudOcr: {
      openaiProfiles: [],
      openaiActiveProfileId: "",
      openaiBaseUrl: "",
      openaiModel: "",
      openaiApiKey: "",
      openaiPrompts: [],
    },
    ...overrides,
  };
}

describe("useOcrEngineSelect", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("0 或 1 个有效接口时保持 local 与 openai 两项", () => {
    const settings = useSettingsStore();
    const { ocrEngineOptions, selectedEngineValue } = useOcrEngineSelect();

    expect(ocrEngineOptions.value).toHaveLength(2);
    expect(ocrEngineOptions.value[0].value).toBe("local");
    expect(ocrEngineOptions.value[1].value).toBe("openai");
    expect(ocrEngineOptions.value[1].ready).toBe(false);
    expect(selectedEngineValue.value).toBe("local");

    settings.applySettings(
      buildSettings({
        ocrEngine: "openai",
        cloudOcr: {
          openaiProfiles: [
            { id: "p1", name: "GLM", baseUrl: "https://a.example.com/v1", model: "glm-4v", apiKey: "k1" },
          ],
          openaiActiveProfileId: "p1",
          openaiBaseUrl: "https://a.example.com/v1",
          openaiModel: "glm-4v",
          openaiApiKey: "k1",
          openaiPrompts: [],
        },
      }),
    );

    expect(ocrEngineOptions.value).toHaveLength(2);
    expect(ocrEngineOptions.value[1].value).toBe("openai");
    expect(ocrEngineOptions.value[1].ready).toBe(true);
    expect(selectedEngineValue.value).toBe("openai");
  });

  it("2 个及以上有效接口时在下拉框内展开各接口并支持一键切换主接口与引擎", async () => {
    const settings = useSettingsStore();
    const twoProfiles = [
      { id: "fast", name: "GLM Flash", baseUrl: "https://a.example.com/v1", model: "glm-4v-flash", apiKey: "k1" },
      { id: "pro", name: "GPT-4o", baseUrl: "https://b.example.com/v1", model: "gpt-4o", apiKey: "k2" },
    ];
    settings.applySettings(
      buildSettings({
        ocrEngine: "local",
        cloudOcr: {
          openaiProfiles: twoProfiles,
          openaiActiveProfileId: "fast",
          openaiBaseUrl: twoProfiles[0].baseUrl,
          openaiModel: twoProfiles[0].model,
          openaiApiKey: twoProfiles[0].apiKey,
          openaiPrompts: [],
        },
      }),
    );

    const { ocrEngineOptions, selectedEngineValue, switchOcrEngine } = useOcrEngineSelect();
    expect(ocrEngineOptions.value.map((o) => o.value)).toEqual([
      "local",
      "openai:fast",
      "openai:pro",
    ]);
    expect(ocrEngineOptions.value[1].label).toContain("GLM Flash");
    expect(ocrEngineOptions.value[2].label).toContain("GPT-4o");
    expect(selectedEngineValue.value).toBe("local");

    vi.mocked(ipasteApi.setOpenaiOcrActiveProfile).mockResolvedValue(
      buildSettings({
        ocrEngine: "local",
        cloudOcr: {
          openaiProfiles: twoProfiles,
          openaiActiveProfileId: "pro",
          openaiBaseUrl: twoProfiles[1].baseUrl,
          openaiModel: twoProfiles[1].model,
          openaiApiKey: twoProfiles[1].apiKey,
          openaiPrompts: [],
        },
      }),
    );
    vi.mocked(ipasteApi.updateOcrEngine).mockResolvedValue(
      buildSettings({
        ocrEngine: "openai",
        cloudOcr: {
          openaiProfiles: twoProfiles,
          openaiActiveProfileId: "pro",
          openaiBaseUrl: twoProfiles[1].baseUrl,
          openaiModel: twoProfiles[1].model,
          openaiApiKey: twoProfiles[1].apiKey,
          openaiPrompts: [],
        },
      }),
    );

    const nextEngine = await switchOcrEngine("openai:pro");
    expect(nextEngine).toBe("openai");
    expect(ipasteApi.setOpenaiOcrActiveProfile).toHaveBeenCalledWith("pro");
    expect(ipasteApi.updateOcrEngine).toHaveBeenCalledWith("openai");
    expect(selectedEngineValue.value).toBe("openai:pro");
  });
});
