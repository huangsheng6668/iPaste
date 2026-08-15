<script setup lang="ts">
import { ChevronRight, ClipboardCopy, CornerDownLeft, FolderInput, Pencil, Plus, Trash2 } from "lucide-vue-next";
import { nextTick, ref, watch } from "vue";
import { t } from "../i18n";
import { categoryDisplayName } from "../lib/format";
import type { Category, ClipViewItem } from "../types";

const props = defineProps<{
  contextMenu: { item: ClipViewItem; index: number; x: number; y: number };
  categories: Category[];
  deleteLabel: string;
  deleteConfirming: boolean;
  showMoveSubmenu: boolean;
}>();

const emit = defineEmits<{
  paste: [];
  copy: [];
  rename: [];
  "move-to": [categoryId: string];
  "create-category": [];
  delete: [];
  "open-move-submenu": [];
  "schedule-close-move-submenu": [];
  "reset-pending-delete": [];
}>();

const contextMenuElement = ref<HTMLElement | null>(null);
const moveSubmenuBranchElement = ref<HTMLElement | null>(null);
const moveSubmenuElement = ref<HTMLElement | null>(null);
const submenuAlignLeft = ref(false);
const submenuOffsetTop = ref(0);
const menuPosition = ref({ x: 0, y: 0 });

watch(
  () => props.contextMenu,
  (menu) => {
    if (!menu) return;
    menuPosition.value = { x: menu.x, y: menu.y };
    void nextTick(positionContextMenu);
  },
  { immediate: true },
);

watch(
  () => props.showMoveSubmenu,
  (open) => {
    if (open) {
      void nextTick(positionMoveSubmenu);
    } else {
      submenuOffsetTop.value = 0;
    }
  },
);

function positionContextMenu() {
  if (!props.contextMenu || !contextMenuElement.value) return;

  const rect = contextMenuElement.value.getBoundingClientRect();
  const padding = 8;
  const maxX = Math.max(padding, window.innerWidth - rect.width - padding);
  const maxY = Math.max(padding, window.innerHeight - rect.height - padding);
  menuPosition.value = {
    x: clamp(menuPosition.value.x, padding, maxX),
    y: clamp(menuPosition.value.y, padding, maxY),
  };
  positionMoveSubmenu();
}

function positionMoveSubmenu() {
  if (!moveSubmenuBranchElement.value || !moveSubmenuElement.value) return;

  const branchRect = moveSubmenuBranchElement.value.getBoundingClientRect();
  const submenuRect = moveSubmenuElement.value.getBoundingClientRect();
  const padding = 8;
  const maxY = Math.max(padding, window.innerHeight - submenuRect.height - padding);

  submenuAlignLeft.value = branchRect.right + submenuRect.width + padding > window.innerWidth
    && branchRect.left - submenuRect.width - padding >= padding;
  submenuOffsetTop.value = clamp(branchRect.top, padding, maxY) - branchRect.top;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}
</script>

<template>
  <div
    ref="contextMenuElement"
    class="clip-context-menu"
    :style="{ left: `${menuPosition.x}px`, top: `${menuPosition.y}px` }"
    role="menu"
    @click.stop
    @contextmenu.prevent.stop
    @mouseleave="emit('reset-pending-delete')"
  >
    <button
      type="button"
      class="context-menu-item context-menu-item-strong"
      tabindex="-1"
      role="menuitem"
      @click="emit('paste')"
    >
      <CornerDownLeft class="size-4" />
      <span>{{ t("common.paste") }}</span>
    </button>
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('copy')"
    >
      <ClipboardCopy class="size-4" />
      <span>{{ t("common.copy") }}</span>
    </button>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="context-menu-item"
      tabindex="-1"
      role="menuitem"
      @click="emit('rename')"
    >
      <Pencil class="size-4" />
      <span>{{ t("common.rename") }}</span>
    </button>
    <div
      ref="moveSubmenuBranchElement"
      class="context-menu-branch"
      :class="{ 'context-menu-branch-left': submenuAlignLeft }"
      @mouseenter="emit('open-move-submenu')"
      @mouseleave="emit('schedule-close-move-submenu')"
    >
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click.stop="emit('open-move-submenu')"
      >
        <FolderInput class="size-4" />
        <span>{{ t("context.moveTo") }}</span>
        <ChevronRight class="ml-auto size-4 text-slate-400" />
      </button>
      <div
        v-if="showMoveSubmenu"
        ref="moveSubmenuElement"
        class="clip-context-submenu"
        :class="{ 'clip-context-submenu-left': submenuAlignLeft }"
        :style="{ top: `${submenuOffsetTop}px` }"
        @mouseenter="emit('open-move-submenu')"
        @mouseleave="emit('schedule-close-move-submenu')"
      >
        <button
          v-for="category in categories"
          :key="category.id"
          type="button"
          class="context-menu-item"
          tabindex="-1"
          role="menuitem"
          @click="emit('move-to', category.id)"
        >
          <span
            class="size-2 rounded-full"
            :style="{ backgroundColor: category.color }"
          />
          <span class="min-w-0 flex-1 truncate">{{ categoryDisplayName(category.name) }}</span>
        </button>
        <div
          v-if="categories.length"
          class="context-menu-separator"
        />
        <button
          type="button"
          class="context-menu-item"
          tabindex="-1"
          role="menuitem"
          @click="emit('create-category')"
        >
          <Plus class="size-4" />
          <span>{{ t("context.createCategory") }}</span>
        </button>
      </div>
    </div>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="context-menu-item context-menu-item-danger"
      :class="{ 'context-menu-item-confirm': deleteConfirming }"
      tabindex="-1"
      role="menuitem"
      @click="emit('delete')"
      @mouseleave="emit('reset-pending-delete')"
    >
      <Trash2 class="size-4" />
      <span>{{ deleteLabel }}</span>
    </button>
  </div>
</template>
