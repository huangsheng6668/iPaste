import { describe, it, expect } from "vitest";
import {
  DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
  DEFAULT_PANEL_LAYOUT,
  DEFAULT_OCR_MODE,
  DEFAULT_OCR_ENGINE,
  cleanAppendCopyTimeoutMinutes,
  cleanPanelLayout,
  cleanOcrMode,
  cleanOcrEngine,
  cleanOpenaiPrompts,
  cleanCloudOcrSettings,
  DEFAULT_OPENAI_OCR_PROMPTS,
} from "./settings";

describe("cleanAppendCopyTimeoutMinutes", () => {
  it("returns the value when it is one of the allowed options", () => {
    expect(cleanAppendCopyTimeoutMinutes(1)).toBe(1);
    expect(cleanAppendCopyTimeoutMinutes(5)).toBe(5);
    expect(cleanAppendCopyTimeoutMinutes(10)).toBe(10);
  });

  it("falls back to the default for disallowed or non-numeric values", () => {
    expect(cleanAppendCopyTimeoutMinutes(2)).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
    expect(cleanAppendCopyTimeoutMinutes(undefined)).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
    expect(cleanAppendCopyTimeoutMinutes("soon")).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
  });
});

describe("cleanPanelLayout", () => {
  it("passes 'side' through", () => {
    expect(cleanPanelLayout("side")).toBe("side");
  });

  it("falls back to the default for anything else", () => {
    expect(cleanPanelLayout("top")).toBe(DEFAULT_PANEL_LAYOUT);
    expect(cleanPanelLayout("garbage")).toBe(DEFAULT_PANEL_LAYOUT);
    expect(cleanPanelLayout(undefined)).toBe(DEFAULT_PANEL_LAYOUT);
  });
});

describe("cleanOcrMode", () => {
  it("passes 'best' through", () => {
    expect(cleanOcrMode("best")).toBe("best");
  });

  it("falls back to the default for anything else", () => {
    expect(cleanOcrMode("fast")).toBe(DEFAULT_OCR_MODE);
    expect(cleanOcrMode("ultra")).toBe(DEFAULT_OCR_MODE);
    expect(cleanOcrMode(undefined)).toBe(DEFAULT_OCR_MODE);
  });
});

describe("cleanOcrEngine", () => {
  it("passes 'openai' through", () => {
    expect(cleanOcrEngine("openai")).toBe("openai");
  });

  it("falls back to the local engine for anything else", () => {
    expect(cleanOcrEngine("local")).toBe(DEFAULT_OCR_ENGINE);
    expect(cleanOcrEngine(undefined)).toBe(DEFAULT_OCR_ENGINE);
  });
});

describe("cleanOpenaiPrompts", () => {
  it("falls back to default prompts when input is not an array or empty", () => {
    expect(cleanOpenaiPrompts(null)).toEqual(DEFAULT_OPENAI_OCR_PROMPTS);
    expect(cleanOpenaiPrompts(undefined)).toEqual(DEFAULT_OPENAI_OCR_PROMPTS);
    expect(cleanOpenaiPrompts([])).toEqual(DEFAULT_OPENAI_OCR_PROMPTS);
    expect(cleanOpenaiPrompts("not-an-array")).toEqual(DEFAULT_OPENAI_OCR_PROMPTS);
  });

  it("preserves valid prompt messages and normalizes roles and contents", () => {
    const custom = [
      { role: "system", content: "System prompt" },
      { role: "user", content: "User prompt" },
      { role: "unknown", content: "Unknown role prompt" },
    ];
    expect(cleanOpenaiPrompts(custom)).toEqual([
      { role: "system", content: "System prompt" },
      { role: "user", content: "User prompt" },
      { role: "user", content: "Unknown role prompt" },
    ]);
  });
});

describe("cleanCloudOcrSettings", () => {
  it("normalizes incomplete or invalid object input", () => {
    expect(cleanCloudOcrSettings(null)).toEqual({
      openaiBaseUrl: "",
      openaiModel: "",
      openaiApiKey: "",
      openaiPrompts: DEFAULT_OPENAI_OCR_PROMPTS,
    });
  });

  it("preserves valid fields and cleans prompts", () => {
    const raw = {
      openaiBaseUrl: "https://api.example.com",
      openaiModel: "custom-model",
      openaiApiKey: "sk-test",
      openaiPrompts: [{ role: "user", content: "Extract text" }],
    };
    expect(cleanCloudOcrSettings(raw)).toEqual({
      openaiBaseUrl: "https://api.example.com",
      openaiModel: "custom-model",
      openaiApiKey: "sk-test",
      openaiPrompts: [{ role: "user", content: "Extract text" }],
    });
  });
});
