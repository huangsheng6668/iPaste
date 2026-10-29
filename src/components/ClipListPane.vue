<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type CSSProperties, type ComponentPublicInstance } from "vue";
import { Image, Inbox, Type, Zap } from "lucide-vue-next";
import ClipCard from "./ClipCard.vue";
import AutomationCard from "./AutomationCard.vue";
import { t } from "../i18n";
import { categoryColorVars } from "../lib/categoryColors";
import { contextItemKey } from "../lib/clipKeys";
import { categoryDisplayName } from "../lib/format";
import type { AutomationAction, Category, CategoryItem, ClipTypeFilter, ClipViewItem } from "../types";

const props = defineProps<{
  // 根节点 .raycast-left-pane 是滚动容器；父组件经此拿到 DOM 以做触底加载与选中滚动
  listRef: (el: HTMLElement | null) => void;
  items: ClipViewItem[];
  selectedIndex: number;
  selectedCategoryId: string;
  typeFilter?: ClipTypeFilter;
  hasSearchQuery?: boolean;
  isLoadingMore: boolean;
  canReorder: boolean;
  editingClipKey: string | null;
  editingClipName: string;
  pendingDeleteKey: string | null;
  draggingItemKey: string | null;
  itemDropTargetKey: string | null;
  itemDropSide: "before" | "after" | null;
  visibleActions: AutomationAction[];
  fallbackGroups: { category: Category; items: CategoryItem[] }[];
  itemCategoryTags: (item: ClipViewItem) => Category[];
  itemDragStyle: (item: ClipViewItem) => CSSProperties | undefined;
  toCategoryClipViewItem: (item: CategoryItem) => ClipViewItem;
}>();

const emit = defineEmits<{
  select: [index: number];
  apply: [item: ClipViewItem];
  expand: [item: ClipViewItem];
  scroll: [event: Event];
  openContextMenu: [payload: { item: ClipViewItem; index: number; x: number; y: number }];
  updateEditingName: [value: string];
  commitRename: [item: ClipViewItem];
  cancelRename: [];
  reorderPointerDown: [payload: { item: ClipViewItem; index: number; event: PointerEvent }];
  hoverPreview: [item: ClipViewItem];
  leavePreview: [item: ClipViewItem];
  // Automation events
  selectAction: [action: AutomationAction];
  runAction: [action: AutomationAction];
  editAction: [action: AutomationAction];
  deleteAction: [action: AutomationAction];
  copyAction: [action: AutomationAction];
  openActionContextMenu: [payload: { action: AutomationAction; x: number; y: number }];
  createAction: [];
}>();

function forwardListRef(el: Element | ComponentPublicInstance | null) {
  listElement.value = el instanceof HTMLElement ? el : null;
  props.listRef(listElement.value);
}

// —— 列表虚拟化（窗口化渲染）——
// 历史条目可达数千条：全量渲染时几千个 ClipCard 实例 + 数万事件监听同时
// 挂载，且任何一次 store 写入都要 patch 整棵列表。卡片是定高双行账本行
// （clip-mini-card：固定 padding + 两行 nowrap 文本 + 固定图标尺寸，
// delete-confirming 只改描边/阴影不影响布局），按"实测高度"做窗口化：
// 只渲染视口 ± OVERSCAN 条，上下用 spacer 撑起总高度，
// 滚动/选中跳转行为对使用者不可见。
const ITEM_FALLBACK_HEIGHT = 50;
const OVERSCAN = 6;
const LIST_GAP = 0; // .raycast-left-pane 的 gap: 0（行间用发丝分隔线）

const listElement = ref<HTMLElement | null>(null);
const viewportHeight = ref(0);
const scrollTop = ref(0);
const itemHeight = ref(ITEM_FALLBACK_HEIGHT);
let viewportObserver: ResizeObserver | null = null;
let scrollFrame: number | null = null;

const itemStride = computed(() => itemHeight.value + LIST_GAP);

const virtualRange = computed(() => {
  const total = props.items.length;
  if (total === 0) return { start: 0, end: 0, topSpacer: 0, bottomSpacer: 0 };
  const stride = itemStride.value;
  const first = Math.floor(Math.max(0, scrollTop.value) / stride);
  const visibleCount = Math.ceil(Math.max(0, viewportHeight.value) / stride) + 1;
  const start = Math.max(0, first - OVERSCAN);
  const end = Math.min(total, first + visibleCount + OVERSCAN);
  // 行间无 gap：spacer 高度就是剩余条数 × 步长。
  const topSpacer = start > 0 ? start * stride : 0;
  const bottomSpacer = (total - end) * stride;
  return { start, end, topSpacer, bottomSpacer };
});

