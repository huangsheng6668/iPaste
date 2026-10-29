<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  Check,
  Clipboard,
  ClipboardCopy,
  CornerDownLeft,
  ExternalLink,
  Eye,
  Globe,
  Maximize2,
  ScanText,
} from "lucide-vue-next";
import AutomationDetailPane from "./AutomationDetailPane.vue";
import { clipImageSrc, clipThumbnailSrc } from "../lib/clipMedia";
import { clipTypeIcon } from "../lib/clipTypeIcons";
import { parseColorFormats } from "../lib/colorFormat";
import { t } from "../i18n";
import { clipMetricText, formatTime, isCodeText, lineCountText, textStats, typeLabel } from "../lib/format";
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
  ocr: [item: ClipViewItem];
  runAutomation: [action: AutomationAction];
}>();

const copiedFormat = ref<string | null>(null);

const isImage = computed(() => props.item?.clipType === "image");
const isColor = computed(() => props.item?.clipType === "color");
const isLink = computed(() => props.item?.clipType === "link");
const isCode = computed(() => isCodeText(props.item?.clipType ?? "", props.item?.text || ""));

// 检查器预览用缩略图（面板内渲染尺寸 ≤480px 已足够），缺失时一次性回退
// 原图；条目切换后重置重试。放大预览/查看器窗口仍走原图（需要完整分辨率）。
const thumbnailFailed = ref(false);
watch(
  () => props.item,
  () => {
    thumbnailFailed.value = false;
  },
);
const imageSrc = computed(() => {
  if (!props.item) return "";
  return thumbnailFailed.value ? clipImageSrc(props.item) : clipThumbnailSrc(props.item);
});
function handleImageError() {
  thumbnailFailed.value = true;
}
const colorValue = computed(() => (props.item ? props.item.text.trim() : ""));
const lines = computed(() => props.item?.text.split(/\r?\n/).length ?? 0);

const colorFormats = computed(() => {
  if (!isColor.value || !colorValue.value) return [];
  return parseColorFormats(colorValue.value) ?? [];
});

async function copyFormatValue(val: string, label: string) {
  try {
    await navigator.clipboard.writeText(val);
    copiedFormat.value = label;
    setTimeout(() => {
      if (copiedFormat.value === label) {
        copiedFormat.value = null;
      }
    }, 1500);
  } catch (_err) {
    void _err;
  }
}

const detailTitle = computed(() => {
  if (!props.item) return "";
  return props.item.displayName?.trim() || typeLabel(props.item.clipType);
});

const displayTime = computed(() => {
  if (!props.item) return "";
  return props.item.collection === "history" ? props.item.lastCapturedAt : props.item.createdAt;
});

const typeIcon = computed(() => (props.item ? clipTypeIcon(props.item.clipType, props.item.text) : Clipboard));

const linkHostname = computed(() => {
  if (!isLink.value || !props.item?.text) return "";
  try {
    const url = new URL(props.item.text.trim());
    return url.hostname;
  } catch {
    return props.item.text.trim();
  }
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
          <span class="clip-inspector-type-chip">
            <component
              :is="typeIcon"
              class="size-3.5"
            />
          </span>
          <span class="clip-inspector-title">{{ detailTitle }}</span>
        </div>

        <div class="flex items-center gap-0.5">
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
            @error="handleImageError"
          >
          <div class="clip-inspector-image-actions">
            <button
              type="button"
              class="clip-inspector-image-action"
              :aria-label="t('clip.recognizeText')"
              :data-tooltip="t('clip.recognizeText')"
              @click="emit('ocr', item)"
            >
              <ScanText class="size-3.5" />
            </button>
            <button
              type="button"
              class="clip-inspector-image-action"
              :aria-label="t('clip.expand')"
              :data-tooltip="t('clip.expand')"
              @click="emit('expand', item)"
            >
              <Eye class="size-3.5" />
            </button>
          </div>
        </div>

        <!-- Color Preview Box with Format Converter Matrix -->
        <div
          v-else-if="isColor"
          class="clip-inspector-color-card"
        >
          <div class="clip-inspector-color-header">
            <div
              class="clip-inspector-color-preview"
              :style="{ backgroundColor: colorValue }"
            />
            <div class="flex flex-col gap-1 min-w-0 flex-1">
              <span class="font-mono text-[0.8125rem] font-semibold tracking-[0.01em] text-[var(--text-1)]">{{ colorValue }}</span>
              <span class="text-[0.625rem] font-medium text-[var(--text-3)]">{{ typeLabel("color") }}</span>
            </div>
          </div>

          <div class="color-matrix-grid">
            <button
              v-for="fmt in colorFormats"
              :key="fmt.label"
              type="button"
              class="color-matrix-item"
              :title="`Click to copy ${fmt.label}`"
              @click="copyFormatValue(fmt.value, fmt.label)"
            >
              <span class="color-matrix-item-label">{{ fmt.label }}</span>
              <span
                v-if="copiedFormat === fmt.label"
                class="inline-flex items-center gap-1 text-[var(--success)] font-semibold"
              >
                <Check class="size-3" />
                <span>{{ t("common.copy") }}</span>
              </span>
              <span
                v-else
                class="color-matrix-item-val truncate"
              >{{ fmt.value }}</span>
            </button>
          </div>
        </div>

        <!-- Link Preview Box -->
        <div
          v-else-if="isLink"
          class="clip-inspector-link"
        >
          <div class="clip-inspector-link-host">
            <Globe class="size-3.5" />
            <span class="truncate">{{ linkHostname }}</span>
          </div>
          <div class="clip-inspector-link-url">
            {{ item.text }}
          </div>
          <a
            :href="item.text"
            target="_blank"
            rel="noopener noreferrer"
            class="btn-ghost clip-inspector-link-button"
          >
            <ExternalLink class="size-3" />
            <span>{{ typeLabel("link") }}</span>
          </a>
        </div>

        <!-- Text / Code Preview Box with Gutter Header -->
        <div
          v-else
          class="clip-inspector-content-box"
        >
          <div class="clip-inspector-code-header tabular-nums">
            <span>{{ isCode ? t("detail.codeSnippet") : t("detail.plainText") }}</span>
            <span>{{ item ? textStats(item.text) : "" }} · {{ lineCountText(lines) }}</span>
          </div>
          <pre class="clip-inspector-code-content">{{ item.text }}</pre>
        </div>

        <!-- Meta Info Ledger Footer -->
        <div
          class="clip-inspector-meta-card tabular-nums"
          :class="{ 'clip-inspector-meta-card-pair': isImage }"
        >
          <div class="clip-inspector-meta-item">
            <span class="clip-inspector-meta-label">{{ t("common.size") }}</span>
            <span class="clip-inspector-meta-val">{{ clipMetricText(item.clipType, item.text, item.previewText) }}</span>
          </div>

          <div
            v-if="!isImage"
            class="clip-inspector-meta-item"
          >
            <span class="clip-inspector-meta-label">{{ t("common.lines") }}</span>
            <span class="clip-inspector-meta-val">{{ lineCountText(lines) }}</span>
          </div>

          <div class="clip-inspector-meta-item">
            <span class="clip-inspector-meta-label">{{ t("detail.copiedAt") }}</span>
            <span class="clip-inspector-meta-val">{{ formatTime(displayTime) }}</span>
          </div>
        </div>
      </div>
    </template>

    <!-- Empty Inspector Placeholder -->
    <div
      v-else
      class="clip-inspector-empty"
    >
      <Clipboard class="size-8 stroke-[1.5]" />
      <p>{{ t("detail.noSelection") }}</p>
    </div>
  </aside>
</template>
