<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { ChevronRight, Keyboard, Monitor } from "lucide-vue-next";
import { t } from "../../i18n";
import { ipasteApi } from "../../lib/ipasteApi";

const showPermissionGuide = ref(false);
const screenRecordingGranted = ref(true);
const accessibilityGranted = ref(true);
let permissionPollTimer: ReturnType<typeof setInterval> | null = null;
let hasRequestedPermission = false;

async function openAccessibilityGuide() {
  showPermissionGuide.value = true;
  await ipasteApi.openAccessibilitySettings();
}

async function refreshPermissionStatuses() {
  accessibilityGranted.value = await ipasteApi.accessibilityPermissionStatus();
  screenRecordingGranted.value = await ipasteApi.screenCapturePermissionStatus();
}

async function openScreenRecordingSettings() {
  // 先触发系统授权弹框（未决定时），再打开系统设置便于用户手动勾选
  if (!hasRequestedPermission) {
    hasRequestedPermission = true;
    await ipasteApi.requestScreenCapturePermission();
  }
  await ipasteApi.openScreenRecordingSettings();
}

async function refreshScreenRecordingStatus() {
  await refreshPermissionStatuses();
}

function onWindowFocus() {
  void refreshPermissionStatuses();
}

onMounted(async () => {
  await refreshPermissionStatuses();
  window.addEventListener("focus", onWindowFocus);
  // 用户在系统设置勾选后回到 App，轮询让状态及时更新
  permissionPollTimer = setInterval(refreshScreenRecordingStatus, 2000);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", onWindowFocus);
  if (permissionPollTimer) {
    clearInterval(permissionPollTimer);
  }
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
          <span
            class="ml-2 text-xs font-normal"
            :class="accessibilityGranted ? 'text-[var(--accent)]' : 'text-[var(--text-3)]'"
          >
            {{ accessibilityGranted ? t("settings.permissions.screenRecording.granted") : t("settings.permissions.screenRecording.notGranted") }}
          </span>
        </h2>
        <p class="mt-1 text-sm leading-6 text-[var(--text-2)]">
          {{ t("settings.permissions.accessibility.description") }}
        </p>
        <p
          v-if="!accessibilityGranted"
          class="mt-1 text-xs leading-5 text-[var(--text-3)]"
        >
          {{ t("settings.permissions.accessibility.restartHint") }}
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
        <p
          v-if="!screenRecordingGranted"
          class="mt-1 text-xs leading-5 text-[var(--text-3)]"
        >
          {{ t("settings.permissions.screenRecording.restartHint") }}
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