const renderedItems = computed(() => {
  const { start, end } = virtualRange.value;
  return props.items.slice(start, end);
});

function measureViewport() {
  const list = listElement.value;
  if (list) viewportHeight.value = list.clientHeight;
}

function measureItemHeight() {
  const list = listElement.value;
  if (!list) return;
  const card = list.querySelector<HTMLElement>(".clip-mini-card");
  if (!card) return;
  const height = card.getBoundingClientRect().height;
  if (height > 0 && Math.abs(height - itemHeight.value) > 0.5) {
    itemHeight.value = height;
  }
}

function handleScroll(event: Event) {
  emit("scroll", event);
  const list = listElement.value;
  if (!list || scrollFrame !== null) return;
  // rAF 节流：一帧最多更新一次滚动位置（窗口重算依赖它）。
  scrollFrame = window.requestAnimationFrame(() => {
    scrollFrame = null;
    scrollTop.value = list.scrollTop;
  });
}

onMounted(() => {
  measureViewport();
  void nextTick(() => {
    measureItemHeight();
    measureViewport();
  });
  if (typeof ResizeObserver !== "undefined" && listElement.value) {
    viewportObserver = new ResizeObserver(() => measureViewport());
    viewportObserver.observe(listElement.value);
  }
});

onBeforeUnmount(() => {
  viewportObserver?.disconnect();
  viewportObserver = null;
  if (scrollFrame !== null) {
    window.cancelAnimationFrame(scrollFrame);
    scrollFrame = null;
  }
});

// 新条目到达（首条渲染出来）后按实际高度校准一次。
watch(
  () => props.items.length,
  () => {
    void nextTick(measureItemHeight);
  },
  { flush: "post" },
);

// 选中项落在渲染窗口外时直接滚动容器让其进入视口：虚拟化下未渲染的卡片
// 无法被 scrollSelectedClipIntoView 的 querySelector 找到，键盘连续导航
// （尤其 Jump/End 类跳转）会失去滚动跟随。窗口内的精细边缘留白仍由
// useClipListScroll.scrollSelectedClipIntoView 负责。
watch(
  () => [props.selectedIndex, props.items.length],
  () => {
    const { start, end } = virtualRange.value;
    if (props.selectedIndex >= start && props.selectedIndex < end) return;
    const list = listElement.value;
    if (!list) return;
    const target =
      props.selectedIndex * itemStride.value - list.clientHeight / 2 + itemHeight.value / 2;
    list.scrollTop = Math.max(0, target);
    scrollTop.value = list.scrollTop;
  },
  { flush: "post" },
);

/**
 * 空态文案与图标：类型筛选激活且无搜索词时给出筛选语境
 * （"没有图片"能确认筛选在工作，而不是历史为空）；
 * 搜索中或全部段回落到通用空态。
 */
const emptyState = computed(() => {
  if (props.selectedCategoryId !== "history") {
    return { icon: Inbox, title: t("empty.categoryTitle"), description: t("empty.categoryDescription") };
  }
  if (!props.hasSearchQuery && props.typeFilter === "image") {
    return { icon: Image, title: t("empty.noImageTitle"), description: t("empty.noImageDescription") };
  }
  if (!props.hasSearchQuery && props.typeFilter === "text") {
    return { icon: Type, title: t("empty.noTextTitle"), description: t("empty.noTextDescription") };
  }
  return { icon: Inbox, title: t("empty.title"), description: t("empty.description") };
});
</script>

