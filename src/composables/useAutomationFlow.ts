import { ref, watch } from "vue";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { serializeAutomations, parseImportFile } from "../stores/lib/automationTransfer";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { AutomationAction, AutomationInput } from "../types";

type IpasteStore = ReturnType<typeof useIpasteStore>;

/**
 * 自动化页签的交互流程（原 App.vue automation 段）：运行/确认、编辑器、详情、
 * 右键菜单、导入导出与 actions 页键盘导航。搬家逻辑零改动：store 经参数注入，
 * 面板隐藏经 hidePanelFromUi 注入以避免回环依赖。
 */
export function useAutomationFlow(store: IpasteStore, hidePanelFromUi: () => Promise<void>) {
  const automationEditorOpen = ref(false);
  const automationEditorAction = ref<AutomationAction | null>(null);
  const automationConfirmOpen = ref(false);
  const automationConfirmAction = ref<AutomationAction | null>(null);
  const automationContextMenu = ref<{ action: AutomationAction; x: number; y: number } | null>(null);
  const automationDetailOpen = ref(false);
  const automationDetailAction = ref<AutomationAction | null>(null);
  const importFileInput = ref<HTMLInputElement | null>(null);

  function handleActionsKey(key: string): boolean {
    if (key === "ArrowDown") {
      store.selectedActionIndex = Math.min(store.selectedActionIndex + 1, Math.max(store.visibleActions.length - 1, 0));
      return true;
    }
    if (key === "ArrowUp") {
      store.selectedActionIndex = Math.max(store.selectedActionIndex - 1, 0);
      return true;
    }
    if (key === "Enter") {
      const action = store.visibleActions[store.selectedActionIndex];
      if (action) void runSelectedAction(action);
      return true;
    }
    if (key === "Escape") {
      store.selectCategory("history");
      return true;
    }
    return false;
  }

  function selectActionCard(index: number) {
    store.selectedActionIndex = index;
  }

  function runSelectedAction(action: AutomationAction) {
    if (action.confirmBeforeRun) {
      automationConfirmAction.value = action;
      automationConfirmOpen.value = true;
      return;
    }
    void executeAutomation(action);
  }

  async function executeAutomation(action: AutomationAction) {
    try {
      await store.runAutomation(action.id);
    } catch (unknownError) {
      console.error("automation run failed", unknownError);
    }
  }

  function openAutomationEditor(action: AutomationAction | null) {
    automationEditorAction.value = action;
    automationEditorOpen.value = true;
  }

  async function saveAutomation(input: AutomationInput) {
    try {
      if (automationEditorAction.value) {
        await store.updateAutomation(automationEditorAction.value.id, input);
      } else {
        await store.createAutomation(input);
      }
    } catch (unknownError) {
      console.error("automation save failed", unknownError);
    }
    automationEditorOpen.value = false;
  }

  async function deleteAutomationAction(action: AutomationAction) {
    try {
      await store.deleteAutomation(action.id);
    } catch (unknownError) {
      console.error("automation delete failed", unknownError);
    }
  }

  async function copyAutomationCommand(action: AutomationAction) {
    try {
      await ipasteApi.copyClip("text", action.command);
    } catch (unknownError) {
      console.error("copy failed", unknownError);
    }
  }

  function openAutomationContextMenu(action: AutomationAction, payload: { x: number; y: number }) {
    automationContextMenu.value = { action, x: payload.x, y: payload.y };
  }

  function closeAutomationContextMenu() {
    automationContextMenu.value = null;
  }

  function openAutomationDetail(action: AutomationAction) {
    automationDetailAction.value = action;
    automationDetailOpen.value = true;
  }

  function watchClosePanelRequest() {
    watch(
      () => store.closePanelRequested,
      (requested) => {
        if (requested) {
          void hidePanelFromUi();
          store.closePanelRequested = false;
        }
      },
    );
  }

  function exportAllAutomations() {
    closeAutomationContextMenu();
    if (!store.automations.length) {
      alert(t("automation.exportEmpty"));
      return;
    }
    try {
      const json = serializeAutomations(store.automations);
      const date = new Date().toISOString().slice(0, 10);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `ipaste-automations-${date}.json`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch {
      alert(t("automation.exportFailed"));
    }
  }

  function triggerImport() {
    closeAutomationContextMenu();
    importFileInput.value?.click();
  }

  async function onImportFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    input.value = "";

    try {
      const text = await file.text();
      const existingNames = new Set(store.automations.map((a) => a.name));
      const result = parseImportFile(text, existingNames);

      if (result.skippedInvalid > 0 && result.valid.length === 0 && result.skippedDuplicates === 0) {
        alert(t("automation.importNoValid"));
        return;
      }

      for (const input_ of result.valid) {
        await store.createAutomation(input_);
      }

      alert(t("automation.importSuccess", { imported: result.valid.length, skipped: result.skippedDuplicates }));
    } catch {
      alert(t("automation.importFailed"));
    }
  }

  return {
    automationEditorOpen,
    automationEditorAction,
    automationConfirmOpen,
    automationConfirmAction,
    automationContextMenu,
    automationDetailOpen,
    automationDetailAction,
    importFileInput,
    selectActionCard,
    runSelectedAction,
    executeAutomation,
    openAutomationEditor,
    saveAutomation,
    deleteAutomationAction,
    copyAutomationCommand,
    openAutomationContextMenu,
    closeAutomationContextMenu,
    openAutomationDetail,
    exportAllAutomations,
    triggerImport,
    onImportFileSelected,
    handleActionsKey,
    watchClosePanelRequest,
  };
}
