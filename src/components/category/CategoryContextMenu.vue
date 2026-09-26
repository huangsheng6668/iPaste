<script setup lang="ts">
import { Palette, Pencil, Trash2 } from "lucide-vue-next";
import { t } from "../../i18n";

// 分类右键菜单：定位由宿主计算（menu.left/top），动作经 emit 交回。
defineProps<{
  menu: { left: number; top: number } | null;
  /** 两击删除确认：第二次点击才真正删除。 */
  isPendingDelete: boolean;
}>();

const emit = defineEmits<{
  rename: [];
  changeColor: [event: MouseEvent];
  delete: [];
}>();
</script>

<template>
  <div
    v-if="menu"
    class="category-context-menu"
    :style="{ left: `${menu.left}px`, top: `${menu.top}px` }"
    @click.stop
    @contextmenu.prevent.stop
  >
    <button
      type="button"
      class="category-context-item"
      tabindex="-1"
      @click="emit('rename')"
    >
      <Pencil class="size-3.5" />
      <span>{{ t("common.rename") }}</span>
    </button>
    <button
      type="button"
      class="category-context-item"
      tabindex="-1"
      @click="emit('changeColor', $event)"
    >
      <Palette class="size-3.5" />
      <span>{{ t("category.changeColor") }}</span>
    </button>
    <div class="context-menu-separator" />
    <button
      type="button"
      class="category-context-item category-context-item-danger"
      :class="{ 'category-context-item-confirm': isPendingDelete }"
      tabindex="-1"
      @click="emit('delete')"
    >
      <Trash2 class="size-3.5" />
      <span>{{ isPendingDelete ? t("common.confirmDelete") : t("category.delete") }}</span>
    </button>
  </div>
</template>
