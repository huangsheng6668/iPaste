import { describe, expect, it } from "vitest";
import {
  MASKED_SECRET,
  clearIfMasked,
  isMaskedSecret,
  restoreSentinelIfEmpty,
} from "./secretMask";

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

  it("restoreSentinelIfEmpty 空值回填哨兵（blur 防误清已存凭据）", () => {
    expect(restoreSentinelIfEmpty("")).toBe(MASKED_SECRET);
    expect(restoreSentinelIfEmpty("   ")).toBe(MASKED_SECRET);
  });

  it("restoreSentinelIfEmpty 非空值原样返回", () => {
    expect(restoreSentinelIfEmpty("sk-real")).toBe("sk-real");
    expect(restoreSentinelIfEmpty(MASKED_SECRET)).toBe(MASKED_SECRET);
  });
});
