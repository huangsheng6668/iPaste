<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Plus, Zap } from "lucide-vue-next";
import { useDragSort } from "../composables/useDragSort";
import CategoryRow from "./category/CategoryRow.vue";
import CategoryContextMenu from "./category/CategoryContextMenu.vue";
import { t } from "../i18n";
import { categoryDisplayName } from "../lib/format";
import type { Category } from "../types";

const CATEGORY_COLOR_OPTIONS = [
  "#2563EB",
  "#0891B2",
  "#0D9488",
  "#059669",
  "#65A30D",
  "#CA8A04",
  "#D97706",
  "#EA580C",
  "#DC2626",
  "#E11D48",
  "#DB2777",
  "#C026D3",
  "#9333EA",
  "#7C3AED",
  "#4F46E5",
  "#0284C7",
  "#475569",
  "#334155",
  "#111827",
  "#78716C",
  "#A16207",
  "#BE123C",
  "#6D28D9",
  "#0F766E",
];

const props = defineProps<{
  categories: Category[];
  selectedCategoryId: string;
  editingCategoryId: string | null;
  historyCount: number;
  categoryCounts: Record<string, number>;
  orientation?: "horizontal" | "vertical";
}>();

const emit = defineEmits<{
  select: [id: string];
  create: [];
  edit: [id: string];
  rename: [category: Category, name: string];
  recolor: [category: Category, color: string];
  finishEditing: [];
  delete: [id: string];
  reorder: [categoryIds: string[]];
}>();

const editingName = ref("");
const railElement = ref<HTMLElement | null>(null);
const categoryScroller = ref<HTMLElement | null>(null);
const pendingDeleteCategoryId = ref<string | null>(null);
const editingColorCategoryId = ref<string | null>(null);
const colorPopoverPosition = ref({ left: 0, top: 0 });
const isCategoryScrolling = ref(false);
const committedEditingId = ref<string | null>(null);
const categoryMenu = ref<{ category: Category; left: number; top: number } | null>(null);
const categoryMenuAnchorRect = ref<DOMRect | null>(null);
let focusTimer: number | null = null;
let categoryScrollTimer: number | null = null;
let suppressNextCategoryClick = false;

watch(
  () => props.editingCategoryId,
  async (id) => {
    if (!id) return;

    const category = props.categories.find((item) => item.id === id);
    editingName.value = category ? categoryDisplayName(category.name) : "";
    committedEditingId.value = null;
    await nextTick();
    scrollCategoryIntoView(id);
    focusEditingInput();
  },
);

watch(
  () => props.categories.map((category) => category.id),
  (ids) => {
    if (pendingDeleteCategoryId.value && !ids.includes(pendingDeleteCategoryId.value)) {
      pendingDeleteCategoryId.value = null;
    }
    if (editingColorCategoryId.value && !ids.includes(editingColorCategoryId.value)) {
      editingColorCategoryId.value = null;
    }
  },
);

watch(
  () => props.selectedCategoryId,
  async (id) => {
    if (id === "history") return;
    await nextTick();
    scrollCategoryIntoView(id);
  },
);

onMounted(() => {
  window.addEventListener("resize", closeFloatingLayers);
  document.addEventListener("click", closeColorPicker);
  document.addEventListener("click", closeCategoryMenu);
});

onUnmounted(() => {
  window.removeEventListener("resize", closeFloatingLayers);
  document.removeEventListener("click", closeColorPicker);
  document.removeEventListener("click", closeCategoryMenu);
  clearCategoryScrollTimer();
  cleanupCategoryDrag();
  if (focusTimer !== null) {
    window.clearTimeout(focusTimer);
    focusTimer = null;
  }
});

function focusEditingInput() {
  if (focusTimer !== null) {
    window.clearTimeout(focusTimer);
  }

  focusTimer = window.setTimeout(() => {
    const input = railElement.value?.querySelector<HTMLInputElement>(".category-chip-input");
    input?.focus();
    focusTimer = null;
  }, 40);
}

