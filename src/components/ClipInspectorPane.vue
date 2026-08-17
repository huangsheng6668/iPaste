<script setup lang="ts">
import { computed } from "vue";
import {
  Clipboard,
  ClipboardCopy,
  CornerDownLeft,
  ExternalLink,
  Eye,
  FileText,
  Image as ImageIcon,
  Maximize2,
  Palette,
  Type,
} from "lucide-vue-next";
import AutomationDetailPane from "./AutomationDetailPane.vue";
import { clipImageSrc } from "../lib/clipMedia";
import { t } from "../i18n";
import { clipMetricText, formatTime, typeLabel } from "../lib/format";
import type { AutomationAction, ClipViewItem } from "../types";

const props = defineProps<{
  item?: ClipViewItem | null;
  automationAction?: AutomationAction | null;
  mode?: "clip" | "actions";
}>();

const emit = defineEmits<{
  copy: [item: ClipViewItem];
  apply: [item: ClipViewItem];
  expand: [item: ClipViewItem];
  runAutomation: [action: AutomationAction];
}>();

const isImage = computed(() => props.item?.clipType === "image");
const isColor = computed(() => props.item?.clipType === "color");
const isLink = computed(() => props.item?.clipType === "link");
const imageSrc = computed(() => (props.item ? clipImageSrc(props.item) : ""));
const colorValue = computed(() => (props.item ? props.item.text.trim() : ""));
const lines = computed(() => props.item?.text.split(/\r?\n/).length ?? 0);
const charCount = computed(() => props.item?.text.length ?? 0);

const detailTitle = computed(() => {
  if (!props.item) return "";
  return props.item.displayName?.trim() || typeLabel(props.item.clipType);
});

const displayTime = computed(() => {
  if (!props.item) return "";
  return props.item.collection === "history" ? props.item.lastCapturedAt : props.item.createdAt;
});

const typeIcon = computed(() => {
  if (!props.item) return Clipboard;
  if (props.item.clipType === "image") return ImageIcon;
  if (props.item.clipType === "color") return Palette;
  if (props.item.clipType === "link") return ExternalLink;
  if (props.item.clipType === "file") return FileText;
  return Type;
});
</script>

<template>
  <aside class="clip-inspector">
    <!-- Automation Detail View -->
    <template v-if="mode === 'actions'">
      <AutomationDetailPane
        :action="automationAction ?? null"
        @run="automationAction && emit('runAutomation', automationAction)"
      />
    </template>

    <!-- Clip Item Inspector View -->
    <template v-else-if="item">
      <!-- Inspector Header -->
      <div class="clip-inspector-header">
        <div class="clip-inspector-header-left">
          <component
            :is="typeIcon"
            class="size-4 text-[var(--accent)]"
          />
          <span class="clip-inspector-title">{{ detailTitle }}</span>
        </div>

        <div class="flex items-center gap-1">
          <button
            type="button"
            class="icon-button"
            :aria-label="t('common.copy')"
            :data-tooltip="t('common.copy')"
            @click="emit('copy', item)"
          >
            <ClipboardCopy class="size-3.5" />
          </button>

          <button
            type="button"
            class="icon-button"
            :aria-label="t('clip.expand')"
            :data-tooltip="t('clip.expand')"
            @click="emit('expand', item)"
          >
            <Maximize2 class="size-3.5" />
          </button>

          <button
            type="button"
            class="icon-button"
            :aria-label="t('common.paste')"
            :data-tooltip="t('common.paste')"
            @click="emit('apply', item)"
          >
            <CornerDownLeft class="size-3.5 text-[var(--accent)]" />
          </button>
        </div>
      </div>

      <!-- Inspector Body -->
      <div class="clip-inspector-body">
        <!-- Image Preview Box -->
        <div
          v-if="isImage"
          class="clip-inspector-image-box group relative"
        >
          <img
            :src="imageSrc"
            :alt="t('common.imagePreviewAlt')"
          >
          <button
            type="button"
            class="absolute bottom-2 right-2 inline-flex items-center gap-1 rounded-md bg-black/70 px-2 py-1 text-[0.6875rem] font-medium text-white shadow backdrop-blur hover:bg-black/90"
            @click="emit('expand', item)"
          >
            <Eye class="size-3" />
            <span>{{ t("clip.expand") }}</span>
          </button>
        </div>

        <!-- Color Preview Box -->
        <div
          v-else-if="isColor"
          class="clip-inspector-color-box"
        >
          <div
            class="clip-inspector-color-preview"
            :style="{ backgroundColor: colorValue }"
          />
          <div class="flex flex-col gap-1 min-w-0">
            <span class="font-mono text-sm font-semibold text-[var(--text-1)]">{{ colorValue }}</span>
            <span class="text-xs text-[var(--text-3)]">{{ typeLabel("color") }}</span>
          </div>
        </div>

        <!-- Link Preview Box -->
        <div
          v-else-if="isLink"
          class="flex flex-col gap-2 rounded-md border border-[var(--border-hairline)] bg-[var(--surface-code)] p-3"
        >
          <div class="flex items-center gap-2 text-xs text-[var(--text-2)] font-mono truncate">
            <ExternalLink class="size-3.5 text-[var(--accent)] shrink-0" />
            <span class="truncate">{{ item.text }}</span>
          </div>
          <a
            :href="item.text"
            target="_blank"
            class="btn-secondary self-start text-xs inline-flex items-center gap-1 mt-1"
          >
            <ExternalLink class="size-3" />
            <span>{{ typeLabel("link") }}</span>
          </a>
        </div>

        <!-- Text / Code Preview Box -->
        <pre
          v-else
          class="clip-inspector-content-box"
        >{{ item.text }}</pre>

        <!-- Meta Info Card Footer -->
        <div class="clip-inspector-meta-card">
          <div class="clip-inspector-meta-item">
            <span class="clip-inspector-meta-label">{{ t("common.size") }}</span>
            <span class="clip-inspector-meta-val">{{ clipMetricText(item.clipType, item.text, item.previewText) }}</span>
          </div>

          <div
            v-if="!isImage"
            class="clip-inspector-meta-item"
          >
            <span class="clip-inspector-meta-label">{{ t("common.lines") }}</span>
            <span class="clip-inspector-meta-val">{{ charCount }} chars / {{ lines }} lines</span>
          </div>

          <div class="clip-inspector-meta-item col-span-2">
            <span class="clip-inspector-meta-label">{{ t("detail.title") }}</span>
            <span class="clip-inspector-meta-val">{{ formatTime(displayTime) }}</span>
          </div>
        </div>
      </div>
    </template>

    <!-- Empty Inspector Placeholder -->
    <div
      v-else
      class="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-[var(--text-3)]"
    >
      <Clipboard class="size-8 stroke-[1.5]" />
      <p class="text-xs">
        {{ t("detail.noSelection") }}
      </p>
    </div>
  </aside>
</template>
