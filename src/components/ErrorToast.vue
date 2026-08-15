<script setup lang="ts">
import { AlertCircle } from "lucide-vue-next";
import { useUiStore } from "../stores/uiStore";

const ui = useUiStore();
</script>

<template>
  <div
    v-if="ui.toasts.length"
    class="pointer-events-none fixed top-3 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-1.5"
    aria-live="polite"
  >
    <button
      v-for="toast in ui.toasts"
      :key="toast.id"
      type="button"
      class="pointer-events-auto flex max-w-[min(24rem,90vw)] cursor-pointer items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-left text-sm text-red-700 shadow-sm transition-all duration-150 hover:bg-red-100"
      :aria-label="toast.message"
      @click="ui.dismissToast(toast.id)"
    >
      <AlertCircle class="size-4 shrink-0" />
      <span class="min-w-0 flex-1 truncate">{{ toast.message }}</span>
    </button>
  </div>
</template>
