<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  Database,
  Keyboard,
  ScanText,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
} from "lucide-vue-next";
import ShortcutsTab from "./settings/ShortcutsTab.vue";
import OcrTab from "./settings/OcrTab.vue";
import DataManagementTab from "./settings/DataManagementTab.vue";
import GeneralTab from "./settings/GeneralTab.vue";
import PermissionsTab from "./settings/PermissionsTab.vue";
import AboutTab from "./settings/AboutTab.vue";
import UpdateDialog from "./UpdateDialog.vue";
import { useUpdater } from "../composables/useUpdater";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "../stores/ipasteStore";
import type { AppInfo } from "../types";

const store = useIpasteStore();
type SettingsTab = "general" | "shortcuts" | "ocr" | "dataManagement" | "permissions" | "about";
const validTabs = new Set<SettingsTab>([
  "general", "shortcuts", "ocr", "dataManagement", "permissions", "about",
]);
const requestedTab = new URLSearchParams(window.location.search).get("tab") as SettingsTab | null;
const activeTab = ref<SettingsTab>(
  requestedTab && validTabs.has(requestedTab) ? requestedTab : "general",
);
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

onMounted(async () => {
  await store.load();
  appInfo.value = await ipasteApi.appInfo();
});

</script>

<template>
  <main class="settings-shell">
    <section class="settings-window">
      <header class="settings-topbar">
        <nav
          class="settings-tabs"
          :aria-label="t('settings.tabsLabel')"
        >
          <button
            v-for="tab in tabs"
            :key="tab.id"
            type="button"
            class="settings-tab"
            :class="{ 'settings-tab-active': activeTab === tab.id }"
            @click="activeTab = tab.id"
          >
            <component
              :is="tab.icon"
              class="size-4"
            />
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

        <AboutTab
          v-else
          :updater="updater"
          :app-info="appInfo"
        />
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
