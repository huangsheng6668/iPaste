<script setup lang="ts">
import { computed } from "vue";
import { Play, Terminal } from "lucide-vue-next";
import { t } from "../i18n";
import type { AutomationAction } from "../types";

const props = defineProps<{ action: AutomationAction; selected: boolean }>();
const emit = defineEmits<{
  (e: "run"): void;
  (e: "edit"): void;
  (e: "copy"): void;
  (e: "delete"): void;
  (e: "open-context-menu", payload: { x: number; y: number }): void;
}>();

const status = computed(() => props.action.lastRun?.status ?? "idle");
const statusLabel = computed(() => {
  const map: Record<string, string> = {
    idle: t("automation.statusIdle"),
    running: t("automation.statusRunning"),
    success: t("automation.statusSuccess"),
    failed: t("automation.statusFailed"),
    timed_out: t("automation.statusTimedOut"),
  };
  return map[status.value] ?? "";
});
</script>

<template>
  <div
    class="clip-card"
    :class="{
      'clip-card-selected': selected,
      'automation-card-running': status === 'running',
      'automation-card-failed': status === 'failed' || status === 'timed_out',
    }"
    tabindex="-1"
    :data-automation-id="action.id"
    @dblclick="emit('run')"
    @contextmenu.prevent="emit('open-context-menu', { x: $event.clientX, y: $event.clientY })"
  >
    <div class="flex items-center gap-2">
      <Terminal class="size-4 shrink-0 text-slate-400" />
      <span class="truncate text-sm font-medium text-slate-900">{{ action.name }}</span>
      <span
        class="automation-status-badge"
        :class="`automation-status-${status}`"
      >
        {{ statusLabel }}
      </span>
    </div>
    <div class="mt-1 flex items-center gap-1 truncate font-mono text-xs text-slate-500">
      <Play class="size-3 shrink-0" />
      <span class="truncate">{{ action.command }}</span>
    </div>
    <p
      v-if="action.cwd"
      class="mt-0.5 truncate text-xs text-slate-400"
    >
      {{ action.cwd }}
    </p>
  </div>
</template>
