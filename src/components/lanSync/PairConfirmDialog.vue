<script setup lang="ts">
import { t } from "../../i18n";
import type { PairRequested } from "../../types/generated/PairRequested";

defineProps<{
  /** 待确认的配对请求（面板以 v-if 保证非空）。 */
  request: PairRequested;
}>();

const emit = defineEmits<{
  accept: [];
  reject: [];
}>();
</script>

<template>
  <div
    class="lan-pair-overlay"
    role="dialog"
    aria-modal="true"
    :aria-label="t('deviceSync.pair.title')"
  >
    <div class="lan-pair-dialog">
      <h3 class="lan-pair-title">{{ t("deviceSync.pair.title") }}</h3>
      <p class="lan-pair-device">{{ request.deviceName }}</p>
      <p class="lan-pair-fingerprint">
        {{ t("deviceSync.pair.fingerprint") }}: {{ request.fingerprint }}
      </p>
      <div class="lan-pair-actions">
        <button
          type="button"
          class="lan-button lan-button-primary"
          @click="emit('accept')"
        >
          {{ t("deviceSync.pair.accept") }}
        </button>
        <button
          type="button"
          class="lan-button"
          @click="emit('reject')"
        >
          {{ t("deviceSync.pair.decline") }}
        </button>
      </div>
    </div>
  </div>
</template>
