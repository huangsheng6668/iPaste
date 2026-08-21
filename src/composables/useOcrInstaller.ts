import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { formatBytes } from "../lib/format";
import { errorMessage } from "../lib/appError";
import { isTauri } from "../lib/env";
import { useIpasteStore } from "../stores/ipasteStore";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { OcrInstallProgress, OcrInstallStatus, OcrMode } from "../types";

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);

export function useOcrInstaller() {
  const store = useIpasteStore();
  const ocrStatus = ref<OcrInstallStatus | null>(null);
  const ocrProgress = ref<OcrInstallProgress | null>(null);
  const ocrMessage = ref<string | null>(null);
  const ocrError = ref<string | null>(null);
  const isInstallingOcr = ref(false);
  const isRemovingOcr = ref(false);
  const lastInstalledOcrMode = ref<OcrMode | null>(null);
  let unlistenOcrProgress: UnlistenFn | null = null;

  const ocrModeOptions = computed<Array<{ label: string; value: OcrMode; description: string; totalBytes: number }>>(() => [
    {
      label: "Fast",
      value: "fast",
      description: t("ocr.mode.fast.description"),
      totalBytes: 10_885_068,
    },
    {
      label: "Best",
      value: "best",
      description: t("ocr.mode.best.description"),
      totalBytes: 21_365_848,
    },
  ]);

  const selectedOcrModeOption = computed(() => {
    return ocrModeOptions.value.find((option) => option.value === store.ocrMode) ?? ocrModeOptions.value[0];
  });
  const ocrStatusText = computed(() => {
    if (!ocrStatus.value) return t("ocr.status.checking");
    if (isMacOs) {
      return t("ocr.status.macos");
    }
    if (ocrStatus.value.installed) {
      return t("ocr.status.installed");
    }
    if (lastInstalledOcrMode.value && lastInstalledOcrMode.value !== store.ocrMode) {
      return t("ocr.status.modeNotDownloaded");
    }
    return t("ocr.status.readyToDownload");
  });
  const ocrDownloadedText = computed(() => {
    const downloaded = ocrProgress.value?.downloadedBytes ?? ocrStatus.value?.downloadedBytes ?? 0;
    const total = ocrProgress.value?.totalBytes ?? ocrStatus.value?.totalBytes ?? selectedOcrModeOption.value.totalBytes;
    return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  });
  const ocrInstallPercent = computed(() => {
    const total = ocrProgress.value?.totalBytes ?? ocrStatus.value?.totalBytes ?? 0;
    const downloaded = ocrProgress.value?.downloadedBytes ?? ocrStatus.value?.downloadedBytes ?? 0;
    if (!total) return ocrStatus.value?.installed ? 100 : 0;
    return Math.min(100, Math.round((downloaded / total) * 100));
  });
  const ocrInstallButtonText = computed(() => {
    if (isInstallingOcr.value) {
      return ocrProgress.value?.phase === "fetchingManifest" ? t("ocr.install.fetchingManifest") : t("ocr.install.downloading");
    }
    if (ocrStatus.value?.installed) {
      return t("ocr.install.repair");
    }
    if (lastInstalledOcrMode.value && lastInstalledOcrMode.value !== store.ocrMode) {
      return t("ocr.install.switchAndDownload");
    }
    return t("ocr.install.download");
  });

  async function loadOcrStatus() {
    if (isMacOs) return;
    try {
      ocrStatus.value = await ipasteApi.ocrInstallStatus();
      const mode = ocrStatus.value.mode;
      if (ocrStatus.value.installed && (mode === "fast" || mode === "best")) {
        lastInstalledOcrMode.value = mode;
      }
    } catch (unknownError) {
      ocrError.value = errorMessage(unknownError);
    }
  }

  async function updateOcrMode(mode: OcrMode) {
    if (mode === store.ocrMode || isInstallingOcr.value || isRemovingOcr.value) return;
    ocrMessage.value = null;
    ocrError.value = null;
    ocrProgress.value = null;
    try {
      await store.updateOcrMode(mode);
      await loadOcrStatus();
    } catch (unknownError) {
      ocrError.value = errorMessage(unknownError);
    }
  }

  async function installOcrAssets() {
    ocrMessage.value = null;
    ocrError.value = null;
    isInstallingOcr.value = true;
    try {
      ocrProgress.value = {
        phase: "fetchingManifest",
        fileName: null,
        downloadedBytes: 0,
        totalBytes: ocrStatus.value?.totalBytes ?? 0,
      };
      ocrStatus.value = await ipasteApi.installOcrAssets();
      const mode = ocrStatus.value.mode;
      if (mode === "fast" || mode === "best") {
        lastInstalledOcrMode.value = mode;
      }
      ocrMessage.value = t("ocr.readyMessage");
    } catch (unknownError) {
      ocrError.value = errorMessage(unknownError);
    } finally {
      isInstallingOcr.value = false;
    }
  }

  async function removeOcrAssets() {
    ocrMessage.value = null;
    ocrError.value = null;
    isRemovingOcr.value = true;
    try {
      ocrStatus.value = await ipasteApi.removeOcrAssets();
      ocrProgress.value = null;
      lastInstalledOcrMode.value = null;
      ocrMessage.value = t("ocr.removedMessage");
    } catch (unknownError) {
      ocrError.value = errorMessage(unknownError);
    } finally {
      isRemovingOcr.value = false;
    }
  }

  async function openOcrInstallDir() {
    if (!ocrStatus.value?.installDir) return;
    ocrMessage.value = null;
    ocrError.value = null;
    try {
      await openPath(ocrStatus.value.installDir);
    } catch (unknownError) {
      ocrError.value = errorMessage(unknownError);
    }
  }

  onMounted(async () => {
    await loadOcrStatus();
    if (isTauri) {
      unlistenOcrProgress = await listen<OcrInstallProgress>(IPASTE_EVENTS.ocrInstallProgress, (event) => {
        ocrProgress.value = event.payload;
      });
    }
  });

  onUnmounted(() => {
    void unlistenOcrProgress?.();
  });

  return {
    ocrStatus,
    ocrProgress,
    ocrMessage,
    ocrError,
    isInstallingOcr,
    isRemovingOcr,
    ocrModeOptions,
    selectedOcrModeOption,
    ocrStatusText,
    ocrDownloadedText,
    ocrInstallPercent,
    ocrInstallButtonText,
    updateOcrMode,
    installOcrAssets,
    removeOcrAssets,
    openOcrInstallDir,
  };
}
