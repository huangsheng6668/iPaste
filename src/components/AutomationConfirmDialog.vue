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
      class="dialog-backdrop"
      @click.self="emit('cancel')"
    >
      <div
        class="dialog-panel dialog-panel-wide"
        role="dialog"
        @keydown.esc="emit('cancel')"
      >
        <div class="dialog-body">
          <h2 class="dialog-title">
            {{ t("automation.confirmTitle", { name: action.name }) }}
          </h2>
          <p class="mt-1 text-sm text-slate-500">
            {{ t("automation.confirmDescription") }}
          </p>
          <pre class="mt-3 max-h-40 overflow-auto rounded-lg bg-slate-50 p-3 font-mono text-xs text-slate-800">{{ action.command }}</pre>
          <p
            v-if="action.cwd"
            class="mt-2 text-xs text-slate-500"
          >
            {{ t("automation.cwd") }}: {{ action.cwd }}
          </p>
        </div>

        <div class="dialog-footer">
          <button
            type="button"
            class="btn-ghost"
            @click="emit('cancel')"
          >
            {{ t("common.cancel") }}
          </button>
          <button
            type="button"
            class="btn-primary"
            @click="emit('confirm')"
          >
            {{ t("automation.run") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
