import { describe, expect, it } from "vitest";
import { CATEGORY_COLOR_OPTIONS, categoryColorVars } from "./categoryColors";
import { contrastText } from "./colorFormat";

describe("CATEGORY_COLOR_OPTIONS", () => {
  it("保留 24 个唯一色值", () => {
    expect(CATEGORY_COLOR_OPTIONS).toHaveLength(24);
    expect(new Set(CATEGORY_COLOR_OPTIONS).size).toBe(24);
  });

  it("全部是六位 HEX", () => {
    for (const color of CATEGORY_COLOR_OPTIONS) {
      expect(color).toMatch(/^#[0-9A-F]{6}$/);
    }
  });
});

describe("categoryColorVars", () => {
  it("派生原始色、自动前景与同色系弱底/描边", () => {
    expect(categoryColorVars("#0D9488")).toEqual({
      "--category-color": "#0D9488",
      "--category-on-color": contrastText("#0D9488"),
      "--category-soft": "rgba(13, 148, 136, 0.12)",
      "--category-ring": "rgba(13, 148, 136, 0.36)",
    });
  });

  it("前景色落在近黑或白两端", () => {
    for (const color of CATEGORY_COLOR_OPTIONS) {
      expect(["#17181c", "#ffffff"]).toContain(categoryColorVars(color)["--category-on-color"]);
    }
  });

  it("无法解析的颜色只保留原值，不生成半透明派生色", () => {
    expect(categoryColorVars("not-a-color")).toEqual({
      "--category-color": "not-a-color",
      "--category-on-color": "#ffffff",
      "--category-soft": "transparent",
      "--category-ring": "transparent",
    });
  });
});
