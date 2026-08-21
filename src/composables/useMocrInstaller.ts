import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { formatBytes } from "../lib/format";
import { errorMessage } from "../lib/appError";
import { isTauri } from "../lib/env";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { OcrInstallProgress, OcrInstallStatus } from "../types";

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);

/** 设置页「日语 · 漫画」Manga-OCR 模型安装状态与操作（与 paddle 安装器相互独立）。 */
export function useMocrInstaller() {
  const mocrStatus = ref<OcrInstallStatus | null>(null);
  const mocrProgress = ref<OcrInstallProgress | null>(null);
  const mocrMessage = ref<string | null>(null);
  const mocrError = ref<string | null>(null);
  const isInstallingMocr = ref(false);
  const isRemovingMocr = ref(false);
  let unlistenMocrProgress: UnlistenFn | null = null;

  const mocrStatusText = computed(() => {
    if (!mocrStatus.value) return t("ocr.mocr.status.checking");
    return mocrStatus.value.installed ? t("ocr.mocr.status.installed") : t("ocr.mocr.status.readyToDownload");
  });
  const mocrDownloadedText = computed(() => {
    const downloaded = mocrProgress.value?.downloadedBytes ?? mocrStatus.value?.downloadedBytes ?? 0;
    const total = mocrProgress.value?.totalBytes ?? mocrStatus.value?.totalBytes ?? 0;
    return `${formatBytes(downloaded)} / ${formatBytes(total)}`;
  });
  const mocrInstallPercent = computed(() => {
    const total = mocrProgress.value?.totalBytes ?? mocrStatus.value?.totalBytes ?? 0;
    const downloaded = mocrProgress.value?.downloadedBytes ?? mocrStatus.value?.downloadedBytes ?? 0;
    if (!total) return mocrStatus.value?.installed ? 100 : 0;
    return Math.min(100, Math.round((downloaded / total) * 100));
  });
  const mocrInstallButtonText = computed(() => {
    if (isInstallingMocr.value) {
      return mocrProgress.value?.phase === "fetchingManifest"
        ? t("ocr.install.fetchingManifest")
        : t("ocr.install.downloading");
    }
    return mocrStatus.value?.installed ? t("ocr.mocr.repair") : t("ocr.mocr.download");
  });

  async function loadMocrStatus() {
    if (isMacOs) return;
    try {
      mocrStatus.value = await ipasteApi.mocrInstallStatus();
    } catch (unknownError) {
      mocrError.value = errorMessage(unknownError);
    }
  }

  async function installMocrAssets() {
    if (isInstallingMocr.value || isRemovingMocr.value) return;
    mocrMessage.value = null;
    mocrError.value = null;
    mocrProgress.value = null;
    isInstallingMocr.value = true;
    try {
      mocrStatus.value = await ipasteApi.installMocrAssets();
      mocrMessage.value = t("ocr.mocr.installDone");
    } catch (unknownError) {
      mocrError.value = errorMessage(unknownError);
    } finally {
      isInstallingMocr.value = false;
    }
  }

  async function removeMocrAssets() {
    if (isInstallingMocr.value || isRemovingMocr.value) return;
    mocrMessage.value = null;
    mocrError.value = null;
    mocrProgress.value = null;
    isRemovingMocr.value = true;
    try {
      mocrStatus.value = await ipasteApi.removeMocrAssets();
      mocrMessage.value = t("ocr.mocr.removed");
    } catch (unknownError) {
      mocrError.value = errorMessage(unknownError);
    } finally {
      isRemovingMocr.value = false;
    }
  }

  async function openMocrInstallDir() {
    const dir = mocrStatus.value?.installDir;
    if (!dir) return;
    try {
      await openPath(dir);
    } catch (unknownError) {
      mocrError.value = errorMessage(unknownError);
    }
  }

  onMounted(async () => {
    if (!isTauri || isMacOs) return;
    await loadMocrStatus();
    unlistenMocrProgress = await listen<OcrInstallProgress>(
      IPASTE_EVENTS.mocrInstallProgress,
      (event) => {
        mocrProgress.value = event.payload;
      },
    );
  });

  onUnmounted(() => {
    unlistenMocrProgress?.();
    unlistenMocrProgress = null;
  });

  return {
    mocrStatus,
    mocrProgress,
    mocrMessage,
    mocrError,
    isInstallingMocr,
    isRemovingMocr,
    mocrStatusText,
    mocrDownloadedText,
    mocrInstallPercent,
    mocrInstallButtonText,
    installMocrAssets,
    removeMocrAssets,
    openMocrInstallDir,
  };
}
