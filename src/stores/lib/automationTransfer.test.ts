import { describe, expect, it } from "vitest";
import { serializeAutomations, parseImportFile } from "./automationTransfer";
import type { AutomationAction } from "../../types";

function action(name: string, command: string, cwd?: string): AutomationAction {
  return {
    id: name,
    name,
    command,
    cwd: cwd ?? null,
    runMode: "background",
    confirmBeforeRun: false,
    closePanelOnSuccess: false,
    sortOrder: 0,
    createdAt: "",
    updatedAt: "",
    lastRun: null,
  };
}

describe("serializeAutomations", () => {
  it("produces valid JSON with version and exportedAt", () => {
    const json = serializeAutomations([action("pull", "git pull")]);
    const parsed = JSON.parse(json);
    expect(parsed.version).toBe(1);
    expect(parsed.exportedAt).toBeTruthy();
    expect(parsed.automations).toHaveLength(1);
    expect(parsed.automations[0].name).toBe("pull");
  });

  it("excludes id, sortOrder, lastRun", () => {
    const json = serializeAutomations([action("a", "echo 1")]);
    const parsed = JSON.parse(json);
    expect(parsed.automations[0].id).toBeUndefined();
    expect(parsed.automations[0].sortOrder).toBeUndefined();
    expect(parsed.automations[0].lastRun).toBeUndefined();
  });
});

describe("parseImportFile", () => {
  it("parses valid automations", () => {
    const json = JSON.stringify({ version: 1, automations: [{ name: "a", command: "echo 1" }] });
    const result = parseImportFile(json, new Set());
    expect(result.valid).toHaveLength(1);
    expect(result.valid[0].name).toBe("a");
    expect(result.skippedDuplicates).toBe(0);
    expect(result.skippedInvalid).toBe(0);
  });

  it("skips duplicates by name", () => {
    const json = JSON.stringify({ automations: [{ name: "a", command: "echo 1" }] });
    const result = parseImportFile(json, new Set(["a"]));
    expect(result.valid).toHaveLength(0);
    expect(result.skippedDuplicates).toBe(1);
  });

  it("skips invalid entries", () => {
    const json = JSON.stringify({ automations: [{ name: "", command: "echo" }, { name: "ok", command: "" }] });
    const result = parseImportFile(json, new Set());
    expect(result.valid).toHaveLength(0);
    expect(result.skippedInvalid).toBe(2);
  });

  it("throws on invalid JSON", () => {
    expect(() => parseImportFile("not json", new Set())).toThrow("invalid-json");
  });

  it("throws when automations array missing", () => {
    expect(() => parseImportFile("{}", new Set())).toThrow("no-automations-array");
  });

  it("defaults confirmBeforeRun and closePanelOnSuccess to false", () => {
    const json = JSON.stringify({ automations: [{ name: "a", command: "echo 1" }] });
    const result = parseImportFile(json, new Set());
    expect(result.valid[0].confirmBeforeRun).toBe(false);
    expect(result.valid[0].closePanelOnSuccess).toBe(false);
  });
});
