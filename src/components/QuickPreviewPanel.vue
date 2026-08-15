<script setup lang="ts">
import { ClipboardCopy, CornerDownLeft, X } from "lucide-vue-next";
import { t } from "../i18n";
import { formatTime, typeLabel } from "../lib/format";
import type { ClipViewItem } from "../types";

defineProps<{
  item: ClipViewItem;
  title: string;
  label: string;
  time: string;
  size: string;
  content: string;
  imageSrc: string;
  colorValue: string;
  locked: boolean;
  selectedText: string;
}>();

const emit = defineEmits<{
  lock: [];
  copy: [];
  paste: [];
  close: [];
}>();
</script>

<template>
  <div
    class="quick-preview-overlay"
    :class="{ 'quick-preview-overlay-locked': locked }"
    role="dialog"
    :aria-label="label"
    @pointerdown.stop="emit('lock')"
    @click.stop="emit('lock')"
    @contextmenu.stop
  >
    <div class="quick-preview-meta">
      <span class="quick-preview-type">{{ typeLabel(item.clipType) }}</span>
      <span
        v-if="title"
        class="quick-preview-title"
      >{{ title }}</span>
      <span class="quick-preview-spacer" />
      <span>{{ formatTime(time) }}</span>
      <span v-if="size">{{ size }}</span>
      <button
        type="button"
        class="quick-preview-action-button"
        :disabled="!selectedText.trim()"
        tabindex="-1"
        :aria-label="t('common.paste')"
        :data-tooltip="t('common.paste')"
        @pointerdown.stop
        @click.stop="emit('paste')"
      >
        <CornerDownLeft class="size-3.5" />
        <span>{{ t("common.paste") }}</span>
      </button>
      <button
        type="button"
        class="quick-preview-icon-button"
        tabindex="-1"
        :aria-label="t('common.copy')"
        :data-tooltip="t('common.copy')"
        @pointerdown.stop
        @click.stop="emit('copy')"
      >
        <ClipboardCopy class="size-3.5" />
      </button>
      <button
        type="button"
        class="quick-preview-icon-button"
        tabindex="-1"
        :aria-label="t('common.close')"
        :data-tooltip="t('common.close')"
        @pointerdown.stop
        @click.stop="emit('close')"
      >
        <X class="size-3.5" />
      </button>
    </div>

    <div
      v-if="item.clipType === 'image'"
      class="quick-preview-image"
    >
      <img
        :src="imageSrc"
        :alt="t('common.imagePreviewAlt')"
      >
    </div>

    <div
      v-else-if="item.clipType === 'color'"
      class="quick-preview-color"
    >
      <span
        class="quick-preview-color-swatch"
        :style="{ backgroundColor: colorValue }"
      />
      <code>{{ content }}</code>
    </div>

    <div
      v-else
      class="quick-preview-text"
    >
      {{ content }}
    </div>
  </div>
</template>
