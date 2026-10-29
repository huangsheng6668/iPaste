<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  CornerDownLeft,
  GripVertical,
  Maximize2,
} from "lucide-vue-next";
import { categoryColorVars } from "../lib/categoryColors";
import { clipImageSrc, clipThumbnailSrc } from "../lib/clipMedia";
import { clipTypeIcon } from "../lib/clipTypeIcons";
import { t } from "../i18n";
import { categoryDisplayName, clipMetricText, formatTime, typeLabel } from "../lib/format";
import type { Category, ClipViewItem } from "../types";

const props = defineProps<{
  item: ClipViewItem;
  index: number;
  selected: boolean;
  categoryTags: Category[];
  editingName: string | null;
  reorderEnabled: boolean;
  deleteConfirming?: boolean;
}>();

const emit = defineEmits<{
  select: [index: number];
  apply: [item: ClipViewItem];
  expand: [item: ClipViewItem];
  openContextMenu: [payload: { item: ClipViewItem; index: number; x: number; y: number }];
  updateEditingName: [value: string];
  commitRename: [item: ClipViewItem];
  cancelRename: [];
  reorderPointerDown: [payload: { item: ClipViewItem; index: number; event: PointerEvent }];
}>();

const isImage = computed(() => props.item.clipType === "image");
const isColor = computed(() => props.item.clipType === "color");
// 列表小图标用缩略图（14px 图标解码整张 4K 原图纯属浪费）；缩略图缺失时
// （存量未补齐/生成失败）@error 一次性回退原图。条目切换后重置重试。
const thumbnailFailed = ref(false);
watch(
  () => props.item,
  () => {
    thumbnailFailed.value = false;
  },
);
const imageSrc = computed(() =>
  thumbnailFailed.value ? clipImageSrc(props.item) : clipThumbnailSrc(props.item),
);
function handleImageError() {
  thumbnailFailed.value = true;
}
const colorPreviewValue = computed(() => props.item.text.trim());
const displayTitle = computed(() => props.item.displayName?.trim() || "");

const snippetTitle = computed(() => {
  if (displayTitle.value) return displayTitle.value;
  if (isImage.value) return typeLabel("image");
  if (isColor.value) return colorPreviewValue.value;
  const raw = props.item.text || props.item.previewText || "";
  const firstLine = raw.split("\n")[0].trim();
  return firstLine || typeLabel(props.item.clipType);
});

const categoryTagLabel = computed(() => {
  if (props.item.collection !== "history") return "";
  if (!props.categoryTags.length) return "";
  const [firstCategory] = props.categoryTags;
  const label = categoryDisplayName(firstCategory.name);
  const extraCount = props.categoryTags.length - 1;
  return extraCount > 0 ? `${label} +${extraCount}` : label;
});

const categoryTagColor = computed(() => {
  if (props.item.collection !== "history") return undefined;
  if (!props.categoryTags.length) return undefined;
  return props.categoryTags[0].color;
});

const displayTime = computed(() =>
  props.item.collection === "history" ? props.item.lastCapturedAt : props.item.createdAt,
);

const metricText = computed(() => clipMetricText(props.item.clipType, props.item.text, props.item.previewText));

const iconComponent = computed(() => clipTypeIcon(props.item.clipType, props.item.text || ""));

function openContextMenu(event: MouseEvent) {
  emit("openContextMenu", {
    item: props.item,
    index: props.index,
    x: event.clientX,
    y: event.clientY,
  });
}

function startReorder(event: PointerEvent) {
  if (!props.reorderEnabled) {
    event.preventDefault();
    return;
  }
  emit("reorderPointerDown", {
    item: props.item,
    index: props.index,
    event,
  });
}
</script>

<template>
  <article
    class="clip-mini-card"
    :class="{
      'clip-mini-card-selected': selected,
      'clip-mini-card-delete-confirming': deleteConfirming,
      'clip-mini-card-reorder': reorderEnabled,
    }"
    role="option"
    :aria-selected="selected"
    @click="emit('select', index)"
    @dblclick="emit('apply', item)"
    @contextmenu.prevent.stop="openContextMenu"
  >
    <div
      v-if="selected"
      class="clip-mini-card-indicator"
    />

    <div
      v-if="deleteConfirming"
      class="clip-delete-confirm-banner"
      role="alert"
    >
      {{ t("clip.confirmDeleteWithBackspace") }}
    </div>

    <span
      v-if="reorderEnabled"
      class="clip-mini-card-grip"
      @pointerdown.stop="startReorder"
    >
      <GripVertical class="size-3.5" />
    </span>

    <span class="clip-mini-card-icon">
      <span
        v-if="isColor"
        class="inline-block size-3 rounded-full border border-[var(--border-strong)]"
        :style="{ backgroundColor: colorPreviewValue }"
      />
      <img
        v-else-if="isImage"
        class="size-3.5 rounded object-cover"
        :src="imageSrc"
        alt=""
        loading="lazy"
        decoding="async"
        @error="handleImageError"
      >
      <component
        :is="iconComponent"
        v-else
        class="size-3"
      />
    </span>

    <div class="clip-mini-card-body">
      <div class="clip-mini-card-line1">
        <input
          v-if="editingName !== null"
          class="clip-title-input flex-1 min-w-0"
          :value="editingName"
          tabindex="-1"
          spellcheck="false"
          @click.stop
          @dblclick.stop
          @input="emit('updateEditingName', ($event.target as HTMLInputElement).value)"
          @keydown.enter.prevent.stop="emit('commitRename', item)"
          @keydown.escape.prevent.stop="emit('cancelRename')"
          @blur="emit('commitRename', item)"
        >
        <span
          v-else
          class="clip-mini-card-title"
        >{{ snippetTitle }}</span>

        <span
          v-if="categoryTagLabel"
          class="clip-mini-card-tag"
        >
          <span
            v-if="categoryTagColor"
            class="clip-mini-card-tag-dot"
            :style="categoryColorVars(categoryTagColor)"
          />
          <span class="clip-mini-card-tag-label">{{ categoryTagLabel }}</span>
        </span>
      </div>

      <div class="clip-mini-card-line2">
        <div class="clip-mini-card-meta">
          <span>{{ metricText }}</span>
          <i aria-hidden="true">·</i>
          <span>{{ formatTime(displayTime) }}</span>
        </div>

        <div class="clip-mini-card-actions">
          <button
            type="button"
            class="clip-mini-card-expand"
            :aria-label="t('clip.expand')"
            @click.stop="emit('expand', item)"
          >
            <Maximize2 class="size-3" />
          </button>
          <span
            v-if="selected"
            class="clip-mini-card-action-hint"
          >
            <CornerDownLeft class="size-2.5" />
            <span class="clip-mini-card-action-label">{{ t("common.paste") }}</span>
          </span>
        </div>
      </div>
    </div>
  </article>
</template>
