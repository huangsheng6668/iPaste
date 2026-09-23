import { describe, expect, it, vi } from "vitest";

// format.ts 依赖 i18n，而 i18n 在模块顶层读取 localStorage/document；
// vitest 运行在 node 环境，先桩浏览器全局再动态导入（与 format.i18n.test.ts 同惯例）。
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
});
vi.stubGlobal("document", {
  documentElement: { lang: "en" },
  createElement: () => ({}),
});

const { isCodeText } = await import("./format");

describe("isCodeText", () => {
  it("html 类型直接判定为代码", () => {
    expect(isCodeText("html", "任意")).toBe(true);
  });
  it("代码提示行命中", () => {
    expect(isCodeText("text", "const x = 1;")).toBe(true);
    expect(isCodeText("text", "第一行说明\nSELECT * FROM t;")).toBe(true);
  });
  it("普通文本不命中", () => {
    expect(isCodeText("text", "hello world")).toBe(false);
    expect(isCodeText("color", "#ff0000")).toBe(false);
  });
});
