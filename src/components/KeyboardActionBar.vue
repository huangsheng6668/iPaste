<script setup lang="ts">
import { computed } from "vue";
import { t } from "../i18n";

const props = defineProps<{
  mode?: "history" | "category" | "automation";
  isMac?: boolean;
}>();

const metaKey = computed(() => (props.isMac ? "⌘" : "Ctrl+"));
</script>

<template>
  <footer class="keyboard-action-bar">
    <!-- Left Action Hints Group -->
    <div
      v-if="mode === 'automation'"
      class="keyboard-action-list"
    >
      <div class="keyboard-action-item font-medium text-[var(--text-1)]">
        <kbd class="keyboard-kbd text-[var(--accent)] font-bold">↵</kbd>
        <span>{{ t("automation.run") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">E</kbd>
        <span>{{ t("automation.edit") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">{{ metaKey }}C</kbd>
        <span>{{ t("common.copy") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">⌫</kbd>
        <span>{{ t("common.delete") }}</span>
      </div>
    </div>

    <div
      v-else
      class="keyboard-action-list"
    >
      <div class="keyboard-action-item font-medium text-[var(--text-1)]">
        <kbd class="keyboard-kbd text-[var(--accent)] font-bold">↵</kbd>
        <span>{{ t("common.paste") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">{{ metaKey }}C</kbd>
        <span>{{ t("common.copy") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">Space</kbd>
        <span>{{ t("clip.expand") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">{{ metaKey }}K</kbd>
        <span>{{ t("automation.entry") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">⌫</kbd>
        <span>{{ t("common.delete") }}</span>
      </div>
    </div>

    <!-- Right Navigation Hints Group -->
    <div class="keyboard-action-list">
      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">⇥</kbd>
        <span>{{ t("category.history") }}</span>
      </div>

      <div class="keyboard-action-item">
        <kbd class="keyboard-kbd">Esc</kbd>
        <span>{{ t("topBar.closePanel") }}</span>
      </div>
    </div>
  </footer>
</template>
