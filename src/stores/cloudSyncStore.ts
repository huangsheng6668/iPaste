import { defineStore } from "pinia";
import { ipasteApi } from "../lib/ipasteApi";
import { showError } from "./uiStore";
import { useSettingsStore } from "./settingsStore";

/**
 * 云同步执行 store（Task 11 从 ipasteStore 原样拆出，行为零改动）。
 * 依赖方向保持单向：ipasteStore → cloudSyncStore → settingsStore；
 * 云端快照的落地（hydrate + clampSelection）留在宿主 store，经注册注入。
 */
export const useCloudSyncStore = defineStore("cloudSync", () => {
  const settings = useSettingsStore();

  let backgroundSyncTimer: number | null = null;
  let applyCloudSnapshot: (() => Promise<void>) | null = null;

  function registerSnapshotApplier(apply: () => Promise<void>) {
    applyCloudSnapshot = apply;
  }

  async function saveCloudSettings(apiAddress: string, apiKey: string) {
    const next = await ipasteApi.updateCloudSettings(apiAddress, apiKey);
    settings.applySettings(next);
    await syncCloudNow();
  }

  async function disableCloudSync() {
    const next = await ipasteApi.disableCloudSync();
    settings.applySettings(next);
  }

  function testCloudSettings(apiAddress: string, apiKey: string) {
    return ipasteApi.testCloudSettings(apiAddress, apiKey);
  }

  async function syncCloudNow() {
    if (!settings.cloud.enabled) return;

    try {
      clearBackgroundSyncTimer();
      await applyCloudSnapshot?.();
    } catch (unknownError) {
      showError(unknownError);
      throw unknownError;
    }
  }

  function syncCloudInBackground() {
    if (!settings.cloud.enabled) return;
    clearBackgroundSyncTimer();
    backgroundSyncTimer = window.setTimeout(() => {
      backgroundSyncTimer = null;
      void ipasteApi.syncCloudInBackground().catch((unknownError) => {
        showError(unknownError);
      });
    }, 600);
  }

  /** 停掉挂起的后台同步计时器（窗口卸载时调用；接线在 Task 15/16 落地）。 */
  function dispose() {
    clearBackgroundSyncTimer();
  }

  function clearBackgroundSyncTimer() {
    if (backgroundSyncTimer === null) return;
    window.clearTimeout(backgroundSyncTimer);
    backgroundSyncTimer = null;
  }

  return {
    registerSnapshotApplier,
    saveCloudSettings,
    disableCloudSync,
    testCloudSettings,
    syncCloudNow,
    syncCloudInBackground,
    dispose,
  };
});
