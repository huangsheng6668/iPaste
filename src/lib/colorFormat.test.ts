import { describe, expect, it } from "vitest";
import { parseColorFormats } from "./colorFormat";

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
