import { listen } from "@tauri-apps/api/event";
import { isTauri } from "../lib/env";
import { IPASTE_EVENTS } from "../types/generated/events";
import { t } from "../i18n";
import type { I18nKey } from "../i18n";
import { useUiStore } from "../stores/uiStore";
import type { useIpasteStore } from "../stores/ipasteStore";
import type {
  AppendCopyChangedEvent,
  AutomationRunFinishedEvent,
  AutomationRunOutputEvent,
  AutomationRunStartedEvent,
  CapturedEvent,
  ClipUpdatedEvent,
  ListeningChangedEvent,
  SettingsChangedEvent,
} from "../types";

type IpasteStore = ReturnType<typeof useIpasteStore>;

/**
 * 主窗口的全局事件接线（原 ipasteStore.bindEvents + store 底部的 automation 事件块）。
 * 搬家逻辑零改动：数据写回仍经 store 的 refs/actions；automation 事件直接操作
 * store 的 automations/runningAutomationLogs/closePanelRequested。
 */
export async function useAppEvents(store: IpasteStore): Promise<void> {
  if (!isTauri) return;

  const ui = useUiStore();

  await listen<CapturedEvent>(IPASTE_EVENTS.clipboardCaptured, (event) => {
    store.upsertClip(event.payload.clip, event.payload.clipTotalCount, event.payload.wasInserted);
  });

  // 局域网同步收到条目（历史或分组）后，主窗口也要刷新列表：lan-clip-received
  // 广播到所有窗口，但此前只有 LAN 同步窗口监听，主窗口列表不更新，
  // 表现为「B 端提示已接收但列表里没有新条目」。
  await listen(IPASTE_EVENTS.lanClipReceived, () => {
    void store.load();
  });

  await listen<ListeningChangedEvent>(IPASTE_EVENTS.listeningChanged, (event) => {
    store.isListening = event.payload.isListening;
  });

  await listen<AppendCopyChangedEvent>(IPASTE_EVENTS.appendCopyChanged, (event) => {
    store.isAppendCopyEnabled = event.payload.isEnabled;
  });

  await listen<ClipUpdatedEvent>(IPASTE_EVENTS.clipUpdated, (event) => {
    if (event.payload.mergedFromId && event.payload.mergedFromId !== event.payload.item.id) {
      if (event.payload.collection === "history") {
        store.clips = store.clips.filter((clip) => clip.id !== event.payload.mergedFromId);
        store.clipTotalCount = Math.max(0, store.clipTotalCount - 1);
        store.visibleHistoryTotalCount = Math.max(0, store.visibleHistoryTotalCount - 1);
      } else {
        store.categoryItems = store.categoryItems.filter((item) => item.id !== event.payload.mergedFromId);
      }
    }
    store.patchItem(event.payload.collection, event.payload.item);
    if (event.payload.collection === "category") {
      store.syncCloudInBackground();
    }
  });

  await listen<SettingsChangedEvent>(IPASTE_EVENTS.settingsChanged, (event) => {
    store.applySettings(event.payload.settings);
  });

  await listen<{ visible: boolean }>(IPASTE_EVENTS.panelVisibilityChanged, (event) => {
    if (event.payload.visible) {
      // 每次面板显示时刷新快照：LAN 同步收到的条目/分类在面板隐藏期间落库，
      // 若事件驱动的刷新错过（如 webview 重建），这里兜底保证数据可见。
      void store.load();
      store.activatePanelDefault();
    }
  });

  // 捕获失败此前是死事件（Rust 发、无人听）：保留排障信号但不打扰 UI。
  await listen<{ message?: string }>(IPASTE_EVENTS.captureError, (event) => {
    console.warn("[ipaste] clipboard capture error:", event.payload);
  });

  // 截图 OCR 预检失败（权限/资源/平台）：设置窗由 Rust 侧直达对应 Tab，这里补 toast
  await listen<{ code: string }>(IPASTE_EVENTS.ocrScreenshotError, (event) => {
    const keyByCode: Record<string, I18nKey> = {
      screenRecordingPermission: "ocrScreenshot.errorScreenRecordingPermission",
      ocrModelMissing: "ocrScreenshot.errorOcrModelMissing",
      ocrUnsupported: "ocrScreenshot.errorOcrUnsupported",
    };
    ui.pushToast(t(keyByCode[event.payload.code] ?? "ocrScreenshot.recognizeFailed"));
  });

  await listen<AutomationRunStartedEvent>(IPASTE_EVENTS.automationRunStarted, (event) => {
    const { automationId, runId, startedAt } = event.payload;
    const action = store.automations.find((entry) => entry.id === automationId);
    if (action) {
      action.lastRun = { id: runId, status: "running", startedAt, finishedAt: null, exitCode: null, durationMs: null };
    }
  });
  await listen<AutomationRunOutputEvent>(IPASTE_EVENTS.automationRunOutput, (event) => {
    const { runId, stream, chunk } = event.payload;
    const logs = store.runningAutomationLogs[runId] ?? { stdout: "", stderr: "" };
    const limit = 200 * 1024;
    if (stream === "stderr") logs.stderr = (logs.stderr + chunk).slice(-limit);
    else logs.stdout = (logs.stdout + chunk).slice(-limit);
    store.runningAutomationLogs = { ...store.runningAutomationLogs, [runId]: logs };
  });
  await listen<AutomationRunFinishedEvent>(IPASTE_EVENTS.automationRunFinished, (event) => {
    const { automationId, status, exitCode, finishedAt } = event.payload;
    const action = store.automations.find((entry) => entry.id === automationId);
    if (action?.lastRun) {
      action.lastRun = { ...action.lastRun, status, exitCode: exitCode ?? null, finishedAt };
    }
    if (action?.closePanelOnSuccess && status === "success") {
      store.closePanelRequested = true;
    }
  });
}
