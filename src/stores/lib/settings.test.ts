import { describe, it, expect } from "vitest";
import {
  DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
  DEFAULT_PANEL_LAYOUT,
  DEFAULT_OCR_MODE,
  cleanAppendCopyTimeoutMinutes,
  cleanPanelLayout,
  cleanOcrMode,
  isSettingsCommandMissing,
} from "./settings";

describe("cleanAppendCopyTimeoutMinutes", () => {
  it("returns the value when it is one of the allowed options", () => {
    expect(cleanAppendCopyTimeoutMinutes(1)).toBe(1);
    expect(cleanAppendCopyTimeoutMinutes(5)).toBe(5);
    expect(cleanAppendCopyTimeoutMinutes(10)).toBe(10);
  });

  it("falls back to the default for disallowed or non-numeric values", () => {
    expect(cleanAppendCopyTimeoutMinutes(2)).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
    expect(cleanAppendCopyTimeoutMinutes(undefined)).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
    expect(cleanAppendCopyTimeoutMinutes("soon")).toBe(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
  });
});

describe("cleanPanelLayout", () => {
  it("passes 'side' through", () => {
    expect(cleanPanelLayout("side")).toBe("side");
  });

  it("falls back to the default for anything else", () => {
    expect(cleanPanelLayout("top")).toBe(DEFAULT_PANEL_LAYOUT);
    expect(cleanPanelLayout("garbage")).toBe(DEFAULT_PANEL_LAYOUT);
    expect(cleanPanelLayout(undefined)).toBe(DEFAULT_PANEL_LAYOUT);
  });
});

describe("cleanOcrMode", () => {
  it("passes 'best' through", () => {
    expect(cleanOcrMode("best")).toBe("best");
  });

  it("falls back to the default for anything else", () => {
    expect(cleanOcrMode("fast")).toBe(DEFAULT_OCR_MODE);
    expect(cleanOcrMode("ultra")).toBe(DEFAULT_OCR_MODE);
    expect(cleanOcrMode(undefined)).toBe(DEFAULT_OCR_MODE);
  });
});

describe("isSettingsCommandMissing", () => {
  it("returns true when the message contains both the command and 'not found'", () => {
    expect(isSettingsCommandMissing("update_language command not found", "update_language")).toBe(true);
  });

  it("returns false when the command is missing", () => {
    expect(isSettingsCommandMissing("some other command not found", "update_language")).toBe(false);
  });

  it("returns false when 'not found' is missing", () => {
    expect(isSettingsCommandMissing("update_language failed with status 500", "update_language")).toBe(false);
  });

  it("stringifies non-string errors", () => {
    expect(isSettingsCommandMissing(new Error("update_language not found"), "update_language")).toBe(true);
  });
});
