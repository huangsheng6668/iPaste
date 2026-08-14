import { describe, expect, it, vi } from "vitest";

// i18n.ts reads localStorage/document at module top level; vitest runs in the
// node environment, so stub the browser globals before the dynamic import.
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
});
vi.stubGlobal("document", {
  documentElement: { lang: "en" },
  createElement: () => ({}),
});

const { languageOptions, messages } = await import("./i18n");

describe("settings.cloud.insecureWarning", () => {
  it("存在于全部支持语言", () => {
    expect(languageOptions.length).toBe(7);
    for (const { value } of languageOptions) {
      expect(
        messages[value]["settings.cloud.insecureWarning"],
        `locale ${value} 缺少 insecureWarning`,
      ).toBeTruthy();
    }
  });
});
