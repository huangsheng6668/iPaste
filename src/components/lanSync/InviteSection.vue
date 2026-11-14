<script setup lang="ts">
import { computed } from "vue";
import { Check, Copy } from "lucide-vue-next";
import { t } from "../../i18n";

const props = defineProps<{
  /** 6 位 PIN（同局域网短码配对）；无有效邀请时为 null。 */
  pin: string | null;
  ticket: string | null;
  /** 本机指纹（endpoint_id 前 8 hex）：guest 确认步核对用（H2 防钓鱼）。 */
  fingerprint: string | null;
  /** mm:ss 倒计时文案（useInviteCountdown.countdown）。 */
  countdown: string;
  /** 票据存在且倒计时已归零（面板组合语义）。 */
  expired: boolean;
  creating: boolean;
  error: string | null;
  /** 复制结果反馈：ok / error / null（面板持有 2s 重置定时器）。 */
  copied: string | null;
}>();

/** 三位分组展示，便于口头/目视抄写。 */
const formattedPin = computed(() =>
  props.pin ? `${props.pin.slice(0, 3)} ${props.pin.slice(3)}` : "",
);

const emit = defineEmits<{
  create: [];
  cancel: [];
  copyTicket: [];
}>();
</script>

<template>
  <!-- 邀请设备（host 侧） -->
  <section class="lan-section">
    <h2 class="lan-section-title">
      {{ t("deviceSync.invite.title") }}
    </h2>
    <template v-if="ticket">
      <div class="lan-pin-row">
        <span class="lan-label">{{ t("deviceSync.invite.pinLabel") }}</span>
        <span
          class="lan-pin"
          :class="{ 'lan-pin-expired': expired }"
          aria-live="polite"
        >{{ formattedPin }}</span>
      </div>
      <div
        v-if="fingerprint"
        class="lan-pin-row"
      >
        <span class="lan-label">{{ t("deviceSync.invite.fingerprintLabel") }}</span>
        <span class="font-mono text-sm text-[var(--text-1)]">{{ fingerprint }}</span>
      </div>
      <p class="lan-hint">
        {{ t("deviceSync.invite.pinHint") }}
      </p>
      <details class="lan-collapsible">
        <summary class="lan-collapsible-header">
          {{ t("deviceSync.invite.ticketFallback") }}
        </summary>
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
              class="size-3.5"
            />
            <Copy
              v-else
              class="size-3.5"
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
      </details>
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