function scrollCategoryIntoView(id: string) {
  const scroller = categoryScroller.value;
  const chip = railElement.value?.querySelector<HTMLElement>(`[data-category-id="${id}"]`);
  if (!scroller || !chip) return;

  const scrollerRect = scroller.getBoundingClientRect();
  const chipRect = chip.getBoundingClientRect();
  const edgePadding = 12;

  if (props.orientation === "vertical") {
    if (chipRect.top < scrollerRect.top + edgePadding) {
      scroller.scrollBy({ top: chipRect.top - scrollerRect.top - edgePadding, behavior: "smooth" });
      showCategoryScrollbar();
      return;
    }

    if (chipRect.bottom > scrollerRect.bottom - edgePadding) {
      scroller.scrollBy({ top: chipRect.bottom - scrollerRect.bottom + edgePadding, behavior: "smooth" });
      showCategoryScrollbar();
    }
    return;
  }

  if (chipRect.left < scrollerRect.left + edgePadding) {
    scroller.scrollBy({ left: chipRect.left - scrollerRect.left - edgePadding, behavior: "smooth" });
    showCategoryScrollbar();
    return;
  }

  if (chipRect.right > scrollerRect.right - edgePadding) {
    scroller.scrollBy({ left: chipRect.right - scrollerRect.right + edgePadding, behavior: "smooth" });
    showCategoryScrollbar();
  }
}

function commitEditing(category: Category) {
  if (committedEditingId.value === category.id) return;
  committedEditingId.value = category.id;

  const name = editingName.value.trim();
  if (name && name !== categoryDisplayName(category.name)) {
    emit("rename", category, name);
  }
  emit("finishEditing");
}

/**
 * 取消改名：**必须先把该行标记为已提交**。
 *
 * 取消会让 `editingCategoryId` 归零 → 输入框被卸载 → 浏览器触发 blur →
 * 输入框上的 `@blur="emit('commit')"` 又跑一次 commitEditing，把用户刚敲的
 * 名字当作确认结果存下去（Esc 取消实际变成保存）。这里占住 committedEditingId，
 * 随后的 blur-commit 就会被 commitEditing 的幂等守卫挡掉。
 */
function cancelEditing(category: Category) {
  committedEditingId.value = category.id;
  emit("finishEditing");
}

function editCategory(category: Category) {
  pendingDeleteCategoryId.value = null;
  editingColorCategoryId.value = null;
  categoryMenu.value = null;
  committedEditingId.value = null;
  emit("edit", category.id);
}

function selectCategory(id: string) {
  if (suppressNextCategoryClick) {
    suppressNextCategoryClick = false;
    return;
  }

  pendingDeleteCategoryId.value = null;
  editingColorCategoryId.value = null;
  categoryMenuAnchorRect.value = null;
  categoryMenu.value = null;
  emit("select", id);
}

function requestDeleteCategory(id: string, options: { keepMenuOpen?: boolean } = {}) {
  editingColorCategoryId.value = null;

  if (pendingDeleteCategoryId.value === id) {
    pendingDeleteCategoryId.value = null;
    categoryMenu.value = null;
    emit("delete", id);
    return;
  }

  pendingDeleteCategoryId.value = id;
  if (!options.keepMenuOpen) {
    categoryMenu.value = null;
  }
}

function openColorPicker(category: Category, event: MouseEvent) {
  pendingDeleteCategoryId.value = null;
  categoryMenu.value = null;

  const trigger = event.currentTarget as HTMLElement;
  const chip = railElement.value?.querySelector<HTMLElement>(`[data-category-id="${category.id}"]`);
  const rect = chip?.getBoundingClientRect() ?? categoryMenuAnchorRect.value ?? trigger.getBoundingClientRect();
  const popoverWidth = 184;
  const estimatedPopoverHeight = 196;
  const padding = 8;
  const left = props.orientation === "vertical"
    ? rect.right + 8
    : rect.left;
  const top = props.orientation === "vertical"
    ? rect.top - 8
    : rect.bottom + 8;
  colorPopoverPosition.value = {
    left: Math.min(Math.max(left, padding), window.innerWidth - popoverWidth - padding),
    top: Math.min(Math.max(top, padding), window.innerHeight - estimatedPopoverHeight - padding),
  };
  editingColorCategoryId.value =
    editingColorCategoryId.value === category.id ? null : category.id;
}

function closeColorPicker() {
  editingColorCategoryId.value = null;
  categoryMenuAnchorRect.value = null;
}

