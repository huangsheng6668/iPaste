<script setup lang="ts">
import { onMounted, ref } from "vue";
import { ChevronRight, Keyboard, Monitor } from "lucide-vue-next";
import { t } from "../../i18n";
import { ipasteApi } from "../../lib/ipasteApi";

const showPermissionGuide = ref(false);
const screenRecordingGranted = ref(true);

async function openAccessibilityGuide() {
  showPermissionGuide.value = true;
  await ipasteApi.openAccessibilitySettings();
}

async function openScreenRecordingSettings() {
  await ipasteApi.openScreenRecordingSettings();
}

onMounted(async () => {
  screenRecordingGranted.value = await ipasteApi.screenCapturePermissionStatus();
});
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-blue">
        <Keyboard class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.permissions.accessibility.title") }}
        </h2>
        <p class="mt-1 text-sm leading-6 text-[var(--text-2)]">
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

    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-blue">
        <Monitor class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("settings.permissions.screenRecording.title") }}
          <span
            class="ml-2 text-xs font-normal"
            :class="screenRecordingGranted ? 'text-[var(--accent)]' : 'text-[var(--text-3)]'"
          >
            {{ screenRecordingGranted ? t("settings.permissions.screenRecording.granted") : t("settings.permissions.screenRecording.notGranted") }}
          </span>
        </h2>
        <p class="mt-1 text-sm leading-6 text-[var(--text-2)]">
          {{ t("settings.permissions.screenRecording.description") }}
        </p>
      </div>

      <button
        type="button"
        class="permission-link"
        @click="openScreenRecordingSettings"
      >
        <span>{{ t("settings.permissions.screenRecording.open") }}</span>
        <ChevronRight class="size-4" />
      </button>
    </section>

    <section
      v-if="showPermissionGuide"
      class="permission-guide"
    >
      <h3 class="text-sm font-semibold text-[var(--text-1)]">
        {{ t("settings.permissions.howTo") }}
      </h3>
      <p class="mt-2 text-sm leading-6 text-[var(--text-2)]">
        {{ t("settings.permissions.guide") }}
      </p>
      <button
        type="button"
        class="permission-link"
        @click="openAccessibilityGuide"
      >
        <span>{{ t("settings.permissions.open") }}</span>
        <ChevronRight class="size-4" />
      </button>
    </section>
  </div>
</template>
