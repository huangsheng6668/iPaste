<script setup lang="ts">
import { t } from "../i18n";
import type { AutomationAction } from "../types";

defineProps<{ open: boolean; action: AutomationAction | null }>();
const emit = defineEmits<{ (e: "confirm"): void; (e: "cancel"): void }>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && action"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40"
      @click.self="emit('cancel')"
    >
      <div
        class="w-[460px] max-w-[90vw] rounded-xl bg-white p-5 shadow-xl"
        role="dialog"
        @keydown.esc="emit('cancel')"
      >
        <h2 class="text-base font-semibold text-slate-900">
          {{ t("automation.confirmTitle", { name: action.name }) }}
        </h2>
        <p class="mt-1 text-sm text-slate-500">{{ t("automation.confirmDescription") }}</p>
        <pre class="mt-3 max-h-40 overflow-auto rounded-lg bg-slate-50 p-3 font-mono text-xs text-slate-800">{{ action.command }}</pre>
        <p v-if="action.cwd" class="mt-2 text-xs text-slate-500">{{ t("automation.cwd") }}: {{ action.cwd }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-lg px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
            @click="emit('cancel')"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            type="button"
            class="rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-700"
            @click="emit('confirm')"
          >
            {{ t("automation.run") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
