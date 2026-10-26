import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("../env", () => ({
  isTauri: true,
}));

const { invoke } = await import("@tauri-apps/api/core");
const { settingsApi } = await import("./settings");

describe("settingsApi.saveOpenaiOcrProfile", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("新接口把空 ID 交给后端生成，不在前端预生成 UUID", async () => {
    vi.mocked(invoke).mockResolvedValue({});

    await settingsApi.saveOpenaiOcrProfile({
      id: "",
      name: "新接口",
      baseUrl: "https://api.example.com/v1",
      model: "vision-model",
      apiKey: "sk-test",
    });

    expect(invoke).toHaveBeenCalledWith("save_openai_ocr_profile", {
      profile: {
        id: "",
        name: "新接口",
        baseUrl: "https://api.example.com/v1",
        model: "vision-model",
        apiKey: "sk-test",
      },
      prompts: expect.any(Array),
    });
  });

  it("编辑已有接口时原样传递稳定 ID", async () => {
    vi.mocked(invoke).mockResolvedValue({});

    await settingsApi.saveOpenaiOcrProfile({
      id: "profile-a",
      name: "已有接口",
      baseUrl: "https://api.example.com/v1",
      model: "vision-model",
      apiKey: "sk-test",
    });

    expect(vi.mocked(invoke).mock.calls[0][1]).toEqual({
      profile: expect.objectContaining({ id: "profile-a" }),
      prompts: expect.any(Array),
    });
  });
});
