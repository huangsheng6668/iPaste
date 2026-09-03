<script setup lang="ts">
import { computed } from "vue";
import { CheckCircle2, Cloud, HardDrive, Unplug } from "lucide-vue-next";
import { t } from "../../i18n";
import { pluralText } from "../../lib/format";
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

const totalItems = computed(() => store.clips.length + store.categoryItems.length);
const imageCount = computed(() => store.clips.filter((c) => c.clipType === "image").length);
const textCount = computed(() => store.clips.length - imageCount.value);
const categoryCount = computed(() => store.categories.length);

const textPercent = computed(() => (totalItems.value ? Math.max(5, Math.round((textCount.value / (totalItems.value + categoryCount.value || 1)) * 100)) : 50));
const imagePercent = computed(() => (totalItems.value ? Math.max(5, Math.round((imageCount.value / (totalItems.value + categoryCount.value || 1)) * 100)) : 30));
const categoryPercent = computed(() => (100 - textPercent.value - imagePercent.value > 0 ? 100 - textPercent.value - imagePercent.value : 10));
</script>

<template>
  <div class="settings-section">
    <div class="data-management-grid">
      <!-- Storage Overview Breakdown -->
      <section class="settings-panel settings-column-panel">
        <div class="settings-panel-heading">
          <div class="settings-icon settings-icon-blue">
            <HardDrive class="size-5" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="text-sm font-semibold text-[var(--text-1)]">
              {{ t("settings.storage.title") }}
            </h2>
            <p class="mt-1 text-sm text-[var(--text-2)] tabular-nums">
              {{ pluralText("settings.storage.summaryItemsOne", "settings.storage.summaryItemsOther", totalItems) }}
              ·
              {{ pluralText("settings.storage.summaryCategoriesOne", "settings.storage.summaryCategoriesOther", categoryCount) }}
            </p>
          </div>
        </div>

        <div class="storage-breakdown-wrap">
          <div class="storage-breakdown-bar">
            <div
              class="storage-breakdown-segment"
              :style="{ width: `${textPercent}%`, backgroundColor: 'var(--accent)' }"
              :title="t('settings.storage.legendText', { value: textCount })"
            />
            <div
              class="storage-breakdown-segment"
              :style="{ width: `${imagePercent}%`, backgroundColor: 'var(--info)' }"
              :title="t('settings.storage.legendImages', { value: imageCount })"
            />
            <div
              class="storage-breakdown-segment"
              :style="{ width: `${categoryPercent}%`, backgroundColor: 'var(--warning)' }"
              :title="t('settings.storage.legendCategories', { value: categoryCount })"
            />
          </div>

          <div class="storage-breakdown-legend tabular-nums">
            <div class="storage-legend-item">
              <span
                class="storage-legend-dot"
                :style="{ backgroundColor: 'var(--accent)' }"
              />
              <span>{{ t("settings.storage.legendText", { value: textCount }) }}</span>
            </div>
            <div class="storage-legend-item">
              <span
                class="storage-legend-dot"
                :style="{ backgroundColor: 'var(--info)' }"
              />
              <span>{{ t("settings.storage.legendImages", { value: imageCount }) }}</span>
            </div>
            <div class="storage-legend-item">
              <span
                class="storage-legend-dot"
                :style="{ backgroundColor: 'var(--warning)' }"
              />
              <span>{{ t("settings.storage.legendCategories", { value: categoryCount }) }}</span>
            </div>
          </div>
        </div>
      </section>

      <!-- Cloud Sync Panel -->
      <section class="settings-panel settings-column-panel">
        <div class="settings-panel-heading">
          <div class="settings-icon settings-icon-teal">
            <Cloud class="size-5" />
          </div>
          <div class="min-w-0">
            <h2 class="text-sm font-semibold text-[var(--text-1)]">
              {{ t("settings.cloud.title") }}
            </h2>
            <p class="mt-1 text-sm text-[var(--text-2)]">
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
