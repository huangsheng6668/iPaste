<script setup lang="ts">
import { Palette, Pencil, Send, Trash2 } from "lucide-vue-next";
import { t } from "../../i18n";
import { fingerprintOf, type SendTarget } from "../../lib/deviceDisplay";

// 分类右键菜单：定位由宿主计算（menu.left/top），动作经 emit 交回。
defineProps<{
  menu: { left: number; top: number } | null;
  /** 两击删除确认：第二次点击才真正删除。 */
  isPendingDelete: boolean;
  /** 「发送到设备」目标列表（恒含「全部在线」哨兵；空数组 = 无可发送目标）。 */
  sendTargets: SendTarget[];
}>();

const emit = defineEmits<{
  rename: [];
  changeColor: [event: MouseEvent];
  delete: [];
  sendCategory: [targetId: string];
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
    <!-- 整组发送：分类是「一整组片段」，逐条发送会散落在对方历史里，故单独入口 -->
    <div class="category-context-label">
      {{ t("category.sendTo") }}
    </div>
    <template v-if="sendTargets.length > 1">
      <button
        v-for="target in sendTargets"
        :key="target.id"
        type="button"
        class="category-context-item"
        tabindex="-1"
        @click="emit('sendCategory', target.id)"
      >
        <Send class="size-3.5" />
        <span>{{ target.isAll ? t("deviceSync.sendTo.all") : (target.name ?? fingerprintOf(target.id)) }}</span>
      </button>
    </template>
    <div
      v-else
      class="category-context-empty"
    >
      {{ t("deviceSync.sendTo.none") }}
    </div>
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