function updateColor(category: Category, color: string) {
  if (color.toLowerCase() !== category.color.toLowerCase()) {
    emit("recolor", category, color);
  }
  closeColorPicker();
}

function handleCategoryWheel(event: WheelEvent) {
  const scroller = categoryScroller.value;
  if (!scroller) return;

  if (props.orientation === "vertical") {
    showCategoryScrollbar();
    return;
  }

  const canScroll = scroller.scrollWidth > scroller.clientWidth;
  if (!canScroll) return;

  const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
  if (!delta) return;

  event.preventDefault();
  showCategoryScrollbar();
  scroller.scrollLeft += delta;
}

function activateCategoryScrollbar() {
  isCategoryScrolling.value = true;
}

function scheduleHideCategoryScrollbar() {
  clearCategoryScrollTimer();
  categoryScrollTimer = window.setTimeout(() => {
    isCategoryScrolling.value = false;
    categoryScrollTimer = null;
  }, 520);
}

function showCategoryScrollbar() {
  clearCategoryScrollTimer();
  isCategoryScrolling.value = true;
  scheduleHideCategoryScrollbar();
}

function clearCategoryScrollTimer() {
  if (categoryScrollTimer === null) return;
  window.clearTimeout(categoryScrollTimer);
  categoryScrollTimer = null;
}

// 显式标注断开 config 箭头函数对 drag 自身的推断循环（vue-tsc TS7022/TS7023）。
const drag: ReturnType<typeof useDragSort<Category>> = useDragSort<Category>({
  canStart: ({ event }) => !props.editingCategoryId && event.button === 0,
  items: () => props.categories,
  itemKey: (category) => category.id,
  itemId: (category) => category.id,
  targetFromPoint: (clientX, clientY) => categoryTargetFromPoint(drag.draggingKey.value ?? "", clientX, clientY),
  onReorder: (categoryIds) => emit("reorder", categoryIds),
  container: () => categoryScroller.value,
  orientation: () => (props.orientation === "vertical" ? "vertical" : "horizontal"),
  edge: 28,
  scrollStep: 12,
  lockAxis: true,
  sourceSelector: "[data-category-id]",
  onDragStarted: () => {
    pendingDeleteCategoryId.value = null;
    editingColorCategoryId.value = null;
    categoryMenu.value = null;
  },
  onDragFinished: () => {
    suppressNextCategoryClick = true;
    window.setTimeout(() => {
      suppressNextCategoryClick = false;
    }, 0);
  },
  onEdgeScroll: () => showCategoryScrollbar(),
});
const {
  draggingKey: draggingCategoryId,
  dropTargetKey: categoryDropTargetId,
  dropSide: categoryDropSide,
  dragStyle: categoryDragStyle,
  cleanup: cleanupCategoryDrag,
} = drag;

function startCategoryDrag(category: Category, event: PointerEvent) {
  drag.start({ item: category, event });
}

function categoryTargetFromPoint(draggedId: string, clientX: number, clientY: number) {
  const scroller = categoryScroller.value;
  const rail = railElement.value;
  if (!scroller || !rail) return null;

  const scrollerRect = scroller.getBoundingClientRect();
  const tolerance = 28;
  if (props.orientation === "vertical") {
    if (clientX < scrollerRect.left - tolerance || clientX > scrollerRect.right + tolerance) {
      return null;
    }
  } else if (clientY < scrollerRect.top - tolerance || clientY > scrollerRect.bottom + tolerance) {
    return null;
  }

  const targets = Array.from(rail.querySelectorAll<HTMLElement>("[data-category-id]"))
    .map((chip) => {
      const id = chip.dataset.categoryId;
      return id && id !== draggedId ? { id, rect: chip.getBoundingClientRect() } : null;
    })
    .filter((item): item is { id: string; rect: DOMRect } => Boolean(item));

  if (!targets.length) return null;

  const beforeTarget = targets.find((target) =>
    props.orientation === "vertical"
      ? clientY < target.rect.top + target.rect.height / 2
      : clientX < target.rect.left + target.rect.width / 2,
  );
  const resolved = beforeTarget ?? targets[targets.length - 1];
  return {
    key: resolved.id,
    id: resolved.id,
    rect: resolved.rect,
  };
}

