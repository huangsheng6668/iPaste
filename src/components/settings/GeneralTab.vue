<script setup lang="ts">
import { computed } from "vue";
import { AlertCircle, AppWindow, CheckCircle2, ClipboardPlus, Database, History, LoaderCircle, MonitorSmartphone, Moon, Power, SlidersHorizontal, Sparkles, Sun, Tags, Trash2 } from "lucide-vue-next";
import LanguageSelect from "../LanguageSelect.vue";
import { languageOptions, t } from "../../i18n";
import { isTauri } from "../../lib/env";
import { setThemePreference, themePreference, type ThemePreference } from "../../lib/theme";
import { useIpasteStore } from "../../stores/ipasteStore";
import { useClearHistory } from "../../composables/useClearHistory";
import { useAutostart } from "../../composables/useAutostart";
import type { Language, PanelLayout, PanelOpenBehavior } from "../../types";

const store = useIpasteStore();
const {
  isClearingHistory,
  confirmingClearHistory,
  storageMessage,
  storageError,
  requestClearHistory,
  cancelClearHistory,
  confirmClearHistory,
} = useClearHistory();
const { autostartEnabled, isTogglingAutostart, autostartError, toggleAutostart } = useAutostart();

const retentionOptions = computed(() => [
  { label: t("settings.retention.7"), value: 7 },
  { label: t("settings.retention.14"), value: 14 },
  { label: t("settings.retention.30"), value: 30 },
  { label: t("settings.retention.90"), value: 90 },
]);

const appendCopyTimeoutOptions = [
  { label: "1", value: 1 },
  { label: "3", value: 3 },
  { label: "5", value: 5 },
  { label: "10", value: 10 },
];

const panelOpenOptions = computed<Array<{ label: string; value: PanelOpenBehavior; icon: typeof History }>>(() => [
  { label: t("settings.panelOpen.history"), value: "history", icon: History },
  { label: t("settings.panelOpen.lastSelected"), value: "last_selected", icon: Tags },
]);

const panelLayoutOptions = computed<Array<{ label: string; value: PanelLayout }>>(() => [
  { label: t("settings.layout.top"), value: "top" },
  { label: t("settings.layout.side"), value: "side" },
]);

const themeOptions = computed<Array<{ label: string; value: ThemePreference; icon: typeof Sun }>>(() => [
  { label: t("settings.appearance.light"), value: "light", icon: Sun },
  { label: t("settings.appearance.dark"), value: "dark", icon: Moon },
  { label: t("settings.appearance.system"), value: "system", icon: MonitorSmartphone },
]);

const retentionText = computed(() => {
  return retentionOptions.value.find((option) => option.value === store.retentionDays)?.label ?? t("settings.retention.30");
});

const appendCopyTimeoutText = computed(() => {
  const label = appendCopyTimeoutOptions.find((option) => option.value === store.appendCopyTimeoutMinutes)?.label ?? "1";
  return t("common.minutes", { value: label });
});

async function updatePanelOpenBehavior(behavior: PanelOpenBehavior) {
  await store.updatePanelOpenBehavior(behavior);
}

async function updatePanelLayout(layout: PanelLayout) {
  await store.updatePanelLayout(layout);
}

async function updateAppendCopyTimeout(minutes: number) {
  await store.updateAppendCopyTimeout(minutes);
}

