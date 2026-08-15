import { describe, expect, it } from "vitest";
import { appErrorCode, appErrorParams, errorMessage, isAppError, isCommandMissing } from "./appError";

describe("isAppError", () => {
  it("accepts the serialized AppError shape", () => {
    expect(isAppError({ code: "port_in_use", message: "端口被占用", params: { port: 51777 } })).toBe(true);
  });

  it("rejects strings, errors and nullish values", () => {
    expect(isAppError("port_in_use")).toBe(false);
    expect(isAppError(new Error("x"))).toBe(false);
    expect(isAppError(null)).toBe(false);
    expect(isAppError({ code: 1, message: "x" })).toBe(false);
  });
});

describe("appErrorCode / appErrorParams", () => {
  it("reads code and params from AppError shape", () => {
    const value = { code: "port_in_use", message: "m", params: { port: 51777, name: "a.exe", pid: 9 } };
    expect(appErrorCode(value)).toBe("port_in_use");
    expect(appErrorParams(value)).toEqual({ port: 51777, name: "a.exe", pid: 9 });
  });

  it("returns null for non-AppError values", () => {
    expect(appErrorCode("command x not found")).toBeNull();
    expect(appErrorParams(new Error("x"))).toBeNull();
  });
});

describe("errorMessage", () => {
  it("prefers AppError.message", () => {
    expect(errorMessage({ code: "internal", message: "中文文案" })).toBe("中文文案");
  });

  it("unwraps Error.message", () => {
    expect(errorMessage(new Error("boom"))).toBe("boom");
  });

  it("passes plain strings through", () => {
    expect(errorMessage("command x not found")).toBe("command x not found");
  });

  it("stringifies anything else", () => {
    expect(errorMessage(undefined)).toBe("undefined");
    expect(errorMessage(42)).toBe("42");
  });
});

describe("isCommandMissing", () => {
  it("returns true when the message contains both the command and 'not found'", () => {
    expect(isCommandMissing("update_language command not found", "update_language")).toBe(true);
  });

  it("returns false when the command is missing", () => {
    expect(isCommandMissing("some other command not found", "update_language")).toBe(false);
  });

  it("returns false when 'not found' is missing", () => {
    expect(isCommandMissing("update_language failed with status 500", "update_language")).toBe(false);
  });

  it("stringifies non-string errors", () => {
    expect(isCommandMissing(new Error("update_language not found"), "update_language")).toBe(true);
  });
});
