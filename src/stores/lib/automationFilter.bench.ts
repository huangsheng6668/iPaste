import { bench, describe } from "vitest";
import { filterAutomations } from "./automationFilter";
import type { AutomationAction } from "../../types";

function mockActions(n: number): AutomationAction[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `action-${i}`,
    name: `action ${i}`,
    command: `git pull --depth ${i}`,
    cwd: `/tmp/repo-${i % 10}`,
    runMode: "background",
    confirmBeforeRun: false,
    closePanelOnSuccess: false,
    sortOrder: i,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    lastRun: null,
  }));
}

describe("automationFilter", () => {
  bench("filter 100 actions, matching query", () => {
    const actions = mockActions(100);
    filterAutomations(actions, "git");
  });
  bench("filter 500 actions, no match", () => {
    const actions = mockActions(500);
    filterAutomations(actions, "zzznomatch");
  });
});
