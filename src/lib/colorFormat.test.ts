import { describe, expect, it } from "vitest";
import { contrastText, parseColorFormats, parseRgb } from "./colorFormat";

/** WCAG 对比度，用来断言 contrastText 的选择确实更可读。 */
function contrastRatio(a: string, b: string): number {
  const lum = (color: string) => {
    const [r, g, bl] = parseRgb(color) as [number, number, number, number];
    const channel = (v: number) => {
      const s = v / 255;
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(bl);
  };
  const [l1, l2] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

describe("parseColorFormats", () => {
  it("解析 #rrggbb 为 HEX/RGB/HSL/CSS 四项", () => {
    expect(parseColorFormats("#ff6600")).toEqual([
      { label: "HEX", value: "#FF6600" },
      { label: "RGB", value: "rgb(255, 102, 0)" },
      { label: "HSL", value: "hsl(24, 100%, 50%)" },
      { label: "CSS", value: "color: #FF6600;" },
    ]);
  });
  it("解析 #rgb 短格式（展开为同色 #rrggbb）", () => {
    expect(parseColorFormats("#f60")?.[0]).toEqual({ label: "HEX", value: "#FF6600" });
  });
  it("解析 rgb() 函数格式", () => {
    const formats = parseColorFormats("rgb(255, 102, 0)");
    expect(formats?.[0]).toEqual({ label: "HEX", value: "#FF6600" });
    expect(formats?.[1]).toEqual({ label: "RGB", value: "rgb(255, 102, 0)" });
  });
  it("hsl() 等暂不支持解析的颜色样式回退 RAW/CSS 而非 null", () => {
    expect(parseColorFormats("hsl(24, 100%, 50%)")).toEqual([
      { label: "RAW", value: "hsl(24, 100%, 50%)" },
      { label: "CSS", value: "color: hsl(24, 100%, 50%);" },
    ]);
  });
  it("空串与非颜色文本返回 null", () => {
    expect(parseColorFormats("普通文本")).toBeNull();
    expect(parseColorFormats("")).toBeNull();
    expect(parseColorFormats("   ")).toBeNull();
  });
});

describe("parseRgb", () => {
  it("展开 #rgb 短格式并补 alpha=1", () => {
    expect(parseRgb("#f60")).toEqual([255, 102, 0, 1]);
  });

  it("解析 #rrggbbaa 的 alpha 通道（保留两位小数）", () => {
    expect(parseRgb("#0d948880")).toEqual([13, 148, 136, 0.5]);
  });

  it("解析 rgb()/rgba() 并对通道值截顶到 255", () => {
    expect(parseRgb("rgb(1, 2, 3)")).toEqual([1, 2, 3, 1]);
    expect(parseRgb("rgba(300, 2, 3, 0.4)")).toEqual([255, 2, 3, 0.4]);
  });

  it("hsl() 与非颜色文本返回 null（与 parseColorFormats 一致）", () => {
    expect(parseRgb("hsl(24, 100%, 50%)")).toBeNull();
    expect(parseRgb("普通文本")).toBeNull();
    expect(parseRgb("")).toBeNull();
  });
});

describe("contrastText", () => {
  // 回归护栏：分类色板两端都曾被写死白字，#CA8A04 上只有约 2.9:1。
  it("亮色背景改用近黑前景，而不是写死的白色", () => {
    for (const light of ["#CA8A04", "#65A30D", "#D97706", "#EA580C", "#F59E0B"]) {
      expect(contrastText(light)).toBe("#17181c");
    }
  });

  // 择优不等于「一律转深色」：#A16207 处在明度中段，白字（4.92:1）反而比
  // 近黑（4.27:1）更清晰。这条守住的是「按实际对比度选」，而非某条硬规则。
  it("中间明度色按实际对比度择优", () => {
    expect(contrastText("#A16207")).toBe("#ffffff");
    expect(contrastRatio("#A16207", "#ffffff")).toBeGreaterThan(
      contrastRatio("#A16207", "#17181c"),
    );
  });

  it("暗色背景保持白色前景", () => {
    for (const dark of ["#111827", "#334155", "#475569", "#4F46E5", "#0F766E"]) {
      expect(contrastText(dark)).toBe("#ffffff");
    }
  });

  it("所选前景色一定比另一种候选对比度更高", () => {
    const palette = [
      "#2563EB", "#0891B2", "#0D9488", "#059669", "#65A30D", "#CA8A04",
      "#D97706", "#EA580C", "#DC2626", "#E11D48", "#DB2777", "#C026D3",
      "#9333EA", "#7C3AED", "#4F46E5", "#0284C7", "#475569", "#334155",
      "#111827", "#78716C", "#A16207", "#BE123C", "#6D28D9", "#0F766E",
    ];
    for (const color of palette) {
      const chosen = contrastText(color);
      const other = chosen === "#17181c" ? "#ffffff" : "#17181c";
      expect(contrastRatio(color, chosen)).toBeGreaterThanOrEqual(contrastRatio(color, other));
    }
  });

  it("整块色板的前景对比度都不低于 3:1（非文本可读下限）", () => {
    const palette = [
      "#2563EB", "#0891B2", "#0D9488", "#059669", "#65A30D", "#CA8A04",
      "#D97706", "#EA580C", "#DC2626", "#E11D48", "#DB2777", "#C026D3",
      "#9333EA", "#7C3AED", "#4F46E5", "#0284C7", "#475569", "#334155",
      "#111827", "#78716C", "#A16207", "#BE123C", "#6D28D9", "#0F766E",
    ];
    for (const color of palette) {
      expect(contrastRatio(color, contrastText(color))).toBeGreaterThanOrEqual(3);
    }
  });

  it("无法解析的颜色回退为白色（保持原视觉，不抛错）", () => {
    expect(contrastText("hsl(24, 100%, 50%)")).toBe("#ffffff");
    expect(contrastText("")).toBe("#ffffff");
  });
});
