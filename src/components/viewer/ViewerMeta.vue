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
  <h1 class="truncate text-base font-semibold text-slate-950">
    {{ title }}
  </h1>
  <p
    v-if="item"
    class="truncate text-xs text-slate-500"
  >
    {{ typeLabel(item.clipType) }} · {{ formatTime(displayTime) }}
  </p>
</template>
