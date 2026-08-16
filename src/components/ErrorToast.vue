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
      class="error-toast"
      :aria-label="toast.message"
      @click="ui.dismissToast(toast.id)"
    >
      <span class="error-toast-icon">
        <AlertCircle class="size-3.5" />
      </span>
      <span class="min-w-0 flex-1 truncate">{{ toast.message }}</span>
    </button>
  </div>
</template>
