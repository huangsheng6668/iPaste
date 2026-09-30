<script setup lang="ts">
import { Check, Palette } from "lucide-vue-next";
import { t } from "../../i18n";
import { contrastText } from "../../lib/colorFormat";
import type { CSSProperties } from "vue";
import type { Category } from "../../types";

// 分类栏的单行（含内联改名与颜色浮层）：纯展示 + emit，
// 选中态、编辑态、拖拽态与浮层位置全部由宿主 CategoryRail 传入。
defineProps<{
  category: Category;
  /** 显示名（已按语言与空名兜底处理）。 */
  label: string;
  /** 条数徽标文本。 */
  count: string;
  isSelected: boolean;
  isEditing: boolean;
  isDragging: boolean;
  dropSide: "before" | "after" | null;
  dragStyle: CSSProperties | undefined;
  isColorPickerOpen: boolean;
  colorPopoverPosition: { left: number; top: number };
  colorOptions: readonly string[];
}>();

const editingName = defineModel<string>("editingName", { required: true });

const emit = defineEmits<{
  select: [];
  edit: [];
  contextMenu: [event: MouseEvent];
  dragStart: [event: PointerEvent];
  commit: [];
  cancelEdit: [];
  recolor: [color: string];
  closeColorPicker: [];
}>();
</script>

<template>
  <div
    class="category-chip category-chip-group group"
    :class="{
      'category-chip-active': isSelected,
      'category-chip-dragging': isDragging,
      'category-chip-drop-before': dropSide === 'before',
      'category-chip-drop-after': dropSide === 'after',
    }"
    :style="dragStyle"
    @click="emit('select')"
    @dblclick.stop="emit('edit')"
    @contextmenu="emit('contextMenu', $event)"
    @pointerdown="emit('dragStart', $event)"
  >
    <span
      v-if="!isEditing"
      class="category-color-dot category-count-dot"
      :style="{ backgroundColor: category.color, color: contrastText(category.color) }"
    >
      {{ count }}
    </span>
    <span
      v-if="!isEditing"
      class="category-chip-label"
    >
      {{ label }}
    </span>
    <input
      v-else
      v-model="editingName"
      class="category-chip-input"
      tabindex="-1"
      @click.stop
      @keydown.enter.prevent.stop="emit('commit')"
      @keydown.escape.prevent.stop="emit('cancelEdit')"
      @blur="emit('commit')"
    >
    <!-- 必须 Teleport 到 body。
         宿主 .tag-strip 带 backdrop-filter，这会同时创建层叠上下文、并让 .tag-strip
         成为 position:fixed 后代的包含块。浮层若留在这里：z-index:80 只在
         .tag-strip 内部竞争，而 .tag-strip 自身是无定位普通块，在 .raycast-container
         里绘制层级低于兄弟节点里 position:relative 的卡片 —— 于是被列表盖住；
         同时 .raycast-container 的 overflow:hidden 还会把它裁掉。
         移到 body 后包含块回到视口，与 openColorPicker 用 getBoundingClientRect
         算出的视口坐标一致（此前会被 .tag-strip 的偏移二次叠加）。 -->
    <Teleport to="body">
      <div
        v-if="isColorPickerOpen"
        class="category-color-popover"
        :style="{ left: `${colorPopoverPosition.left}px`, top: `${colorPopoverPosition.top}px` }"
        @click.stop
        @pointerdown.stop
        @mouseleave="emit('closeColorPicker')"
      >
        <div class="category-color-popover-title">
          <Palette class="size-3.5" />
          <span>{{ t("category.color") }}</span>
        </div>
        <div class="category-color-grid">
          <button
            v-for="color in colorOptions"
            :key="color"
            type="button"
            class="category-color-swatch"
            :class="{ 'category-color-swatch-active': color.toLowerCase() === category.color.toLowerCase() }"
            :style="{ backgroundColor: color, color: contrastText(color) }"
            :aria-label="t('category.selectColor', { color })"
            tabindex="-1"
            @click="emit('recolor', color)"
          >
            <Check
              v-if="color.toLowerCase() === category.color.toLowerCase()"
              class="size-3.5"
            />
          </button>
        </div>
        <label class="category-custom-color">
          <input
            type="color"
            :value="category.color"
            tabindex="-1"
            @change="emit('recolor', ($event.target as HTMLInputElement).value)"
          >
          <span>{{ t("category.customColor") }}</span>
        </label>
      </div>
    </Teleport>
  </div>
</template>
