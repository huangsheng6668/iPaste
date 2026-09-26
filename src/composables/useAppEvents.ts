import { subscribe } from "../platform/events";
import { isTauri } from "../lib/env";
import { ipasteApi } from "../lib/ipasteApi";
import { IPASTE_EVENTS } from "../types/generated/events";
import { t } from "../i18n";
import type { I18nKey } from "../i18n";
import { useUiStore } from "../stores/uiStore";
import { useSettingsStore } from "../stores/settingsStore";
import { useAutomationStore } from "../stores/automationStore";
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
import type { PairRequested } from "../types/generated/PairRequested";

type IpasteStore = ReturnType<typeof useIpasteStore>;

/**
 * 主窗口的全局事件接线（原 ipasteStore.bindEvents + store 底部的 automation 事件块）。
 * 搬家逻辑零改动：数据写回仍经 store 的 refs/actions；automation 事件直接操作
 * store 的 automations/runningAutomationLogs/closePanelRequested。
 */
export async function useAppEvents(store: IpasteStore): Promise<void> {
  if (!isTauri) return;

  const ui = useUiStore();
  const settings = useSettingsStore();
  const automation = useAutomationStore();

  await subscribe<CapturedEvent>(IPASTE_EVENTS.clipboardCaptured, (payload) => {
    store.applyCaptured(payload);
  });

  await subscribe<ListeningChangedEvent>(IPASTE_EVENTS.listeningChanged, (payload) => {
    store.isListening = payload.isListening;
  });

  await subscribe<AppendCopyChangedEvent>(IPASTE_EVENTS.appendCopyChanged, (payload) => {
    store.isAppendCopyEnabled = payload.isEnabled;
  });

  await subscribe<ClipUpdatedEvent>(IPASTE_EVENTS.clipUpdated, (payload) => {
    store.applyClipUpdate(payload);
  });

  await subscribe<SettingsChangedEvent>(IPASTE_EVENTS.settingsChanged, (payload) => {
    settings.applySettings(payload.settings);
  });

  await subscribe<{ visible: boolean }>(IPASTE_EVENTS.panelVisibilityChanged, (payload) => {
    if (payload.visible) {
      // 每次面板显示时刷新快照：LAN 同步收到的条目/分类在面板隐藏期间落库，
      // 若事件驱动的刷新错过（如 webview 重建），这里兜底保证数据可见。
      void store.load();
      store.activatePanelDefault();
    }
  });

  // 捕获失败此前是死事件（Rust 发、无人听）：保留排障信号但不打扰 UI。
  await subscribe<{ message?: string }>(IPASTE_EVENTS.captureError, (payload) => {
    console.warn("[ipaste] clipboard capture error:", payload);
  });

  // 截图 OCR 预检失败（权限/资源/平台）：设置窗由 Rust 侧直达对应 Tab，这里补 toast
  await subscribe<{ code: string }>(IPASTE_EVENTS.ocrScreenshotError, (payload) => {
    const keyByCode: Record<string, I18nKey> = {
      screenRecordingPermission: "ocrScreenshot.errorScreenRecordingPermission",
      ocrModelMissing: "ocrScreenshot.errorOcrModelMissing",
      ocrUnsupported: "ocrScreenshot.errorOcrUnsupported",
      screenCaptureFailed: "ocrScreenshot.errorScreenCaptureFailed",
      ocrCloudKeyMissing: "ocrScreenshot.errorOcrCloudKeyMissing",
    };
    ui.pushToast(t(keyByCode[payload.code] ?? "ocrScreenshot.recognizeFailed"));
  });

  await subscribe<AutomationRunStartedEvent>(IPASTE_EVENTS.automationRunStarted, (payload) => {
    const { automationId, runId, startedAt } = payload;
    const action = automation.automations.find((entry) => entry.id === automationId);
    if (action) {
      action.lastRun = { id: runId, status: "running", startedAt, finishedAt: null, exitCode: null, durationMs: null };
    }
  });
  await subscribe<AutomationRunOutputEvent>(IPASTE_EVENTS.automationRunOutput, (payload) => {
    const { runId, stream, chunk } = payload;
    const logs = automation.runningAutomationLogs[runId] ?? { stdout: "", stderr: "" };
    const limit = 200 * 1024;
    if (stream === "stderr") logs.stderr = (logs.stderr + chunk).slice(-limit);
    else logs.stdout = (logs.stdout + chunk).slice(-limit);
    automation.runningAutomationLogs = { ...automation.runningAutomationLogs, [runId]: logs };
  });
  await subscribe<AutomationRunFinishedEvent>(IPASTE_EVENTS.automationRunFinished, (payload) => {
    const { automationId, status, exitCode, finishedAt } = payload;
    const action = automation.automations.find((entry) => entry.id === automationId);
    if (action?.lastRun) {
      action.lastRun = { ...action.lastRun, status, exitCode: exitCode ?? null, finishedAt };
    }
    if (action?.closePanelOnSuccess && status === "success") {
      store.closePanelRequested = true;
    }
  });

  // 跨设备同步：收到对端剪贴板/整组时提示并刷新列表——同步落库发生在 Rust
  // 侧，前端 store 不会自动感知，需像 panelVisibilityChanged 一样主动 load
  //（v4 行为恢复）。pairJoinFailed 等配对失败反馈由 lan-sync 窗口的
  // useDeviceSync 就地展示，主窗口不重复 toast；配对请求本身例外，见下方
  // pairRequest 监听。
  await subscribe<DeviceClipReceived>(IPASTE_EVENTS.deviceClipReceived, () => {
    ui.pushToast(t("deviceSync.clipReceived"));
    void store.load();
  });

  await subscribe<DeviceCategoryReceived>(IPASTE_EVENTS.deviceCategoryReceived, () => {
    ui.pushToast(t("deviceSync.categoryReceived"));
    void store.load();
  });

  // 跨设备接收失败（含 auto 剪贴板写失败诊断）：仅 console.warn 静默记录——
  // 无头/锁屏场景的噪音不该用 toast 打扰用户，失败细节留日志排查。
  await subscribe<DeviceClipReceiveFailed>(IPASTE_EVENTS.deviceClipReceiveFailed, (payload) => {
    console.warn("[ipaste] device clip receive failed:", payload);
  });

  // 配对请求可见性兜底（v0.9.2）：唯一的 ipaste://pair-request 弹窗逻辑在
  // 瞬态的 lan-sync 窗口里，窗口关闭时请求完全不可见，120s 后端自动拒绝会让
  // 对端误以为被拒。主窗口常驻监听：拉起设备管理窗（含恢复待确认请求）并
  // toast 提示用户前去确认。
  await subscribe<PairRequested>(IPASTE_EVENTS.pairRequest, (payload) => {
    void ipasteApi.openLanSync().catch((error: unknown) => {
      console.warn("[ipaste] open lan-sync window failed:", error);
    });
    ui.pushToast(t("deviceSync.pair.incoming", { name: payload.deviceName }));
  });
}
