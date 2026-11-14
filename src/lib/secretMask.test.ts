import { describe, expect, it } from "vitest";
import { MASKED_SECRET, clearIfMasked, isMaskedSecret } from "./secretMask";

describe("secretMask", () => {
  it("识别掩码哨兵（容忍首尾空白）", () => {
    expect(isMaskedSecret(MASKED_SECRET)).toBe(true);
    expect(isMaskedSecret(` ${MASKED_SECRET} `)).toBe(true);
    expect(isMaskedSecret("sk-real")).toBe(false);
    expect(isMaskedSecret("")).toBe(false);
  });

  it("clearIfMasked 仅清空哨兵值", () => {
    expect(clearIfMasked(MASKED_SECRET)).toBe("");
    expect(clearIfMasked("sk-real")).toBe("sk-real");
    expect(clearIfMasked("")).toBe("");
  });
});
