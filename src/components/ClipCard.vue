<script setup lang="ts">
import { computed } from "vue";
import {
  CornerDownLeft,
  FileText,
  GripVertical,
  Image,
  Link,
  Maximize2,
  Palette,
  Type,
} from "lucide-vue-next";
import { clipImageSrc } from "../lib/clipMedia";
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
const imageSrc = computed(() => clipImageSrc(props.item));
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

const iconComponent = computed(() => {
  if (props.item.clipType === "link") return Link;
  if (props.item.clipType === "color") return Palette;
  if (props.item.clipType === "image") return Image;
  if (props.item.clipType === "file") return FileText;
  return Type;
});

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
    :class="{ 'clip-mini-card-selected': selected }"
    role="option"
    :aria-selected="selected"
    @click="emit('select', index)"
    @dblclick="emit('apply', item)"
    @contextmenu.prevent.stop="openContextMenu"
  >
    <!-- Selected Active Coral Indicator -->
    <div
      v-if="selected"
      class="clip-mini-card-indicator"
    />

    <!-- Delete Confirmation Floating Badge -->
    <div
      v-if="deleteConfirming"
      class="clip-delete-confirm-banner"
      role="alert"
    >
      {{ t("clip.confirmDeleteWithBackspace") }}
    </div>

    <!-- Line 1: Type Icon + Title / Snippet + Category Tag -->
    <div class="clip-mini-card-line1">
      <span
        v-if="reorderEnabled"
        class="cursor-grab text-[var(--text-3)] hover:text-[var(--text-1)]"
        @pointerdown.stop="startReorder"
      >
        <GripVertical class="size-3.5" />
      </span>

      <span class="clip-mini-card-icon">
        <span
          v-if="isColor"
          class="inline-block size-3 rounded-full border border-black/20 dark:border-white/20"
          :style="{ backgroundColor: colorPreviewValue }"
        />
        <img
          v-else-if="isImage"
          class="size-3.5 rounded object-cover"
          :src="imageSrc"
          alt=""
        >
        <component
          :is="iconComponent"
          v-else
          class="size-3.5"
        />
      </span>

      <!-- Inline Rename Input or Text Title -->
      <input
        v-if="editingName !== null"
        class="clip-title-input flex-1 min-w-0 bg-transparent text-xs outline-none border-b border-[var(--accent)]"
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
          class="size-1.5 rounded-full"
          :style="{ backgroundColor: categoryTagColor }"
        />
        {{ categoryTagLabel }}
      </span>
    </div>

    <!-- Line 2: Size/Chars + Time + Enter Hint -->
    <div class="clip-mini-card-line2">
      <div class="clip-mini-card-meta">
        <span>{{ metricText }}</span>
        <span>·</span>
        <span>{{ formatTime(displayTime) }}</span>
      </div>

      <div class="flex items-center gap-1">
        <button
          type="button"
          class="hidden group-hover:inline-flex p-0.5 rounded text-[var(--text-3)] hover:text-[var(--text-1)]"
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
          {{ t("common.paste") }}
        </span>
      </div>
    </div>
  </article>
</template>
