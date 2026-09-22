<script setup lang="ts">
import { AlertCircle, BookOpenText, CheckCircle2, Cloud, Download, FolderOpen, LoaderCircle, ScanText, Unplug } from "lucide-vue-next";
import { t } from "../../i18n";
import { formatBytes } from "../../lib/format";
import { useIpasteStore } from "../../stores/ipasteStore";
import { useOpenaiOcr } from "../../composables/useOpenaiOcr";
import { useMocrInstaller } from "../../composables/useMocrInstaller";
import { useOcrInstaller } from "../../composables/useOcrInstaller";
import type { OcrEngine } from "../../types";

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const store = useIpasteStore();
const {
  openaiBaseUrl,
  openaiModel,
  openaiApiKey,
  openaiMessage,
  openaiError,
  isTestingOpenai,
  isSavingOpenai,
  openaiConfigured,
  openaiStatusText,
  formComplete,
  testOpenai,
  saveOpenaiConfig,
  clearOpenaiConfig,
} = useOpenaiOcr();

const ocrEngineOptions: Array<{ value: OcrEngine; label: string; description: string }> = [
  {
    value: "local",
    label: t("settings.bigmodel.engineLocal"),
    description: t("settings.bigmodel.engineLocalDescription"),
  },
  {
    value: "openai",
    label: t("settings.openai.engineOpenai"),
    description: t("settings.openai.engineOpenaiDescription"),
  },
];

function updateOcrEngine(value: OcrEngine) {
  void store.updateOcrEngine(value);
}

function engineBadgeLabel(): string {
  if (store.ocrEngine === "openai") return t("settings.openai.engineOpenai");
  return t("settings.bigmodel.engineLocal");
}