function openCategoryMenu(category: Category, event: MouseEvent) {
  event.preventDefault();
  event.stopPropagation();
  pendingDeleteCategoryId.value = null;
  editingColorCategoryId.value = null;
  categoryMenuAnchorRect.value = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const menuWidth = 168;
  const padding = 8;
  categoryMenu.value = {
    category,
    left: Math.min(Math.max(event.clientX, padding), window.innerWidth - menuWidth - padding),
    top: Math.min(Math.max(event.clientY, padding), window.innerHeight - 180),
  };
}

function closeCategoryMenu() {
  categoryMenu.value = null;
}

function closeFloatingLayers() {
  pendingDeleteCategoryId.value = null;
  editingColorCategoryId.value = null;
  categoryMenuAnchorRect.value = null;
  categoryMenu.value = null;
}

defineExpose({
  closeFloatingLayers,
});

function countLabel(count: number | undefined) {
  return (count ?? 0) > 99 ? "99+" : String(Math.max(count ?? 0, 0));
}
</script>

<template>
  <section
    ref="railElement"
    class="tag-strip"
    :class="{ 'tag-strip-vertical': orientation === 'vertical' }"
  >
    <nav
      ref="categoryScroller"
      class="category-scroll subtle-scrollbar min-w-0 flex-1 overflow-x-auto py-1"
      :class="{
        'category-scroll-vertical': orientation === 'vertical',
        'subtle-scrollbar-active': isCategoryScrolling,
      }"
      @wheel="handleCategoryWheel"
      @scroll="showCategoryScrollbar"
      @mouseenter="activateCategoryScrollbar"
      @mouseleave="scheduleHideCategoryScrollbar"
    >
      <button
        type="button"
        class="category-chip"
        :class="{ 'category-chip-active': selectedCategoryId === 'history' }"
        tabindex="-1"
        @click="selectCategory('history')"
      >
        <span class="category-count-dot category-count-dot-history">{{ countLabel(historyCount) }}</span>
        <span class="category-chip-label">{{ t("category.history") }}</span>
      </button>

      <CategoryRow
        v-for="category in categories"
        :key="category.id"
        v-model:editing-name="editingName"
        :data-category-id="category.id"
        :category="category"
        :label="categoryDisplayName(category.name)"
        :count="countLabel(categoryCounts[category.id])"
        :is-selected="selectedCategoryId === category.id"
        :is-editing="editingCategoryId === category.id"
        :is-dragging="draggingCategoryId === category.id"
        :drop-side="categoryDropTargetId === category.id ? categoryDropSide : null"
        :drag-style="categoryDragStyle(category)"
        :is-color-picker-open="editingColorCategoryId === category.id"
        :color-popover-position="colorPopoverPosition"
        :color-options="CATEGORY_COLOR_OPTIONS"
        @select="selectCategory(category.id)"
        @edit="editCategory(category)"
        @context-menu="openCategoryMenu(category, $event)"
        @drag-start="startCategoryDrag(category, $event)"
        @commit="commitEditing(category)"
        @cancel-edit="cancelEditing(category)"
        @recolor="updateColor(category, $event)"
        @close-color-picker="closeColorPicker"
      />

      <button
        type="button"
        class="category-chip category-chip-actions"
        :class="{ 'category-chip-active': selectedCategoryId === 'actions' }"
        tabindex="-1"
        @click="selectCategory('actions')"
      >
        <Zap class="size-4" />
        <span class="category-chip-label">{{ t("automation.entry") }}</span>
      </button>
    </nav>

    <div class="category-create-wrap flex shrink-0 items-center gap-2">
      <button
        type="button"
        class="category-chip category-chip-create"
        tabindex="-1"
        @click="emit('create')"
      >
        <Plus class="size-4" />
        <span>{{ t("category.create") }}</span>
      </button>
    </div>

    <CategoryContextMenu
      :menu="categoryMenu"
      :is-pending-delete="pendingDeleteCategoryId === categoryMenu?.category.id"
      @rename="categoryMenu && editCategory(categoryMenu.category)"
      @change-color="categoryMenu && openColorPicker(categoryMenu.category, $event)"
      @delete="categoryMenu && requestDeleteCategory(categoryMenu.category.id, { keepMenuOpen: true })"
    />
  </section>
</template>
