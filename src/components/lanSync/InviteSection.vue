<script setup lang="ts">
import { Check, Copy } from "lucide-vue-next";
import { t } from "../../i18n";

defineProps<{
  ticket: string | null;
  /** mm:ss 倒计时文案（useInviteCountdown.countdown）。 */
  countdown: string;
  /** 票据存在且倒计时已归零（面板组合语义）。 */
  expired: boolean;
  creating: boolean;
  error: string | null;
  /** 复制结果反馈：ok / error / null（面板持有 2s 重置定时器）。 */
  copied: string | null;
}>();

const emit = defineEmits<{
  create: [];
  cancel: [];
  copyTicket: [];
}>();
</script>

<template>
  <!-- 邀请设备（host 侧） -->
  <section class="lan-section">
    <h2 class="lan-section-title">{{ t("deviceSync.invite.title") }}</h2>
    <template v-if="ticket">
      <div class="lan-ticket-row">
        <input
          class="lan-input"
          :class="{ 'lan-ticket-expired': expired }"
          readonly
          :value="ticket"
          aria-readonly="true"
        >
        <button
          type="button"
          class="lan-button"
          :disabled="expired"
          @click="emit('copyTicket')"
        >
          <Check
            v-if="copied === 'ok'"
            :size="14"
          />
          <Copy
            v-else
            :size="14"
          />
          {{ copied === "ok" ? t("deviceSync.invite.copied") : t("deviceSync.invite.copy") }}
        </button>
      </div>
      <p
        v-if="copied === 'error'"
        class="lan-error"
      >
        {{ t("deviceSync.invite.copyFailed") }}
      </p>
      <div class="lan-ticket-row">
        <span class="lan-hint">
          {{
            expired
              ? t("deviceSync.invite.expired")
              : t("deviceSync.invite.expiresIn", { time: countdown })
          }}
        </span>
        <button
          type="button"
          class="lan-button"
          @click="emit('cancel')"
        >
          {{ t("deviceSync.invite.cancel") }}
        </button>
      </div>
    </template>
    <button
      v-else
      type="button"
      class="lan-button lan-button-primary"
      :disabled="creating"
      @click="emit('create')"
    >
      {{ t("deviceSync.invite.button") }}
    </button>
    <p
      v-if="error"
      class="lan-error"
    >
      {{ error }}
    </p>
  </section>
</template>
