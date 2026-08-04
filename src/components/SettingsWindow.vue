<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  AlertCircle,
  AppWindow,
  Blocks,
  Box,
  Brush,
  CheckCircle2,
  Cpu,
  Database,
  Download,
  Keyboard,
  RefreshCw,
  ScanText,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  SquareCode,
  Zap,
} from "lucide-vue-next";
import ShortcutsTab from "./settings/ShortcutsTab.vue";
import OcrTab from "./settings/OcrTab.vue";
import DataManagementTab from "./settings/DataManagementTab.vue";
import GeneralTab from "./settings/GeneralTab.vue";
import PermissionsTab from "./settings/PermissionsTab.vue";
import UpdateDialog from "./UpdateDialog.vue";
import { useUpdater } from "../composables/useUpdater";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "../stores/ipasteStore";
import type { AppInfo } from "../types";

const store = useIpasteStore();
type SettingsTab = "general" | "shortcuts" | "ocr" | "dataManagement" | "permissions" | "about";
const activeTab = ref<SettingsTab>("general");
const appInfo = ref<AppInfo | null>(null);
const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const updater = useUpdater();

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

onMounted(async () => {
  await store.load();
  appInfo.value = await ipasteApi.appInfo();
});

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
        <GeneralTab v-if="activeTab === 'general'" />

        <ShortcutsTab v-else-if="activeTab === 'shortcuts'" />

        <OcrTab v-else-if="activeTab === 'ocr'" />

        <DataManagementTab v-else-if="activeTab === 'dataManagement'" />

        <PermissionsTab v-else-if="activeTab === 'permissions'" />

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
