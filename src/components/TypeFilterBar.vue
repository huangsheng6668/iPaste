<script setup lang="ts">
import { computed } from "vue";
import { Image, Layers, Type } from "lucide-vue-next";
import { t } from "../i18n";
import { isMacOs } from "../lib/env";
import type { ClipTypeFilter } from "../types";

/**
 * 历史类型筛选分段控件（全部/文本/图片）。
 * 动机：图片无法被文本搜索命中，且复制频次低、散落在文本历史里；
 * 一键切到图片段即可让稀疏图片聚合成一屏。每段计数徽章在切换前就
 * 告知该段有多少条（计数跟随搜索上下文，由后端随分页返回）。
 */
const props = defineProps<{
  filter: ClipTypeFilter;
  totalCount: number;
  textCount: number;
  imageCount: number;
}>();

const emit = defineEmits<{
  select: [filter: ClipTypeFilter];
}>();

const cycleShortcut = computed(() => (isMacOs ? "⌘G" : "Ctrl+G"));

const segments = computed(() => [
  { id: "all" as const, label: t("typeFilter.all"), icon: Layers, count: props.totalCount },
  { id: "text" as const, label: t("typeFilter.text"), icon: Type, count: props.textCount },
  { id: "image" as const, label: t("typeFilter.image"), icon: Image, count: props.imageCount },
]);

function countLabel(count: number) {
  return count > 99 ? "99+" : String(Math.max(count, 0));
}
</script>

<template>
  <div class="type-filter-bar">
    <div
      class="type-filter-segment"
      role="group"
      :aria-label="t('typeFilter.ariaLabel')"
    >
      <button
        v-for="segment in segments"
        :key="segment.id"
        type="button"
        class="type-filter-option"
        :class="{ 'type-filter-option-active': filter === segment.id }"
        :aria-pressed="filter === segment.id"
        :data-tooltip="`${segment.label} · ${cycleShortcut}`"
        tabindex="-1"
        @click="emit('select', segment.id)"
      >
        <component
          :is="segment.icon"
          class="size-3.5"
        />
        <span>{{ segment.label }}</span>
        <span class="type-filter-count">{{ countLabel(segment.count) }}</span>
      </button>
    </div>

    <!-- 键盘可发现性：循环切换提示保持安静（text-3 + kbd），不抢搜索框的焦点语义 -->
    <kbd class="keyboard-kbd type-filter-hint pointer-events-none select-none">{{ cycleShortcut }}</kbd>
  </div>
</template>
