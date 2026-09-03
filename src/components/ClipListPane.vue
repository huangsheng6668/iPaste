<script setup lang="ts">
import { type CSSProperties, type ComponentPublicInstance } from "vue";
import { Inbox, Zap } from "lucide-vue-next";
import ClipCard from "./ClipCard.vue";
import AutomationCard from "./AutomationCard.vue";
import { t } from "../i18n";
import { contextItemKey } from "../lib/clipKeys";
import { categoryDisplayName } from "../lib/format";
import type { AutomationAction, Category, CategoryItem, ClipViewItem } from "../types";

const { listRef } = defineProps<{
  // 根节点 .raycast-left-pane 是滚动容器；父组件经此拿到 DOM 以做触底加载与选中滚动
  listRef: (el: HTMLElement | null) => void;
  items: ClipViewItem[];
  selectedIndex: number;
  selectedCategoryId: string;
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
  listRef(el instanceof HTMLElement ? el : null);
}
</script>

<template>
  <div
    :ref="forwardListRef"
    class="raycast-left-pane"
    @scroll="emit('scroll', $event)"
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
              class="size-2 rounded-full shrink-0"
              :style="{ backgroundColor: group.category.color }"
            />
            <span class="truncate">{{ categoryDisplayName(group.category.name) }}</span>
            <span class="text-[0.625rem] text-[var(--text-3)]">({{ group.items.length }})</span>
          </header>
          <div class="flex flex-col gap-1">
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

    <!-- Normal Clip List -->
    <template v-else-if="items.length">
      <ClipCard
        v-for="(item, index) in items"
        :key="`${item.collection}-${item.id}`"
        :item="item"
        :index="index"
        :data-item-key="contextItemKey(item)"
        :data-item-id="item.id"
        :selected="selectedIndex === index"
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
        @select="emit('select', index)"
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

      <!-- Infinite scroll loading shimmer -->
      <div
        v-if="selectedCategoryId === 'history' && isLoadingMore"
        class="skeleton-card h-12 w-full rounded-md"
      />
    </template>

    <!-- Empty State -->
    <div
      v-else
      class="empty-state py-12"
    >
      <div class="empty-state-icon">
        <Inbox class="size-6 text-[var(--text-3)]" />
      </div>
      <h2 class="text-sm font-medium text-[var(--text-1)]">
        {{ selectedCategoryId === "history" ? t("empty.title") : t("empty.categoryTitle") }}
      </h2>
      <p class="text-xs text-[var(--text-3)]">
        {{ selectedCategoryId === "history" ? t("empty.description") : t("empty.categoryDescription") }}
      </p>
    </div>
  </div>
</template>
