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
import type { DeviceClipReceived } from "../types/generated/DeviceClipReceived";
import type { DeviceClipReceiveFailed } from "../types/generated/DeviceClipReceiveFailed";
import type { DeviceCategoryReceived } from "../types/generated/DeviceCategoryReceived";

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
      screenCaptureFailed: "ocrScreenshot.errorScreenCaptureFailed",
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

  // 跨设备同步：收到对端剪贴板/整组时提示并刷新列表——同步落库发生在 Rust
  // 侧，前端 store 不会自动感知，需像 panelVisibilityChanged 一样主动 load
  //（v4 行为恢复）。pairJoinFailed 等配对流反馈由 lan-sync 窗口的
  // useDeviceSync 就地展示，主窗口不重复 toast。
  await listen<DeviceClipReceived>(IPASTE_EVENTS.deviceClipReceived, () => {
    ui.pushToast(t("deviceSync.clipReceived"));
    void store.load();
  });

  await listen<DeviceCategoryReceived>(IPASTE_EVENTS.deviceCategoryReceived, () => {
    ui.pushToast(t("deviceSync.categoryReceived"));
    void store.load();
  });

  // 跨设备接收失败（含 auto 剪贴板写失败诊断）：仅 console.warn 静默记录——
  // 无头/锁屏场景的噪音不该用 toast 打扰用户，失败细节留日志排查。
  await listen<DeviceClipReceiveFailed>(IPASTE_EVENTS.deviceClipReceiveFailed, (event) => {
    console.warn("[ipaste] device clip receive failed:", event.payload);
  });
}
