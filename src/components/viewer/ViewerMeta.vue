<script setup lang="ts">
import { formatTime, typeLabel } from "../../lib/format";
import type { ClipViewItem } from "../../types";

// 查看器标题区（标题 + 类型/时间副标题）：纯展示，无内部状态；
// 拖动窗口由父级的 .clip-viewer-drag-zone 容器负责。
defineProps<{
  title: string;
  item: ClipViewItem | undefined;
  displayTime: string;
}>();
</script>

<template>
  <h1 class="truncate text-base font-semibold text-[var(--text-1)]">
    {{ title }}
  </h1>
  <p
    v-if="item"
    class="truncate text-xs text-[var(--text-2)]"
  >
    {{ typeLabel(item.clipType) }} · {{ formatTime(displayTime) }}
  </p>
</template>