function engineReady(): boolean {
  if (store.ocrEngine === "openai") return openaiConfigured.value;
  return true;
}

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
const {
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
} = useMocrInstaller();
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-violet">
          <ScanText class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.bigmodel.engineTitle") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.bigmodel.engineSubtitle") }}
          </p>
        </div>
        <span
          class="ocr-status-badge"
          :class="{ 'ocr-status-badge-ready': engineReady() }"
        >
          {{ engineBadgeLabel() }}
        </span>
      </div>

      <div class="ocr-mode-options">
        <button
          v-for="option in ocrEngineOptions"
          :key="option.value"
          type="button"
          class="ocr-mode-option"
          :class="{ 'ocr-mode-option-active': store.ocrEngine === option.value }"
          :aria-pressed="store.ocrEngine === option.value"
          @click="updateOcrEngine(option.value)"
        >
          <span class="ocr-mode-option-header">
            <span>{{ option.label }}</span>
          </span>
          <span class="ocr-mode-option-description">{{ option.description }}</span>
        </button>
      </div>
      <p class="ocr-mode-hint">
        {{ t("settings.bigmodel.engineHint") }}
      </p>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-teal">
          <Cloud class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.openai.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ openaiStatusText }}
          </p>
        </div>
        <span
          class="ocr-status-badge"
          :class="{ 'ocr-status-badge-ready': openaiConfigured }"
        >
          {{ openaiConfigured ? t("common.ready") : t("settings.openai.notConfigured") }}
        </span>
      </div>

      <p class="ocr-mode-hint">
        {{ t("settings.openai.description") }}
      </p>

      <label class="settings-field">
        <span>{{ t("settings.openai.baseUrl") }}</span>
        <input
          v-model="openaiBaseUrl"
          type="url"
          placeholder="https://api.openai.com/v1"
          spellcheck="false"
        >
      </label>

      <label class="settings-field">
        <span>{{ t("settings.openai.model") }}</span>
        <input
          v-model="openaiModel"
          type="text"
          placeholder="glm-4v-flash"
          spellcheck="false"
        >
      </label>

      <label class="settings-field">
        <span>API Key</span>
        <input
          v-model="openaiApiKey"
          type="password"
          autocomplete="current-password"
          spellcheck="false"
        >
      </label>

      <p class="ocr-mode-hint">
        {{ t("settings.openai.privacyHint") }}
      </p>

      <p
        v-if="openaiError || openaiMessage"
        class="settings-message"
        :class="{ 'settings-message-error': openaiError }"
      >
        <CheckCircle2
          v-if="openaiMessage && !openaiError"
          class="size-4"
        />
        <AlertCircle
          v-else
          class="size-4"
        />
        <span>{{ openaiError || openaiMessage }}</span>
      </p>

      <div class="settings-action-row">
        <button
          type="button"
          class="settings-action-button"
          :disabled="isTestingOpenai || isSavingOpenai || !formComplete"
          @click="testOpenai"
        >
          <LoaderCircle
            v-if="isTestingOpenai"
            class="size-4 update-spin"
          />
          <CheckCircle2
            v-else
            class="size-4"
          />
          <span>{{ isTestingOpenai ? t("settings.openai.testing") : t("settings.openai.test") }}</span>
        </button>
        <button
          type="button"
          class="settings-action-button settings-action-button-primary"
          :disabled="isTestingOpenai || isSavingOpenai || !formComplete"
          @click="saveOpenaiConfig"
        >
          <Cloud class="size-4" />
          <span>{{ isSavingOpenai ? t("common.saving") : t("settings.openai.save") }}</span>
        </button>
        <button
          type="button"
          class="settings-action-button settings-action-button-danger"
          :disabled="isTestingOpenai || isSavingOpenai || !openaiConfigured"
          @click="clearOpenaiConfig"
        >
          <Unplug class="size-4" />
          <span>{{ t("settings.openai.clear") }}</span>
        </button>
      </div>
    </section>

    <section
      v-if="!isMacOs"
      class="settings-panel settings-column-panel"
    >
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-violet">
          <ScanText class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.tabs.ocr") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
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

    <section
      class="settings-panel settings-column-panel"
    >
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-violet">
          <BookOpenText class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("ocr.mocr.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ mocrStatusText }}
          </p>
        </div>
        <span
          class="ocr-status-badge"
          :class="{ 'ocr-status-badge-ready': mocrStatus?.installed }"
        >
          {{ mocrStatus?.installed ? t("common.ready") : t("common.notInstalled") }}
        </span>
      </div>

      <p class="ocr-mode-hint">
        {{ t("ocr.mocr.description") }}
      </p>

      <div class="ocr-install-panel">
        <div class="ocr-install-meter">
          <div
            class="ocr-install-meter-fill"
            :style="{ width: `${mocrInstallPercent}%` }"
          />
        </div>
        <div class="ocr-install-meta">
          <span>{{ mocrDownloadedText }}</span>
          <span>{{ mocrInstallPercent }}%</span>
        </div>
      </div>

      <div class="ocr-install-details">
        <span>{{ t("ocr.mocr.downloadContents") }}</span>
        <div
          v-if="mocrStatus?.installDir"
          class="ocr-install-dir-row"
        >
          <span>{{ t("ocr.directory", { path: mocrStatus.installDir }) }}</span>
          <button
            type="button"
            class="settings-icon-button"
            :title="t('ocr.openDownloadDir')"
            :aria-label="t('ocr.openDownloadDir')"
            @click="openMocrInstallDir"
          >
            <FolderOpen class="size-4" />
          </button>
        </div>
        <span v-if="mocrProgress?.fileName">{{ t("ocr.currentFile", { file: mocrProgress.fileName }) }}</span>
      </div>

      <p
        v-if="mocrError || mocrMessage"
        class="settings-message"
        :class="{ 'settings-message-error': mocrError }"
      >
        <CheckCircle2
          v-if="mocrMessage && !mocrError"
          class="size-4"
        />
        <AlertCircle
          v-else
          class="size-4"
        />
        <span>{{ mocrError || mocrMessage }}</span>
      </p>

      <div class="settings-action-row">
        <button
          type="button"
          class="settings-action-button settings-action-button-primary"
          :disabled="isInstallingMocr || isRemovingMocr"
          @click="installMocrAssets"
        >
          <LoaderCircle
            v-if="isInstallingMocr"
            class="size-4 update-spin"
          />
          <Download
            v-else
            class="size-4"
          />
          <span>{{ mocrInstallButtonText }}</span>
        </button>
        <button
          type="button"
          class="settings-action-button settings-action-button-danger"
          :disabled="isInstallingMocr || isRemovingMocr || !mocrStatus?.installed"
          @click="removeMocrAssets"
        >
          <Unplug class="size-4" />
          <span>{{ isRemovingMocr ? t("ocr.deleting") : t("ocr.mocr.delete") }}</span>
        </button>
      </div>
    </section>
  </div>
</template>
