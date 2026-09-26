<script setup lang="ts">
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cloud,
  LoaderCircle,
  Plus,
  RotateCcw,
  Trash2,
  Unplug,
} from "lucide-vue-next";
import { t } from "../../i18n";
import { useSettingsStore } from "../../stores/settingsStore";
import OcrEngineSection from "./ocr/OcrEngineSection.vue";
import OcrLocalSection from "./ocr/OcrLocalSection.vue";
import OcrMangaSection from "./ocr/OcrMangaSection.vue";
import { useOpenaiOcr } from "../../composables/useOpenaiOcr";
import type { OcrEngine } from "../../types";

const settings = useSettingsStore();
const {
  openaiBaseUrl,
  openaiModel,
  openaiApiKey,
  openaiPrompts,
  isPromptsExpanded,
  openaiMessage,
  openaiError,
  isTestingOpenai,
  isSavingOpenai,
  openaiConfigured,
  openaiStatusText,
  formComplete,
  addPrompt,
  removePrompt,
  restoreDefaultPrompts,
  testOpenai,
  saveOpenaiConfig,
  clearOpenaiConfig,
} = useOpenaiOcr();

function updateOcrEngine(value: OcrEngine) {
  void settings.updateOcrEngine(value);
}


</script>

<template>
  <div class="settings-section">
    <OcrEngineSection
      :engine="settings.ocrEngine"
      :openai-configured="openaiConfigured"
      @select="updateOcrEngine"
    />

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

      <!-- 自定义提示词（折叠区域） -->
      <div class="ocr-prompts-section mt-1 border-t border-[var(--border)] pt-3">
        <button
          type="button"
          class="flex w-full cursor-pointer items-center justify-between py-1 text-left text-xs font-semibold text-[var(--text-2)] transition-colors hover:text-[var(--text-1)]"
          :aria-expanded="isPromptsExpanded"
          @click="isPromptsExpanded = !isPromptsExpanded"
        >
          <span class="flex items-center gap-1.5">
            <ChevronDown
              v-if="isPromptsExpanded"
              class="size-4 text-[var(--text-3)]"
            />
            <ChevronRight
              v-else
              class="size-4 text-[var(--text-3)]"
            />
            <span>{{ t("settings.openai.promptsTitle") }}</span>
          </span>
          <span class="text-[11px] font-medium text-[var(--text-3)]">
            {{ openaiPrompts.length }}
          </span>
        </button>

        <div
          v-if="isPromptsExpanded"
          class="mt-3 flex flex-col gap-3"
        >
          <div class="rounded-[var(--r-md)] border border-amber-500/20 bg-amber-500/10 px-3 py-2 text-xs leading-relaxed text-amber-700 dark:text-amber-300">
            {{ t("settings.openai.promptsVariableHint") }}
          </div>

          <div
            v-for="(prompt, index) in openaiPrompts"
            :key="index"
            class="flex flex-col gap-2 rounded-[var(--r-md)] border border-[var(--border)] bg-[var(--surface-inset)] p-2.5"
          >
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="text-[11px] font-semibold text-[var(--text-3)]">{{ t("settings.openai.promptsRole") }}</span>
                <select
                  v-model="prompt.role"
                  class="rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-0.5 text-xs text-[var(--text-1)] outline-none focus:border-[var(--accent)]"
                >
                  <option value="system">
                    {{ t("settings.openai.promptsRoleSystem") }}
                  </option>
                  <option value="user">
                    {{ t("settings.openai.promptsRoleUser") }}
                  </option>
                </select>
              </div>
              <button
                type="button"
                class="cursor-pointer rounded p-1 text-[var(--text-3)] transition-colors hover:bg-red-500/10 hover:text-red-500 disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-[var(--text-3)]"
                :disabled="openaiPrompts.length <= 1"
                :title="t('settings.openai.removePrompt')"
                :aria-label="t('settings.openai.removePrompt')"
                @click="removePrompt(index)"
              >
                <Trash2 class="size-3.5" />
              </button>
            </div>

            <textarea
              v-model="prompt.content"
              rows="3"
              class="w-full resize-y rounded border border-[var(--border)] bg-[var(--surface)] p-2 font-mono text-xs leading-normal text-[var(--text-1)] placeholder-[var(--text-3)] outline-none focus:border-[var(--accent)]"
              :placeholder="t('settings.openai.promptsContent')"
              spellcheck="false"
            />
          </div>

          <div class="flex items-center justify-between pt-1">
            <button
              type="button"
              class="inline-flex cursor-pointer items-center gap-1.5 rounded-[var(--r-md)] border border-[var(--border)] bg-[var(--surface)] px-2.5 py-1 text-xs font-semibold text-[var(--text-1)] transition-colors hover:bg-[var(--surface-hover)]"
              @click="addPrompt('user')"
            >
              <Plus class="size-3.5" />
              <span>{{ t("settings.openai.addPrompt") }}</span>
            </button>

            <button
              type="button"
              class="inline-flex cursor-pointer items-center gap-1 text-xs text-[var(--text-3)] transition-colors hover:text-[var(--text-1)]"
              @click="restoreDefaultPrompts"
            >
              <RotateCcw class="size-3" />
              <span>{{ t("settings.openai.restoreDefaultPrompts") }}</span>
            </button>
          </div>
        </div>
      </div>

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

    <OcrLocalSection />

    <OcrMangaSection />
  </div>
</template>