async function updateLanguage(language: Language) {
  await store.updateLanguage(language);
}
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel settings-language-panel items-start">
      <div class="settings-icon settings-icon-teal">
        <Sparkles class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.language.title") }}
        </h2>
        <p class="mt-1 text-sm text-[var(--text-2)]">
          {{ t("settings.language.description") }}
        </p>
      </div>

      <LanguageSelect
        class="settings-language-select"
        :model-value="store.language"
        :options="languageOptions"
        :label="t('settings.language.title')"
        @update:model-value="updateLanguage"
      />
    </section>

    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-violet">
        <Sun class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.appearance.title") }}
        </h2>
        <p class="mt-1 text-sm text-[var(--text-2)]">
          {{ t("settings.appearance.description") }}
        </p>
      </div>

      <div
        class="segmented-control"
        role="radiogroup"
        :aria-label="t('settings.appearance.title')"
      >
        <button
          v-for="option in themeOptions"
          :key="option.value"
          type="button"
          class="segmented-option segmented-option-with-icon"
          :class="{ 'segmented-option-active': themePreference === option.value }"
          role="radio"
          :aria-checked="themePreference === option.value"
          @click="setThemePreference(option.value)"
        >
          <component
            :is="option.icon"
            class="size-3.5"
          />
          <span>{{ option.label }}</span>
        </button>
      </div>
    </section>

    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-teal">
        <Power class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.autostart.title") }}
        </h2>
        <p class="mt-1 text-sm text-[var(--text-2)]">
          {{ t("settings.autostart.description") }}
        </p>
        <p
          v-if="autostartError"
          class="settings-message settings-message-error mt-2"
        >
          <AlertCircle class="size-4" />
          <span>{{ autostartError }}</span>
        </p>
      </div>

      <button
        type="button"
        class="switch-control"
        :class="{ 'switch-control-active': autostartEnabled }"
        :disabled="isTogglingAutostart || !isTauri"
        :aria-pressed="autostartEnabled"
        :aria-label="t('settings.autostart.title')"
        @click="toggleAutostart"
      >
        <span />
      </button>
    </section>

    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-blue">
        <SlidersHorizontal class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.openDefault.title") }}
        </h2>
        <p class="mt-1 text-sm text-[var(--text-2)]">
          {{ t("settings.openDefault.description") }}
        </p>
      </div>

      <div class="segmented-control">
        <button
          v-for="option in panelOpenOptions"
          :key="option.value"
          type="button"
          class="segmented-option segmented-option-with-icon"
          :class="{ 'segmented-option-active': store.panelOpenBehavior === option.value }"
          @click="updatePanelOpenBehavior(option.value)"
        >
          <component
            :is="option.icon"
            class="size-3.5"
          />
          <span>{{ option.label }}</span>
        </button>
      </div>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-blue">
          <AppWindow class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.layout.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.layout.description") }}
          </p>
        </div>
      </div>

      <div class="settings-layout-options">
        <button
          v-for="option in panelLayoutOptions"
          :key="option.value"
          type="button"
          class="layout-option-button"
          :class="{ 'layout-option-button-active': store.panelLayout === option.value }"
          :aria-pressed="store.panelLayout === option.value"
          @click="updatePanelLayout(option.value)"
        >
          <span
            class="layout-option-preview"
            :class="`layout-option-preview-${option.value}`"
          >
            <span class="layout-preview-categories">
              <span />
              <span />
              <span />
            </span>
            <span class="layout-preview-list">
              <span />
              <span />
              <span />
              <span />
            </span>
          </span>
          <span class="layout-option-label">{{ option.label }}</span>
        </button>
      </div>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-teal">
          <ClipboardPlus class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.appendCopy.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.appendCopy.description", { duration: appendCopyTimeoutText }) }}
          </p>
        </div>
      </div>

      <div class="segmented-control settings-retention-control">
        <button
          v-for="option in appendCopyTimeoutOptions"
          :key="option.value"
          type="button"
          class="segmented-option"
          :class="{ 'segmented-option-active': store.appendCopyTimeoutMinutes === option.value }"
          @click="updateAppendCopyTimeout(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-blue">
          <Database class="size-5" />
        </div>
        <div class="min-w-0">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.storage.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.storage.description", { duration: retentionText }) }}
          </p>
        </div>
      </div>

      <div class="segmented-control settings-retention-control">
        <button
          v-for="option in retentionOptions"
          :key="option.value"
          type="button"
          class="segmented-option"
          :class="{ 'segmented-option-active': store.retentionDays === option.value }"
          @click="store.updateRetentionDays(option.value)"
        >
          {{ option.label }}
        </button>
      </div>

      <div class="settings-storage-actions">
        <button
          v-if="!confirmingClearHistory"
          type="button"
          class="settings-action-button settings-action-button-danger"
          :disabled="isClearingHistory"
          @click="requestClearHistory"
        >
          <Trash2 class="size-4" />
          <span>{{ t("settings.storage.clearHistory") }}</span>
        </button>

        <template v-else>
          <button
            type="button"
            class="settings-action-button settings-action-button-danger settings-action-button-confirm"
            :disabled="isClearingHistory"
            @click="confirmClearHistory"
          >
            <LoaderCircle
              v-if="isClearingHistory"
              class="size-4 update-spin"
            />
            <Trash2
              v-else
              class="size-4"
            />
            <span>{{ t("settings.storage.clearConfirm") }}</span>
          </button>
          <button
            type="button"
            class="settings-action-button"
            :disabled="isClearingHistory"
            @click="cancelClearHistory"
          >
            <span>{{ t("common.cancel") }}</span>
          </button>
        </template>
      </div>

      <p
        v-if="storageError || storageMessage"
        class="settings-message"
        :class="{ 'settings-message-error': storageError }"
      >
        <AlertCircle
          v-if="storageError"
          class="size-4"
        />
        <CheckCircle2
          v-else
          class="size-4"
        />
        <span>{{ storageError || storageMessage }}</span>
      </p>
    </section>
  </div>
</template>
