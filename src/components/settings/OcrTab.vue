<script setup lang="ts">
import { AlertCircle, CheckCircle2, Download, FolderOpen, LoaderCircle, ScanText, Unplug } from "lucide-vue-next";
import { t } from "../../i18n";
import { formatBytes } from "../../lib/format";
import { useIpasteStore } from "../../stores/ipasteStore";
import { useOcrInstaller } from "../../composables/useOcrInstaller";

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const store = useIpasteStore();
const {
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
} = useOcrInstaller();
</script>

<template>
  <div class="settings-section">
    <section
      v-if="!isMacOs"
      class="settings-panel settings-column-panel"
    >
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-violet">
          <ScanText class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-slate-950">
            {{ t("settings.tabs.ocr") }}
          </h2>
          <p class="mt-1 text-sm text-slate-500">
            {{ ocrStatusText }}
          </p>
        </div>
        <span
          class="ocr-status-badge"
          :class="{ 'ocr-status-badge-ready': ocrStatus?.installed }"
        >
          {{ ocrStatus?.installed ? t("common.ready") : t("common.notInstalled") }}
        </span>
      </div>

      <div class="ocr-mode-options">
        <button
          v-for="option in ocrModeOptions"
          :key="option.value"
          type="button"
          class="ocr-mode-option"
          :class="{ 'ocr-mode-option-active': store.ocrMode === option.value }"
          :aria-pressed="store.ocrMode === option.value"
          :disabled="isInstallingOcr || isRemovingOcr"
          @click="updateOcrMode(option.value)"
        >
          <span class="ocr-mode-option-header">
            <span>{{ option.label }}</span>
            <span>{{ formatBytes(option.totalBytes) }}</span>
          </span>
          <span class="ocr-mode-option-description">{{ option.description }}</span>
        </button>
      </div>
      <p class="ocr-mode-hint">
        {{ t("ocr.modeHint") }}
      </p>

      <div class="ocr-install-panel">
        <div class="ocr-install-meter">
          <div
            class="ocr-install-meter-fill"
            :style="{ width: `${ocrInstallPercent}%` }"
          />
        </div>
        <div class="ocr-install-meta">
          <span>{{ ocrDownloadedText }}</span>
          <span>{{ ocrInstallPercent }}%</span>
        </div>
      </div>

      <div class="ocr-install-details">
        <span>{{ t("ocr.downloadContents") }}</span>
        <span>{{ t("ocr.currentSelection", { label: selectedOcrModeOption.label, description: selectedOcrModeOption.description }) }}</span>
        <div
          v-if="ocrStatus?.installDir"
          class="ocr-install-dir-row"
        >
          <span>{{ t("ocr.directory", { path: ocrStatus.installDir }) }}</span>
          <button
            type="button"
            class="settings-icon-button"
            :title="t('ocr.openDownloadDir')"
            :aria-label="t('ocr.openDownloadDir')"
            @click="openOcrInstallDir"
          >
            <FolderOpen class="size-4" />
          </button>
        </div>
        <span v-if="ocrStatus?.manifestUrl">{{ t("ocr.manifest", { url: ocrStatus.manifestUrl }) }}</span>
        <span v-if="ocrProgress?.fileName">{{ t("ocr.currentFile", { file: ocrProgress.fileName }) }}</span>
      </div>

      <p
        v-if="ocrError || ocrMessage"
        class="settings-message"
        :class="{ 'settings-message-error': ocrError }"
      >
        <CheckCircle2
          v-if="ocrMessage && !ocrError"
          class="size-4"
        />
        <AlertCircle
          v-else
          class="size-4"
        />
        <span>{{ ocrError || ocrMessage }}</span>
      </p>

      <div class="settings-action-row">
        <button
          type="button"
          class="settings-action-button settings-action-button-primary"
          :disabled="isInstallingOcr || isRemovingOcr"
          @click="installOcrAssets"
        >
          <LoaderCircle
            v-if="isInstallingOcr"
            class="size-4 update-spin"
          />
          <Download
            v-else
            class="size-4"
          />
          <span>{{ ocrInstallButtonText }}</span>
        </button>
        <button
          type="button"
          class="settings-action-button settings-action-button-danger"
          :disabled="isInstallingOcr || isRemovingOcr || !ocrStatus?.installed"
          @click="removeOcrAssets"
        >
          <Unplug class="size-4" />
          <span>{{ isRemovingOcr ? t("ocr.deleting") : t("ocr.deleteResources") }}</span>
        </button>
      </div>
    </section>
  </div>
</template>
