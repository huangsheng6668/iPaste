import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { nextTick } from "vue";
import type { AppSettings } from "../types";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    updateOpenaiOcrConfig: vi.fn(),
    saveOpenaiOcrProfile: vi.fn(),
    setOpenaiOcrActiveProfile: vi.fn(),
    deleteOpenaiOcrProfile: vi.fn(),
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

const { ipasteApi } = await import("../lib/ipasteApi");
const { useSettingsStore } = await import("../stores/settingsStore");
const { useOpenaiOcr } = await import("./useOpenaiOcr");

function settingsWithCloud(cloudOcr: AppSettings["cloudOcr"]): AppSettings {
  return {
    shortcut: "CommandOrControl+Shift+V",
    ocrShortcut: "CommandOrControl+Shift+O",
    retentionDays: 30,
    appendCopyTimeoutMinutes: 1,
    panelOpenBehavior: "history",
    panelLayout: "top",
    ocrMode: "fast",
    ocrEngine: "openai",
    language: "zh-CN",
    cloud: { apiAddress: "", apiKey: "", enabled: false, lastConnectedAt: null },
    cloudOcr,
  };
}

describe("useOpenaiOcr", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("当 settings.cloudOcr 已经加载时，初始化立即读取已保存的配置，表单不为空", () => {
    const settings = useSettingsStore();
    settings.cloudOcr = {
      openaiProfiles: [{
        id: "profile-a",
        name: "GLM",
        baseUrl: "https://api.openai.com/v1",
        model: "glm-4v-flash",
        apiKey: "sk-test-key-123456",
      }],
      openaiActiveProfileId: "profile-a",
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

  it("当 settings.cloudOcr 在初始化之后更新时，表单自动响应同步", async () => {
    const settings = useSettingsStore();
    const {
      openaiBaseUrl,
      openaiModel,
      openaiApiKey,
      openaiPrompts,
      formComplete,
    } = useOpenaiOcr();

    expect(openaiBaseUrl.value).toBe("");
    expect(formComplete.value).toBe(false);

    settings.cloudOcr = {
      openaiProfiles: [{
        id: "profile-b",
        name: "DeepSeek",
        baseUrl: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        apiKey: "sk-loaded-later",
      }],
      openaiActiveProfileId: "profile-b",
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

  it("本地无鉴权端点留空 API Key 时表单仍可保存/测试", () => {
    const settings = useSettingsStore();
    settings.cloudOcr = {
      openaiProfiles: [{
        id: "local",
        name: "Ollama",
        baseUrl: "http://localhost:11434/v1",
        model: "llava",
        apiKey: "",
      }],
      openaiActiveProfileId: "local",
      openaiBaseUrl: "http://localhost:11434/v1",
      openaiModel: "llava",
      openaiApiKey: "",
      openaiPrompts: [{ role: "system", content: "Extract text." }],
    };

    const { formComplete, openaiConfigured } = useOpenaiOcr();
    expect(formComplete.value).toBe(true);
    expect(openaiConfigured.value).toBe(true);
  });

  it("多个接口时默认编辑主接口，并可切换到其他接口", () => {
    const settings = useSettingsStore();
    settings.cloudOcr = {
      openaiProfiles: [
        { id: "a", name: "接口 A", baseUrl: "https://a.example.com/v1", model: "model-a", apiKey: "key-a" },
        { id: "b", name: "接口 B", baseUrl: "https://b.example.com/v1", model: "model-b", apiKey: "key-b" },
      ],
      openaiActiveProfileId: "b",
      openaiBaseUrl: "https://b.example.com/v1",
      openaiModel: "model-b",
      openaiApiKey: "key-b",
      openaiPrompts: [{ role: "system", content: "Extract text." }],
    };

    const { profiles, selectedProfileId, profileName, openaiModel, selectProfile } = useOpenaiOcr();
    expect(profiles.value).toHaveLength(2);
    expect(selectedProfileId.value).toBe("b");
    expect(profileName.value).toBe("接口 B");

    selectProfile("a");
    expect(selectedProfileId.value).toBe("a");
    expect(profileName.value).toBe("接口 A");
    expect(openaiModel.value).toBe("model-a");
  });

  it("保存新接口后选中后端生成的配置", async () => {
    const settings = useSettingsStore();
    const createdProfile = {
      id: "generated-id",
      name: "新接口",
      baseUrl: "https://new.example.com/v1",
      model: "new-model",
      apiKey: "new-key",
    };
    vi.mocked(ipasteApi.saveOpenaiOcrProfile).mockResolvedValue(settingsWithCloud({
      openaiProfiles: [createdProfile],
      openaiActiveProfileId: "generated-id",
      openaiBaseUrl: createdProfile.baseUrl,
      openaiModel: createdProfile.model,
      openaiApiKey: createdProfile.apiKey,
      openaiPrompts: [{ role: "system", content: "Extract text." }],
    }));

    const {
      profileName,
      openaiBaseUrl,
      openaiModel,
      openaiApiKey,
      selectedProfileId,
      saveOpenaiConfig,
    } = useOpenaiOcr();
    profileName.value = "新接口";
    openaiBaseUrl.value = createdProfile.baseUrl;
    openaiModel.value = createdProfile.model;
    openaiApiKey.value = createdProfile.apiKey;

    await saveOpenaiConfig();
    await nextTick();

    expect(selectedProfileId.value).toBe("generated-id");
    expect(settings.cloudOcr.openaiProfiles).toEqual([createdProfile]);
  });
});
