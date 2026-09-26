import { describe, expect, it } from "vitest";
import { currentWindowKind } from "./useWindowRoute";

describe("currentWindowKind", () => {
  it("无 window 参数时为主面板", () => {
    expect(currentWindowKind("")).toBe("main");
    expect(currentWindowKind("?foo=bar")).toBe("main");
  });

  it("5 个辅助窗口各自解析正确", () => {
    expect(currentWindowKind("?window=settings")).toBe("settings");
    expect(currentWindowKind("?window=clip-viewer")).toBe("clip-viewer");
    expect(currentWindowKind("?window=lan-sync")).toBe("lan-sync");
    expect(currentWindowKind("?window=ocr-overlay")).toBe("ocr-overlay");
    expect(currentWindowKind("?window=ocr-result")).toBe("ocr-result");
  });

  it("未知值回落主面板（不抛出、不误判为辅助窗口）", () => {
    expect(currentWindowKind("?window=bogus")).toBe("main");
    expect(currentWindowKind("?window=")).toBe("main");
    expect(currentWindowKind("?window=main")).toBe("main");
  });

  it("与其他查询参数共存时仍能解析，且不区分参数顺序", () => {
    expect(currentWindowKind("?label=viewer-1&window=clip-viewer&auto-recognize=1")).toBe("clip-viewer");
    expect(currentWindowKind("?window=ocr-result&monitor=2")).toBe("ocr-result");
  });

  it("大小写敏感（与原实现逐字比较一致）", () => {
    expect(currentWindowKind("?window=Settings")).toBe("main");
  });
});
