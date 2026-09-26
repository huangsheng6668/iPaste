<script setup lang="ts">
import { t } from "../../i18n";
import type { OcrLine, OcrSelectableWord } from "../../lib/ocr/textLayout";
import { ref } from "vue";

// 查看器图片舞台：图片框、OCR 文本层（行/高亮/可选词）、识别扫描蒙层。
// OCR 面板经默认插槽注入（宿主保留其全部 props，避免透传十几个 prop）。
// stageElement 以 prop 形式接收宿主的 ref 并直接绑定到根节点：命中测试与
// 选区定位都依赖真实 DOM 元素，ref 必须在宿主侧仍指向该元素。
defineProps<{
  src: string;
  imageStyle: Record<string, string>;
  imageFrameStyle: Record<string, string>;
  ocrTextLayerStyle: Record<string, string>;
  canPan: boolean;
  isDragging: boolean;
  isRecognizing: boolean;
  lines: OcrLine[];
  words: OcrSelectableWord[];
  selectedWordIndexes: Set<number>;
  highlights: ReadonlyArray<{ key: string; left: number; top: number; width: number; height: number }>;
}>();

const emit = defineEmits<{
  wheel: [event: WheelEvent];
  pointerDown: [event: PointerEvent];
  pointerMove: [event: PointerEvent];
  pointerUp: [event: PointerEvent];
  pointerCancel: [event: PointerEvent];
  lostPointerCapture: [event: PointerEvent];
  imageLoad: [event: Event];
  wordPointerDown: [payload: { event: PointerEvent; selectionIndex: number }];
  wordPointerMove: [event: PointerEvent];
  wordPointerUp: [event: PointerEvent];
  wordPointerCancel: [event: PointerEvent];
  wordPointerLostCapture: [event: PointerEvent];
}>();

// 舞台元素以模板 ref 持有并 defineExpose 给宿主：命中测试与选区定位都依赖
// 真实 DOM 元素，宿主把它接到 useImageViewer 的 imageStageElement 上。
const stageElement = ref<HTMLElement | null>(null);
defineExpose({ stageElement });
</script>

<template>
  <div
    ref="stageElement"
    class="viewer-image-stage"
    :class="{
      'viewer-image-stage-pannable': canPan,
      'viewer-image-stage-dragging': isDragging,
      'viewer-image-stage-recognizing': isRecognizing,
    }"
    @wheel="emit('wheel', $event)"
    @pointerdown="emit('pointerDown', $event)"
    @pointermove="emit('pointerMove', $event)"
    @pointerup="emit('pointerUp', $event)"
    @pointercancel="emit('pointerCancel', $event)"
    @lostpointercapture="emit('lostPointerCapture', $event)"
  >
    <div
      class="viewer-image-frame"
      :style="imageFrameStyle"
    >
      <img
        :src="src"
        :style="imageStyle"
        draggable="false"
        :alt="t('common.imagePreviewAlt')"
        @load="emit('imageLoad', $event)"
      >
      <div
        v-if="lines.length"
        class="viewer-image-ocr-layer"
        :style="ocrTextLayerStyle"
      >
        <span
          v-for="highlight in highlights"
          :key="highlight.key"
          class="viewer-image-ocr-highlight"
          :style="{
            left: `${highlight.left}px`,
            top: `${highlight.top}px`,
            width: `${highlight.width}px`,
            height: `${highlight.height}px`,
          }"
        />
        <span
          v-for="line in lines"
          :key="line.key"
          class="viewer-image-ocr-line"
          :style="{
            left: `${line.left}px`,
            top: `${line.top}px`,
            width: `${line.width}px`,
            height: `${line.height}px`,
            fontSize: `${Math.max(10, line.height * 0.84)}px`,
          }"
          aria-hidden="true"
        >
          {{ line.text }}
        </span>
        <button
          v-for="word in words"
          :key="`${word.lineKey}:${word.selectionIndex}`"
          type="button"
          class="viewer-image-ocr-word"
          :class="{ 'viewer-image-ocr-word-selected': selectedWordIndexes.has(word.selectionIndex) }"
          :data-ocr-word-index="word.selectionIndex"
          :aria-label="word.text"
          :style="{
            left: `${word.left}px`,
            top: `${word.top}px`,
            width: `${word.width}px`,
            height: `${word.height}px`,
          }"
          @pointerdown="emit('wordPointerDown', { event: $event, selectionIndex: word.selectionIndex })"
          @pointermove="emit('wordPointerMove', $event)"
          @pointerup="emit('wordPointerUp', $event)"
          @pointercancel="emit('wordPointerCancel', $event)"
          @lostpointercapture="emit('wordPointerLostCapture', $event)"
        />
      </div>
    </div>

    <div
      v-if="isRecognizing"
      class="viewer-image-scan-mask"
      aria-hidden="true"
    >
      <span />
    </div>

    <slot />
  </div>
</template>
