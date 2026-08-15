import { describe, expect, it } from "vitest";
import { filterAutomations } from "./automationFilter";
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

describe("filterAutomations", () => {
  it("empty query returns all", () => {
    const all = [action("pull", "git pull"), action("deploy", "npm run deploy")];
    expect(filterAutomations(all, "")).toHaveLength(2);
  });

  it("matches by name", () => {
    const all = [action("pull", "git pull"), action("deploy", "npm run deploy")];
    expect(filterAutomations(all, "pull")).toHaveLength(1);
  });

  it("matches by name or command", () => {
    const all = [action("pull", "git pull"), action("deploy", "npm run deploy")];
    expect(filterAutomations(all, "deploy")).toHaveLength(1);
    expect(filterAutomations(all, "git")).toHaveLength(1);
  });

  it("matches by cwd", () => {
    const all = [action("a", "echo 1", "/tmp/proj"), action("b", "echo 2")];
    expect(filterAutomations(all, "/tmp")).toHaveLength(1);
  });

  it("is case-insensitive", () => {
    const all = [action("Pull", "git pull")];
    expect(filterAutomations(all, "PULL")).toHaveLength(1);
  });
});
