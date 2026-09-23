<script setup lang="ts">
import AutomationDetailPane from "./AutomationDetailPane.vue";
import { t } from "../i18n";
import type { AutomationAction } from "../types";

defineProps<{
  open: boolean;
  action: AutomationAction | null;
}>();

const emit = defineEmits<{
  run: [action: AutomationAction];
  close: [];
}>();
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && action"
      class="dialog-backdrop"
      @click.self="emit('close')"
    >
      <div class="automation-detail-panel">
        <AutomationDetailPane
          :action="action"
          @run="emit('run', action)"
        />
        <div class="flex justify-end border-t border-slate-200 px-4 py-2">
          <button
            type="button"
            class="btn-ghost"
            @click="emit('close')"
          >
            {{ t("common.cancel") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
