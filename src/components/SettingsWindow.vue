<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  AlertCircle,
  AppWindow,
  Blocks,
  Box,
  Brush,
  CheckCircle2,
  ChevronRight,
  Cloud,
  ClipboardPlus,
  Cpu,
  Database,
  Download,
  History,
  Keyboard,
  LoaderCircle,
  Power,
  RefreshCw,
  ScanText,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  SquareCode,
  Tags,
  Trash2,
  Unplug,
  Zap,
} from "lucide-vue-next";
import LanguageSelect from "./LanguageSelect.vue";
import ShortcutsTab from "./settings/ShortcutsTab.vue";
import OcrTab from "./settings/OcrTab.vue";
import UpdateDialog from "./UpdateDialog.vue";
import { useUpdater } from "../composables/useUpdater";
import { languageOptions, t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "../stores/ipasteStore";
import type { AppInfo, Language, PanelLayout, PanelOpenBehavior } from "../types";

const store = useIpasteStore();
type SettingsTab = "general" | "shortcuts" | "ocr" | "dataManagement" | "permissions" | "about";
const activeTab = ref<SettingsTab>("general");
const showPermissionGuide = ref(false);
const cloudApiAddress = ref("");
const cloudApiKey = ref("");
const cloudMessage = ref<string | null>(null);
const cloudError = ref<string | null>(null);
const isTestingCloud = ref(false);
const isSavingCloud = ref(false);
const appInfo = ref<AppInfo | null>(null);
const autostartEnabled = ref(false);
const isTogglingAutostart = ref(false);
const autostartError = ref<string | null>(null);
const isClearingHistory = ref(false);
const confirmingClearHistory = ref(false);
const storageMessage = ref<string | null>(null);
const storageError = ref<string | null>(null);
const isTauri = "__TAURI_INTERNALS__" in window;
const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const updater = useUpdater();

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

const tabs = computed(() => {
  const items: Array<{ id: SettingsTab; label: string; icon: typeof SlidersHorizontal }> = [
    { id: "general", label: t("settings.tabs.general"), icon: SlidersHorizontal },
    { id: "shortcuts", label: t("settings.tabs.shortcuts"), icon: Keyboard },
    { id: "dataManagement", label: t("settings.tabs.dataManagement"), icon: Database },
    { id: "about", label: t("settings.tabs.about"), icon: Sparkles },
  ];

  if (!isMacOs) {
    items.splice(2, 0, { id: "ocr", label: t("settings.tabs.ocr"), icon: ScanText });
  }

  if (isMacOs) {
    items.splice(3, 0, { id: "permissions", label: t("settings.tabs.permissions"), icon: ShieldCheck });
  }

  return items;
});

const techStack = computed(() => [
  { name: "Vue 3", detail: t("settings.tech.vue"), icon: Blocks, tone: "emerald" },
  { name: "TypeScript", detail: t("settings.tech.ts"), icon: SquareCode, tone: "blue" },
  { name: "Tauri 2", detail: t("settings.tech.tauri"), icon: AppWindow, tone: "teal" },
  { name: "Rust", detail: t("settings.tech.rust"), icon: Cpu, tone: "slate" },
  { name: "Pinia", detail: t("settings.tech.pinia"), icon: Box, tone: "amber" },
  { name: "Tailwind CSS", detail: t("settings.tech.tailwind"), icon: Brush, tone: "sky" },
  { name: "Vite", detail: t("settings.tech.vite"), icon: Zap, tone: "violet" },
  { name: "SQLite", detail: t("settings.tech.sqlite"), icon: Database, tone: "indigo" },
]);

const retentionText = computed(() => {
  return retentionOptions.value.find((option) => option.value === store.retentionDays)?.label ?? t("settings.retention.30");
});

const appendCopyTimeoutText = computed(() => {
  const label = appendCopyTimeoutOptions.find((option) => option.value === store.appendCopyTimeoutMinutes)?.label ?? "1";
  return t("common.minutes", { value: label });
});

const cloudStatusText = computed(() => {
  return store.cloud.enabled ? t("settings.cloud.enabled") : t("settings.cloud.disabled");
});
onMounted(async () => {
  await store.load();
  appInfo.value = await ipasteApi.appInfo();
  await loadAutostartStatus();
  resetCloudForm();
});

async function openAccessibilityGuide() {
  showPermissionGuide.value = true;
  await ipasteApi.openAccessibilitySettings();
}

function resetCloudForm() {
  cloudApiAddress.value = store.cloud.apiAddress;
  cloudApiKey.value = store.cloud.apiKey;
  cloudMessage.value = null;
  cloudError.value = null;
}

async function testCloud() {
  cloudMessage.value = null;
  cloudError.value = null;
  isTestingCloud.value = true;
  try {
    await store.testCloudSettings(cloudApiAddress.value, cloudApiKey.value);
    cloudMessage.value = t("settings.cloud.connected");
  } catch (unknownError) {
    cloudError.value = String(unknownError);
  } finally {
    isTestingCloud.value = false;
  }
}

async function saveCloud() {
  cloudMessage.value = null;
  cloudError.value = null;
  isSavingCloud.value = true;
  try {
    await store.saveCloudSettings(cloudApiAddress.value, cloudApiKey.value);
    resetCloudForm();
    cloudMessage.value = t("settings.cloud.saved");
  } catch (unknownError) {
    cloudError.value = String(unknownError);
  } finally {
    isSavingCloud.value = false;
  }
}

async function disableCloud() {
  cloudMessage.value = null;
  cloudError.value = null;
  isSavingCloud.value = true;
  try {
    await store.disableCloudSync();
    resetCloudForm();
    cloudMessage.value = t("settings.cloud.disabledMessage");
  } catch (unknownError) {
    cloudError.value = String(unknownError);
  } finally {
    isSavingCloud.value = false;
  }
}

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

function requestClearHistory() {
  storageMessage.value = null;
  storageError.value = null;
  confirmingClearHistory.value = true;
}

function cancelClearHistory() {
  confirmingClearHistory.value = false;
}

async function confirmClearHistory() {
  if (isClearingHistory.value) return;
  isClearingHistory.value = true;
  storageMessage.value = null;
  storageError.value = null;
  try {
    const deleted = await store.clearHistory();
    confirmingClearHistory.value = false;
    storageMessage.value = t("settings.storage.cleared", { count: deleted });
  } catch (unknownError) {
    storageError.value = String(unknownError);
  } finally {
    isClearingHistory.value = false;
  }
}

async function loadAutostartStatus() {
  if (!isTauri) return;
  try {
    autostartEnabled.value = await ipasteApi.isAutostartEnabled();
  } catch (unknownError) {
    autostartError.value = String(unknownError);
  }
}

async function toggleAutostart() {
  if (isTogglingAutostart.value) return;
  autostartError.value = null;
  isTogglingAutostart.value = true;
  try {
    autostartEnabled.value = autostartEnabled.value
      ? await ipasteApi.disableAutostart()
      : await ipasteApi.enableAutostart();
  } catch (unknownError) {
    autostartError.value = String(unknownError);
  } finally {
    isTogglingAutostart.value = false;
  }
}

</script>

<template>
  <main class="settings-shell">
    <section class="settings-window">
      <header class="settings-topbar">
        <nav class="settings-tabs" :aria-label="t('settings.tabsLabel')">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            type="button"
            class="settings-tab"
            :class="{ 'settings-tab-active': activeTab === tab.id }"
            @click="activeTab = tab.id"
          >
            <component :is="tab.icon" class="size-4" />
            <span>{{ tab.label }}</span>
          </button>
        </nav>
      </header>

      <div class="settings-content subtle-scrollbar">
        <div v-if="activeTab === 'general'" class="settings-section">
          <section class="settings-panel settings-language-panel items-start">
            <div class="settings-icon settings-icon-teal">
              <Sparkles class="size-5" />
            </div>

            <div class="min-w-0 flex-1">
              <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.language.title") }}</h2>
              <p class="mt-1 text-sm text-slate-500">{{ t("settings.language.description") }}</p>
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
            <div class="settings-icon settings-icon-teal">
              <Power class="size-5" />
            </div>

            <div class="min-w-0 flex-1">
              <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.autostart.title") }}</h2>
              <p class="mt-1 text-sm text-slate-500">{{ t("settings.autostart.description") }}</p>
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
              <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.openDefault.title") }}</h2>
              <p class="mt-1 text-sm text-slate-500">{{ t("settings.openDefault.description") }}</p>
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
                <component :is="option.icon" class="size-3.5" />
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
                <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.layout.title") }}</h2>
                <p class="mt-1 text-sm text-slate-500">{{ t("settings.layout.description") }}</p>
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
                <span class="layout-option-preview" :class="`layout-option-preview-${option.value}`">
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
                <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.appendCopy.title") }}</h2>
                <p class="mt-1 text-sm text-slate-500">{{ t("settings.appendCopy.description", { duration: appendCopyTimeoutText }) }}</p>
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
                <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.storage.title") }}</h2>
                <p class="mt-1 text-sm text-slate-500">{{ t("settings.storage.description", { duration: retentionText }) }}</p>
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
                  <LoaderCircle v-if="isClearingHistory" class="size-4 update-spin" />
                  <Trash2 v-else class="size-4" />
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
              <AlertCircle v-if="storageError" class="size-4" />
              <CheckCircle2 v-else class="size-4" />
              <span>{{ storageError || storageMessage }}</span>
            </p>
          </section>

        </div>

        <ShortcutsTab v-else-if="activeTab === 'shortcuts'" />

        <OcrTab v-else-if="activeTab === 'ocr'" />

        <div v-else-if="activeTab === 'dataManagement'" class="settings-section">
          <div class="data-management-grid">
            <section class="settings-panel settings-column-panel">
              <div class="settings-panel-heading">
                <div class="settings-icon settings-icon-teal">
                  <Cloud class="size-5" />
                </div>
                <div class="min-w-0">
                  <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.cloud.title") }}</h2>
                  <p class="mt-1 text-sm text-slate-500">{{ cloudStatusText }}</p>
                </div>
              </div>

              <p class="sync-hint">
                {{ t("settings.cloud.description") }}
              </p>

              <label class="settings-field">
                <span>{{ t("settings.cloud.apiAddress") }}</span>
                <input v-model="cloudApiAddress" type="url" placeholder="https://your-project.pages.dev" spellcheck="false" />
              </label>

              <label class="settings-field">
                <span>API Key</span>
                <input v-model="cloudApiKey" type="password" autocomplete="current-password" />
              </label>

              <p v-if="cloudError || cloudMessage" class="settings-message" :class="{ 'settings-message-error': cloudError }">
                <CheckCircle2 v-if="cloudMessage && !cloudError" class="size-4" />
                <Unplug v-else class="size-4" />
                <span>{{ cloudError || cloudMessage }}</span>
              </p>

              <div class="settings-action-row">
                <button type="button" class="settings-action-button" :disabled="isTestingCloud" @click="testCloud">
                  <CheckCircle2 class="size-4" />
                  <span>{{ isTestingCloud ? t("settings.cloud.testing") : t("settings.cloud.test") }}</span>
                </button>
                <button type="button" class="settings-action-button settings-action-button-primary" :disabled="isSavingCloud" @click="saveCloud">
                  <Cloud class="size-4" />
                  <span>{{ isSavingCloud ? t("common.saving") : t("settings.cloud.saveAndSync") }}</span>
                </button>
                <button type="button" class="settings-action-button settings-action-button-danger" :disabled="isSavingCloud || !store.cloud.enabled" @click="disableCloud">
                  <Unplug class="size-4" />
                  <span>{{ t("settings.cloud.disable") }}</span>
                </button>
              </div>
            </section>
          </div>
        </div>

        <div v-else-if="activeTab === 'permissions'" class="settings-section">
          <section class="settings-panel items-start">
            <div class="settings-icon settings-icon-blue">
              <Keyboard class="size-5" />
            </div>

            <div class="min-w-0 flex-1">
              <h2 class="text-sm font-semibold text-slate-950">{{ t("settings.permissions.accessibility.title") }}</h2>
              <p class="mt-1 text-sm leading-6 text-slate-500">
                {{ t("settings.permissions.accessibility.description") }}
              </p>
            </div>

            <button
              type="button"
              class="switch-control"
              :class="{ 'switch-control-active': showPermissionGuide }"
              :aria-label="t('settings.permissions.showGuide')"
              @click="openAccessibilityGuide"
            >
              <span />
            </button>
          </section>

          <section v-if="showPermissionGuide" class="permission-guide">
            <h3 class="text-sm font-semibold text-slate-950">{{ t("settings.permissions.howTo") }}</h3>
            <p class="mt-2 text-sm leading-6 text-slate-600">
              {{ t("settings.permissions.guide") }}
            </p>
            <button type="button" class="permission-link" @click="openAccessibilityGuide">
              <span>{{ t("settings.permissions.open") }}</span>
              <ChevronRight class="size-4" />
            </button>
          </section>
        </div>

        <div v-else class="settings-section">
          <section class="settings-panel settings-about-panel">
            <div class="settings-about-header">
              <div class="settings-icon settings-icon-violet">
                <Sparkles class="size-5" />
              </div>
              <div class="min-w-0">
                <h2 class="text-sm font-semibold text-slate-950">iPaste</h2>
                <p class="mt-1 text-sm text-slate-500">{{ t("settings.about.description") }}</p>
              </div>
            </div>

            <section class="about-update-panel" :class="{ 'about-update-panel-error': updater.updateStatus.value === 'error' }">
              <div class="about-update-copy">
                <div class="about-update-icon" :class="{ 'about-update-icon-error': updater.updateStatus.value === 'error' }">
                  <AlertCircle v-if="updater.updateStatus.value === 'error'" class="size-4" />
                  <Download v-else-if="updater.updateStatus.value === 'available' || updater.updateStatus.value === 'downloading'" class="size-4" />
                  <CheckCircle2 v-else-if="updater.updateStatus.value === 'noUpdate' || updater.updateStatus.value === 'ready'" class="size-4" />
                  <RefreshCw v-else class="size-4" />
                </div>
                <div class="min-w-0">
                  <div class="about-update-heading">
                    <h3 class="about-update-title">{{ t("settings.about.softwareUpdate") }}</h3>
                    <span class="about-version-badge">v{{ appInfo?.version ?? "0.1.0" }}</span>
                  </div>
                  <p>{{ updater.updateSummaryText.value }}</p>
                </div>
              </div>

              <button
                type="button"
                class="settings-action-button settings-action-button-primary about-update-button"
                :disabled="updater.isUpdateBusy.value"
                @click="updater.checkForUpdate()"
              >
                <RefreshCw class="size-4" :class="{ 'update-spin': updater.updateStatus.value === 'checking' }" />
                <span>{{ updater.updateButtonText.value }}</span>
              </button>
            </section>

            <div>
              <h3 class="about-label">{{ t("settings.about.techStack") }}</h3>
              <div class="tech-stack-grid">
                <div v-for="item in techStack" :key="item.name" class="tech-stack-item">
                  <div class="tech-stack-icon" :class="`tech-stack-icon-${item.tone}`">
                    <component :is="item.icon" class="size-4" />
                  </div>
                  <div class="min-w-0">
                    <strong>{{ item.name }}</strong>
                    <span>{{ item.detail }}</span>
                  </div>
                </div>
              </div>
            </div>
          </section>
        </div>
      </div>
    </section>

    <UpdateDialog
      :open="updater.updateDialogOpen.value"
      :status="updater.updateStatus.value"
      :update="updater.availableUpdate.value"
      :current-version="appInfo?.version"
      :error="updater.updateError.value"
      :error-phase="updater.updateErrorPhase.value"
      :downloaded-bytes="updater.updateDownloadedBytes.value"
      :total-bytes="updater.updateTotalBytes.value"
      @dismiss="updater.dismissUpdateDialog"
      @install="updater.installAvailableUpdate"
      @relaunch="updater.relaunchForUpdate"
    />
  </main>
</template>
