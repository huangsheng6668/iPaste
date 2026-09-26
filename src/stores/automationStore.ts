import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";
import { filterAutomations } from "./lib/automationFilter";
import type { AutomationAction, AutomationInput } from "../types";

/** 自动化（快捷动作）store：CRUD 与运行日志（Task 13 从 ipasteStore 原样拆出）。 */
export const useAutomationStore = defineStore("automation", () => {
  const automations = ref<AutomationAction[]>([]);
  const selectedActionIndex = ref(0);
  const actionsQuery = ref("");
  const runningAutomationLogs = ref<Record<string, { stdout: string; stderr: string }>>({});

  const visibleActions = computed(() => filterAutomations(automations.value, actionsQuery.value));

  async function loadAutomations() {
    automations.value = await ipasteApi.listAutomations();
  }

  async function createAutomation(input: AutomationInput) {
    await ipasteApi.createAutomation(input);
    await loadAutomations();
  }

  async function updateAutomation(id: string, input: AutomationInput) {
    await ipasteApi.updateAutomation(id, input);
    await loadAutomations();
  }

  async function deleteAutomation(id: string) {
    await ipasteApi.deleteAutomation(id);
    await loadAutomations();
  }

  async function runAutomation(id: string) {
    return await ipasteApi.runAutomation(id);
  }

  return {
    automations,
    selectedActionIndex,
    actionsQuery,
    runningAutomationLogs,
    visibleActions,
    loadAutomations,
    createAutomation,
    updateAutomation,
    deleteAutomation,
    runAutomation,
  };
});
