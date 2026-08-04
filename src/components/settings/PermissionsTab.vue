<script setup lang="ts">
import { ref } from "vue";
import { ChevronRight, Keyboard } from "lucide-vue-next";
import { t } from "../../i18n";
import { ipasteApi } from "../../lib/ipasteApi";

const showPermissionGuide = ref(false);

async function openAccessibilityGuide() {
  showPermissionGuide.value = true;
  await ipasteApi.openAccessibilitySettings();
}
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel items-start">
      <div class="settings-icon settings-icon-blue">
        <Keyboard class="size-5" />
      </div>

      <div class="min-w-0 flex-1">
        <h2 class="text-sm font-semibold text-slate-950">
          {{ t("settings.permissions.accessibility.title") }}
        </h2>
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

    <section
      v-if="showPermissionGuide"
      class="permission-guide"
    >
      <h3 class="text-sm font-semibold text-slate-950">
        {{ t("settings.permissions.howTo") }}
      </h3>
      <p class="mt-2 text-sm leading-6 text-slate-600">
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
