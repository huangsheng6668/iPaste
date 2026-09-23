import { invoke } from "@tauri-apps/api/core";
import type {
  AutomationAction,
  AutomationInput,
  AutomationRunDetail,
  AutomationRunSummary,
} from "../../types";
import { isTauri } from "../env";

let mockAutomations: AutomationAction[] = [];

/** 自动化动作域：shell 快捷动作的 CRUD 与执行记录查询。 */
export const automationsApi = {
  listAutomations() {
    if (!isTauri) return Promise.resolve(structuredClone(mockAutomations));
    return invoke<AutomationAction[]>("list_automations");
  },
  createAutomation(input: AutomationInput) {
    if (!isTauri) {
      const action: AutomationAction = {
        id: `mock-${mockAutomations.length + 1}`,
        ...input,
        runMode: "background",
        sortOrder: mockAutomations.length,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        lastRun: null,
      };
      mockAutomations.push(action);
      return Promise.resolve(action);
    }
    return invoke<AutomationAction>("create_automation", { input });
  },
  updateAutomation(id: string, input: AutomationInput) {
    if (!isTauri) {
      const index = mockAutomations.findIndex((action) => action.id === id);
      if (index < 0) return Promise.reject(new Error("not found"));
      mockAutomations[index] = { ...mockAutomations[index], ...input, updatedAt: new Date().toISOString() };
      return Promise.resolve(structuredClone(mockAutomations[index]));
    }
    return invoke<AutomationAction>("update_automation", { id, input });
  },
  deleteAutomation(id: string) {
    if (!isTauri) {
      mockAutomations = mockAutomations.filter((action) => action.id !== id);
      return Promise.resolve();
    }
    return invoke<void>("delete_automation", { id });
  },
  runAutomation(id: string) {
    if (!isTauri) {
      return Promise.resolve({
        id: `run-${id}`,
        status: "success" as const,
        startedAt: new Date().toISOString(),
        finishedAt: new Date().toISOString(),
        exitCode: 0,
        durationMs: 1,
      });
    }
    return invoke<AutomationRunSummary>("run_automation", { id });
  },
  getAutomationRun(runId: string) {
    if (!isTauri) {
      return Promise.resolve({
        id: runId,
        automationId: "mock",
        status: "success" as const,
        exitCode: 0,
        stdout: "",
        stderr: "",
        stdoutTruncated: false,
        stderrTruncated: false,
        startedAt: new Date().toISOString(),
        finishedAt: new Date().toISOString(),
        durationMs: 1,
      });
    }
    return invoke<AutomationRunDetail>("get_automation_run", { runId });
  },
};
