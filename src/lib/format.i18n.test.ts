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

const { currentLanguage } = await import("../i18n");
const { lineCountText, pluralText } = await import("./format");

describe("lineCountText", () => {
  it("英文按复数形式区分 1 line / N lines", () => {
    currentLanguage.value = "en";
    expect(lineCountText(1)).toBe("1 line");
    expect(lineCountText(3)).toBe("3 lines");
  });

  it("中文不区分复数，始终为 N 行", () => {
    currentLanguage.value = "zh-CN";
    expect(lineCountText(1)).toBe("1 行");
    expect(lineCountText(3)).toBe("3 行");
  });

  it("法语 0 和 1 走单数", () => {
    currentLanguage.value = "fr";
    expect(lineCountText(0)).toBe("0 ligne");
    expect(lineCountText(1)).toBe("1 ligne");
    expect(lineCountText(2)).toBe("2 lignes");
  });
});

describe("pluralText", () => {
  it("按当前语言与数值选择单复数 key", () => {
    currentLanguage.value = "en";
    expect(pluralText("stats.lineOne", "stats.lineOther", 1)).toBe("1 line");
    expect(pluralText("stats.lineOne", "stats.lineOther", 5)).toBe("5 lines");
    currentLanguage.value = "zh-CN";
    expect(pluralText("stats.lineOne", "stats.lineOther", 1)).toBe("1 行");
  });
});
