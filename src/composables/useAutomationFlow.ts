import { ref, watch } from "vue";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { serializeAutomations, parseImportFile } from "../stores/lib/automationTransfer";
import { useAutomationStore } from "../stores/automationStore";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { AutomationAction, AutomationInput } from "../types";

type IpasteStore = ReturnType<typeof useIpasteStore>;

/**
 * 自动化页签的交互流程（原 App.vue automation 段）：运行/确认、编辑器、详情、
 * 右键菜单、导入导出与 actions 页键盘导航。搬家逻辑零改动：store 经参数注入，
 * 面板隐藏经 hidePanelFromUi 注入以避免回环依赖。
 */
export function useAutomationFlow(store: IpasteStore, hidePanelFromUi: () => Promise<void>) {
  const automation = useAutomationStore();
  const automationEditorOpen = ref(false);
  const automationEditorAction = ref<AutomationAction | null>(null);
  const importPreviewOpen = ref(false);
  const importPreviewItems = ref<AutomationInput[]>([]);
  const importPreviewSkipped = ref(0);
  const automationContextMenu = ref<{ action: AutomationAction; x: number; y: number } | null>(null);
  const automationDetailOpen = ref(false);
  const automationDetailAction = ref<AutomationAction | null>(null);
  const importFileInput = ref<HTMLInputElement | null>(null);

  function handleActionsKey(key: string): boolean {
    if (key === "ArrowDown") {
      automation.selectedActionIndex = Math.min(automation.selectedActionIndex + 1, Math.max(automation.visibleActions.length - 1, 0));
      return true;
    }
    if (key === "ArrowUp") {
      automation.selectedActionIndex = Math.max(automation.selectedActionIndex - 1, 0);
      return true;
    }
    if (key === "Enter") {
      const action = automation.visibleActions[automation.selectedActionIndex];
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
    automation.selectedActionIndex = index;
  }

  function runSelectedAction(action: AutomationAction) {
    // 确认语义由后端原生对话框强制（sudo 模型），前端不再拦截。
    void executeAutomation(action);
  }

  async function executeAutomation(action: AutomationAction) {
    try {
      await automation.runAutomation(action.id);
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
        await automation.updateAutomation(automationEditorAction.value.id, input);
      } else {
        await automation.createAutomation(input);
      }
    } catch (unknownError) {
      console.error("automation save failed", unknownError);
    }
    automationEditorOpen.value = false;
  }

  async function deleteAutomationAction(action: AutomationAction) {
    try {
      await automation.deleteAutomation(action.id);
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
    if (!automation.automations.length) {
      alert(t("automation.exportEmpty"));
      return;
    }
    try {
      const json = serializeAutomations(automation.automations);
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
      const existingNames = new Set(automation.automations.map((a) => a.name));
      const result = parseImportFile(text, existingNames);

      if (result.skippedInvalid > 0 && result.valid.length === 0 && result.skippedDuplicates === 0) {
        alert(t("automation.importNoValid"));
        return;
      }
      if (result.valid.length === 0) {
        alert(t("automation.importSuccess", { imported: 0, skipped: result.skippedDuplicates }));
        return;
      }

      // 先预览后创建：用户在预览框逐条过目命令前不落库。
      importPreviewItems.value = result.valid;
      importPreviewSkipped.value = result.skippedDuplicates;
      importPreviewOpen.value = true;
    } catch {
      alert(t("automation.importFailed"));
    }
  }

  async function confirmImportAutomations() {
    importPreviewOpen.value = false;
    try {
      for (const input of importPreviewItems.value) {
        await automation.createAutomation(input);
      }
      alert(
        t("automation.importSuccess", {
          imported: importPreviewItems.value.length,
          skipped: importPreviewSkipped.value,
        }),
      );
    } catch {
      alert(t("automation.importFailed"));
    } finally {
      importPreviewItems.value = [];
    }
  }

  function cancelImportAutomations() {
    importPreviewOpen.value = false;
    importPreviewItems.value = [];
  }

  return {
    automationEditorOpen,
    automationEditorAction,
    automationContextMenu,
    automationDetailOpen,
    automationDetailAction,
    importPreviewOpen,
    importPreviewItems,
    confirmImportAutomations,
    cancelImportAutomations,
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
