<script setup lang="ts">
import { t } from "../i18n";
import type { AutomationInput } from "../types";

defineProps<{ open: boolean; items: AutomationInput[] }>();
const emit = defineEmits<{ (e: "confirm"): void; (e: "cancel"): void }>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && items.length"
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
            {{ t("automation.importPreviewTitle", { count: items.length }) }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("automation.importPreviewHint") }}
          </p>
          <ul class="mt-3 max-h-64 space-y-2 overflow-auto">
            <li
              v-for="(item, index) in items"
              :key="index"
              class="rounded-[var(--r-md)] border border-[var(--border-hairline)] bg-[var(--surface-inset)] p-3"
            >
              <p class="text-xs font-semibold text-[var(--text-1)]">
                {{ item.name }}
              </p>
              <pre class="mt-1 overflow-auto font-mono text-xs text-[var(--text-1)]">{{ item.command }}</pre>
            </li>
          </ul>
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
            {{ t("automation.importPreviewConfirm") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
