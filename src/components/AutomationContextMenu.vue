<script setup lang="ts">
import { ClipboardCopy, Download, Info, Pencil, Play, Trash2, Upload } from "lucide-vue-next";
import { t } from "../i18n";
import type { AutomationAction } from "../types";

defineProps<{
  automation: AutomationAction;
  x: number;
  y: number;
}>();

const emit = defineEmits<{
  run: [];
  edit: [];
  copy: [];
  detail: [];
  delete: [];
  import: [];
  export: [];
  close: [];
}>();
</script>

<template>
  <div
    class="clip-context-menu"
    :style="{ left: `${x}px`, top: `${y}px` }"
    @click.stop
    @contextmenu.prevent.stop
  >
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('run'); emit('close')"
    >
      <Play class="size-3.5" /> {{ t("automation.run") }}
    </button>
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('edit'); emit('close')"
    >
      <Pencil class="size-3.5" /> {{ t("automation.edit") }}
    </button>
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('copy'); emit('close')"
    >
      <ClipboardCopy class="size-3.5" /> {{ t("automation.copy") }}
    </button>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('detail'); emit('close')"
    >
      <Info class="size-3.5" /> {{ t("automation.detailStatus") }}
    </button>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="context-menu-item context-menu-item-strong"
      tabindex="-1"
      role="menuitem"
      @click="emit('delete'); emit('close')"
    >
      <Trash2 class="size-3.5" /> {{ t("automation.delete") }}
    </button>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('import'); emit('close')"
    >
      <Upload class="size-3.5" /> {{ t("automation.importAction") }}
    </button>
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('export'); emit('close')"
    >
      <Download class="size-3.5" /> {{ t("automation.exportAll") }}
    </button>
  </div>
</template>
