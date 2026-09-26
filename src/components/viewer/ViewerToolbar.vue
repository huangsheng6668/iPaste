<script setup lang="ts">
import {
  LoaderCircle,
  Maximize2,
  Pin,
  PinOff,
  RotateCw,
  RotateCcw,
  Save,
  ScanText,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-vue-next";
import ViewerMeta from "./ViewerMeta.vue";
import { t } from "../../i18n";
import type { ClipViewItem } from "../../types";

// 查看器顶栏：置顶、标题区、图片工具箱、文本编辑动作与关闭。
// 纯展示组件：状态经 props 传入，交互经 emit 交回宿主，无内部业务状态。
defineProps<{
  isImage: boolean;
  isPinned: boolean;
  title: string;
  item: ClipViewItem | undefined;
  displayTime: string;
  canZoomOutImage: boolean;
  canZoomInImage: boolean;
  isImageActualSize: boolean;
  imageZoomLabel: string;
  hasChanged: boolean;
  isRecognizingImage: boolean;
  isOcrResultActive: boolean;
}>();

const emit = defineEmits<{
  togglePin: [];
  /** 标题区按下：把原生 mousedown 透传给宿主（拖动实现依赖 event.button）。 */
  drag: [event: MouseEvent];
  zoomOut: [];
  zoomIn: [];
  actualSize: [];
  rotate: [];
  recognizeText: [];
  resetDraft: [];
  applyChanges: [];
  close: [];
}>();
</script>

<template>
  <header
    class="clip-viewer-toolbar"
    :class="{ 'clip-viewer-toolbar-image': isImage }"
  >
    <button
      type="button"
      class="viewer-icon-button"
      :class="{ 'viewer-icon-button-active': isPinned }"
      :aria-label="isPinned ? t('viewer.unpin') : t('viewer.pin')"
      :data-tooltip="isPinned ? t('viewer.unpin') : t('viewer.pin')"
      @click="emit('togglePin')"
    >
      <PinOff
        v-if="isPinned"
        class="size-4"
      />
      <Pin
        v-else
        class="size-4"
      />
    </button>

    <div
      class="clip-viewer-drag-zone min-w-0 flex-1"
      @mousedown="emit('drag', $event)"
    >
      <ViewerMeta
        :title="title"
        :item="item"
        :display-time="displayTime"
      />
    </div>

    <div
      v-if="isImage"
      class="viewer-image-toolbox"
      role="toolbar"
      :aria-label="t('viewer.imageToolbar')"
      @pointerdown.stop
      @wheel.stop
    >
      <button
        type="button"
        class="viewer-icon-button"
        :disabled="!canZoomOutImage"
        :aria-label="t('viewer.zoomOut')"
        :data-tooltip="t('viewer.zoomOut')"
        @click="emit('zoomOut')"
      >
        <ZoomOut class="size-4" />
      </button>
      <button
        type="button"
        class="viewer-icon-button"
        :disabled="!canZoomInImage"
        :aria-label="t('viewer.zoomIn')"
        :data-tooltip="t('viewer.zoomIn')"
        @click="emit('zoomIn')"
      >
        <ZoomIn class="size-4" />
      </button>
      <button
        type="button"
        class="viewer-icon-button"
        :class="{ 'viewer-icon-button-active': isImageActualSize }"
        :aria-label="t('viewer.actualSize')"
        :data-tooltip="t('viewer.actualSize')"
        @click="emit('actualSize')"
      >
        <Maximize2 class="size-4" />
      </button>
      <button
        type="button"
        class="viewer-icon-button"
        :aria-label="t('viewer.rotateClockwise')"
        :data-tooltip="t('viewer.rotateClockwise')"
        @click="emit('rotate')"
      >
        <RotateCw class="size-4" />
      </button>
      <button
        type="button"
        class="viewer-image-zoom-label"
        :aria-label="t('viewer.restore100')"
        :data-tooltip="t('viewer.restore100')"
        @click="emit('actualSize')"
      >
        {{ imageZoomLabel }}
      </button>
      <button
        type="button"
        class="viewer-icon-button"
        :class="{ 'viewer-icon-button-active': isOcrResultActive }"
        :disabled="isRecognizingImage"
        :aria-label="t('viewer.recognizeText')"
        :data-tooltip="t('viewer.recognizeText')"
        @click="emit('recognizeText')"
      >
        <LoaderCircle
          v-if="isRecognizingImage"
          class="size-4 update-spin"
        />
        <ScanText
          v-else
          class="size-4"
        />
      </button>
    </div>

    <button
      v-if="!isImage"
      type="button"
      class="viewer-action-button"
      :disabled="!hasChanged"
      @click="emit('resetDraft')"
    >
      <RotateCcw class="size-4" />
      <span>{{ t("viewer.reset") }}</span>
    </button>

    <button
      v-if="!isImage"
      type="button"
      class="viewer-action-button viewer-action-button-primary"
      :disabled="!hasChanged"
      @click="emit('applyChanges')"
    >
      <Save class="size-4" />
      <span>{{ t("viewer.applyChanges") }}</span>
    </button>

    <button
      type="button"
      class="viewer-icon-button"
      :aria-label="t('viewer.closeWindow')"
      :data-tooltip="t('viewer.closeWindow')"
      @click="emit('close')"
    >
      <X class="size-4" />
    </button>
  </header>
</template>
