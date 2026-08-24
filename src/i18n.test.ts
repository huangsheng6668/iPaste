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
type Language = (typeof languageOptions)[number]["value"];

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

describe("i18n completeness", () => {
  it("全部语言与 en 的 key 集合完全对齐（无缺失、无多余）", () => {
    const enKeys = Object.keys(messages.en).sort();
    expect(enKeys.length).toBeGreaterThan(0);
    for (const { value } of languageOptions) {
      const localeKeys = Object.keys(messages[value]).sort();
      expect(
        localeKeys,
        `locale ${value} 与 en 的 key 集合不一致：缺失 [${enKeys.filter((key) => !localeKeys.includes(key)).join(", ")}]，多余 [${localeKeys.filter((key) => !enKeys.includes(key)).join(", ")}]`,
      ).toEqual(enKeys);
    }
  });

  it("占位符参数在各语言间一致（如 {time}）", () => {
    const asRecord = (locale: string) => messages[locale as Language] as unknown as Record<string, string>;
    const placeholders = (value: string) => (value.match(/\{[a-zA-Z]+\}/g) ?? []).sort().join(",");
    for (const key of Object.keys(messages.en)) {
      const expected = placeholders(asRecord("en")[key]);
      for (const { value } of languageOptions) {
        expect(
          placeholders(asRecord(value)[key]),
          `locale ${value} 的 ${key} 占位符与 en 不一致`,
        ).toBe(expected);
      }
    }
  });
});
