<script setup lang="ts">
import {
  AlertCircle,
  BookOpenText,
  CheckCircle2,
  Download,
  FolderOpen,
  LoaderCircle,
  Unplug,
} from "lucide-vue-next";
import { t } from "../../../i18n";
import { useMocrInstaller } from "../../../composables/useMocrInstaller";

// 日语·漫画专用 Manga-OCR 模型安装区：自持安装器 composable（与 Paddle 安装器相互独立）。
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
</template>
