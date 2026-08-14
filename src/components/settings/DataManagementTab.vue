<script setup lang="ts">
import { computed } from "vue";
import { CheckCircle2, Cloud, Unplug } from "lucide-vue-next";
import { t } from "../../i18n";
import { useIpasteStore } from "../../stores/ipasteStore";
import { useCloudSync } from "../../composables/useCloudSync";

const store = useIpasteStore();
const {
  cloudApiAddress,
  cloudApiKey,
  cloudMessage,
  cloudError,
  isTestingCloud,
  isSavingCloud,
  cloudStatusText,
  testCloud,
  saveCloud,
  disableCloud,
} = useCloudSync();

const isInsecureHttpAddress = computed(() =>
  cloudApiAddress.value.trim().toLowerCase().startsWith("http://"),
);
</script>

<template>
  <div class="settings-section">
    <div class="data-management-grid">
      <section class="settings-panel settings-column-panel">
        <div class="settings-panel-heading">
          <div class="settings-icon settings-icon-teal">
            <Cloud class="size-5" />
          </div>
          <div class="min-w-0">
            <h2 class="text-sm font-semibold text-slate-950">
              {{ t("settings.cloud.title") }}
            </h2>
            <p class="mt-1 text-sm text-slate-500">
              {{ cloudStatusText }}
            </p>
          </div>
        </div>

        <p class="sync-hint">
          {{ t("settings.cloud.description") }}
        </p>

        <label class="settings-field">
          <span>{{ t("settings.cloud.apiAddress") }}</span>
          <input
            v-model="cloudApiAddress"
            type="url"
            placeholder="https://your-project.pages.dev"
            spellcheck="false"
          >
        </label>

        <p
          v-if="isInsecureHttpAddress"
          class="settings-message settings-message-error"
        >
          {{ t("settings.cloud.insecureWarning") }}
        </p>

        <label class="settings-field">
          <span>API Key</span>
          <input
            v-model="cloudApiKey"
            type="password"
            autocomplete="current-password"
          >
        </label>

        <p
          v-if="cloudError || cloudMessage"
          class="settings-message"
          :class="{ 'settings-message-error': cloudError }"
        >
          <CheckCircle2
            v-if="cloudMessage && !cloudError"
            class="size-4"
          />
          <Unplug
            v-else
            class="size-4"
          />
          <span>{{ cloudError || cloudMessage }}</span>
        </p>

        <div class="settings-action-row">
          <button
            type="button"
            class="settings-action-button"
            :disabled="isTestingCloud"
            @click="testCloud"
          >
            <CheckCircle2 class="size-4" />
            <span>{{ isTestingCloud ? t("settings.cloud.testing") : t("settings.cloud.test") }}</span>
          </button>
          <button
            type="button"
            class="settings-action-button settings-action-button-primary"
            :disabled="isSavingCloud"
            @click="saveCloud"
          >
            <Cloud class="size-4" />
            <span>{{ isSavingCloud ? t("common.saving") : t("settings.cloud.saveAndSync") }}</span>
          </button>
          <button
            type="button"
            class="settings-action-button settings-action-button-danger"
            :disabled="isSavingCloud || !store.cloud.enabled"
            @click="disableCloud"
          >
            <Unplug class="size-4" />
            <span>{{ t("settings.cloud.disable") }}</span>
          </button>
        </div>
      </section>
    </div>
  </div>
</template>