<template>
  <div
    :ref="forwardListRef"
    class="raycast-left-pane"
    @scroll="handleScroll"
  >
    <!-- Automations list if automation category selected -->
    <template v-if="selectedCategoryId === 'automation'">
      <div
        v-if="visibleActions.length"
        class="flex flex-col gap-1.5"
      >
        <AutomationCard
          v-for="(action, index) in visibleActions"
          :key="action.id"
          :action="action"
          :selected="selectedIndex === index"
          @click="emit('selectAction', action)"
          @run="emit('runAction', action)"
          @edit="emit('editAction', action)"
          @delete="emit('deleteAction', action)"
          @copy="emit('copyAction', action)"
          @open-context-menu="emit('openActionContextMenu', { action, x: $event.x, y: $event.y })"
        />
      </div>
      <div
        v-else
        class="empty-state py-12"
      >
        <div class="empty-state-icon">
          <Zap class="size-6 text-[var(--accent)]" />
        </div>
        <h2 class="text-sm font-semibold text-[var(--text-1)]">
          {{ t("automation.entry") }}
        </h2>
        <p class="text-xs text-[var(--text-3)]">
          {{ t("automation.noActions") }}
        </p>
        <button
          type="button"
          class="btn-primary mt-3 text-xs"
          @click="emit('createAction')"
        >
          {{ t("automation.newAction") }}
        </button>
      </div>
    </template>

    <!-- Search fallback groups -->
    <template v-else-if="fallbackGroups.length">
      <div class="flex flex-col gap-3">
        <section
          v-for="group in fallbackGroups"
          :key="group.category.id"
          class="flex flex-col gap-1.5"
        >
          <header class="flex items-center gap-1.5 px-1 py-0.5 text-xs text-[var(--text-2)] font-medium">
            <span
              class="size-2 rounded-full shrink-0 bg-[var(--category-color)]"
              :style="categoryColorVars(group.category.color)"
            />
            <span class="truncate">{{ categoryDisplayName(group.category.name) }}</span>
            <span class="text-[0.625rem] text-[var(--text-3)]">({{ group.items.length }})</span>
          </header>
          <div class="flex flex-col gap-0">
            <ClipCard
              v-for="item in group.items"
              :key="item.id"
              :item="toCategoryClipViewItem(item)"
              :index="0"
              :selected="false"
              :category-tags="[]"
              :editing-name="null"
              :reorder-enabled="false"
              @apply="emit('apply', toCategoryClipViewItem(item))"
              @expand="emit('expand', toCategoryClipViewItem(item))"
              @open-context-menu="emit('openContextMenu', $event)"
            />
          </div>
        </section>
      </div>
    </template>

    <!-- Normal Clip List（虚拟化：仅渲染视口窗口，spacer 撑起总高度） -->
    <template v-else-if="items.length">
      <div
        v-if="virtualRange.topSpacer > 0"
        :style="{ height: `${virtualRange.topSpacer}px`, flexShrink: 0 }"
        aria-hidden="true"
      />
      <ClipCard
        v-for="(item, index) in renderedItems"
        :key="`${item.collection}-${item.id}`"
        :item="item"
        :index="virtualRange.start + index"
        :data-item-key="contextItemKey(item)"
        :data-item-id="item.id"
        :selected="selectedIndex === virtualRange.start + index"
        :category-tags="itemCategoryTags(item)"
        :editing-name="editingClipKey === contextItemKey(item) ? editingClipName : null"
        :reorder-enabled="canReorder && item.collection === 'category'"
        :delete-confirming="pendingDeleteKey === contextItemKey(item)"
        :style="itemDragStyle(item)"
        :class="{
          'clip-card-dragging': draggingItemKey === contextItemKey(item),
          'clip-card-drop-before': itemDropTargetKey === contextItemKey(item) && itemDropSide === 'before',
          'clip-card-drop-after': itemDropTargetKey === contextItemKey(item) && itemDropSide === 'after',
          'clip-card-delete-confirming': pendingDeleteKey === contextItemKey(item),
        }"
        @select="emit('select', virtualRange.start + index)"
        @apply="emit('apply', item)"
        @expand="emit('expand', item)"
        @open-context-menu="emit('openContextMenu', $event)"
        @update-editing-name="emit('updateEditingName', $event)"
        @commit-rename="emit('commitRename', $event)"
        @cancel-rename="emit('cancelRename')"
        @reorder-pointer-down="emit('reorderPointerDown', $event)"
        @pointerenter="emit('hoverPreview', item)"
        @pointerleave="emit('leavePreview', item)"
      />
      <div
        v-if="virtualRange.bottomSpacer > 0"
        :style="{ height: `${virtualRange.bottomSpacer}px`, flexShrink: 0 }"
        aria-hidden="true"
      />

      <!-- Infinite scroll loading shimmer -->
      <div
        v-if="selectedCategoryId === 'history' && isLoadingMore"
        class="skeleton-card w-full"
      />
    </template>

    <!-- Empty State -->
    <div
      v-else
      class="empty-state py-12"
    >
      <div class="empty-state-icon">
        <component
          :is="emptyState.icon"
          class="size-6 text-[var(--text-3)]"
        />
      </div>
      <h2 class="text-sm font-medium text-[var(--text-1)]">
        {{ emptyState.title }}
      </h2>
      <p class="text-xs text-[var(--text-3)]">
        {{ emptyState.description }}
      </p>
    </div>
  </div>
</template>
