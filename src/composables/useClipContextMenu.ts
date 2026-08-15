import { ref } from "vue";
import { t } from "../i18n";
import { contextItemKey, originalClipId } from "../lib/clipKeys";
import { ipasteApi } from "../lib/ipasteApi";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { ClipViewItem } from "../types";

type IpasteStore = ReturnType<typeof useIpasteStore>;

type ClipContextMenuOptions = {
  onStartRename: (item: ClipViewItem) => void | Promise<void>;
};

const CATEGORY_COLORS = ["#0D9488", "#2563EB", "#7C3AED", "#D97706", "#DC2626", "#475569"];

/**
 * 剪贴卡片右键菜单（含“移动到分类”子菜单）的开关状态与业务动作（原 App.vue
 * context menu 段）。定位（positionContextMenu/positionMoveSubmenu 与元素 ref）
 * 由 ClipContextMenu.vue 负责：组件 watch contextMenu/showMoveSubmenu prop 触发。
 * 全量浮层关闭（quick preview、分类栏）仍由 App.vue 的 closeFloatingLayers 编排。
 */
export function useClipContextMenu(store: IpasteStore, options: ClipContextMenuOptions) {
  const contextMenu = ref<{ item: ClipViewItem; index: number; x: number; y: number } | null>(null);
  const showMoveSubmenu = ref(false);
  const pendingDeleteContextKey = ref<string | null>(null);
  const pendingDeleteByKey = ref<string | null>(null);
  const editingCategoryId = ref<string | null>(null);
  let moveSubmenuCloseTimer: number | null = null;

  function openFallbackContextMenu(payload: { item: ClipViewItem; index: number; x: number; y: number }) {
    contextMenu.value = payload;
  }

  function openClipContextMenu(payload: { item: ClipViewItem; index: number; x: number; y: number }) {
    store.setSelectedIndex(payload.index);
    pendingDeleteContextKey.value = null;
    pendingDeleteByKey.value = null;
    contextMenu.value = payload;
  }

  async function pasteContextItem() {
    const item = contextMenu.value?.item;
    close();
    if (!item) return;
    await store.applyItem(item);
  }

  async function copyContextItem() {
    const item = contextMenu.value?.item;
    close();
    if (!item) return;
    await store.copyItem(item);
  }

  async function renameContextItem() {
    const item = contextMenu.value?.item;
    close();
    if (!item) return;

    await options.onStartRename(item);
  }

  async function addContextItemToCategory(categoryId: string) {
    const item = contextMenu.value?.item;
    if (!item) return;
    close();
    await addItemToCategory(item, categoryId);
  }

  async function addItemToCategory(item: ClipViewItem, categoryId: string) {
    const clipId = originalClipId(item);
    await store.addToCategory(clipId, categoryId);
  }

  async function deleteContextItem() {
    const item = contextMenu.value?.item;
    if (!item) return;

    const deleteKey = contextItemKey(item);
    if (pendingDeleteContextKey.value !== deleteKey) {
      pendingDeleteContextKey.value = deleteKey;
      return;
    }

    close();
    if (item.collection === "history") {
      await store.deleteClip(item.id);
      return;
    }

    await store.removeCategoryItem(item.id);
  }

  async function deleteSelectedItem(item: ClipViewItem) {
    pendingDeleteByKey.value = null;
    if (item.collection === "history") {
      await store.deleteClip(item.id);
      return;
    }

    await store.removeCategoryItem(item.id);
  }

  function contextDeleteLabel(item: ClipViewItem) {
    const isPending = pendingDeleteContextKey.value === contextItemKey(item);
    if (item.collection === "history") return isPending ? t("common.confirmDelete") : t("common.delete");
    return isPending ? t("context.confirmRemove") : t("context.removeFromCategory");
  }

  async function createCategoryForContextItem() {
    const item = contextMenu.value?.item;
    if (!item) return;

    close();
    const clipId = originalClipId(item);
    const color = CATEGORY_COLORS[store.categories.length % CATEGORY_COLORS.length];
    const { category, item: categoryItem } = await ipasteApi.createCategoryWithClip(t("category.newCategory"), color, clipId);
    store.categories.push(category);
    store.categoryItems.push(categoryItem);
    store.clips = store.clips.map((clip) =>
      clip.id === clipId ? { ...clip, favoriteCount: clip.favoriteCount + 1 } : clip,
    );
    store.selectCategory(category.id);
    store.syncCloudInBackground();
    editingCategoryId.value = category.id;
  }

  function openMoveSubmenu() {
    clearMoveSubmenuCloseTimer();
    showMoveSubmenu.value = true;
  }

  function scheduleCloseMoveSubmenu() {
    clearMoveSubmenuCloseTimer();
    moveSubmenuCloseTimer = window.setTimeout(() => {
      showMoveSubmenu.value = false;
      moveSubmenuCloseTimer = null;
    }, 120);
  }

  function closeMoveSubmenu() {
    clearMoveSubmenuCloseTimer();
    showMoveSubmenu.value = false;
  }

  function clearMoveSubmenuCloseTimer() {
    if (moveSubmenuCloseTimer === null) return;
    window.clearTimeout(moveSubmenuCloseTimer);
    moveSubmenuCloseTimer = null;
  }

  function resetPendingDelete() {
    pendingDeleteContextKey.value = null;
  }

  function close() {
    contextMenu.value = null;
    pendingDeleteContextKey.value = null;
    pendingDeleteByKey.value = null;
    closeMoveSubmenu();
  }

  return {
    contextMenu,
    showMoveSubmenu,
    pendingDeleteContextKey,
    pendingDeleteByKey,
    editingCategoryId,
    contextDeleteLabel,
    openFallbackContextMenu,
    openClipContextMenu,
    pasteContextItem,
    copyContextItem,
    renameContextItem,
    addContextItemToCategory,
    deleteContextItem,
    deleteSelectedItem,
    createCategoryForContextItem,
    openMoveSubmenu,
    scheduleCloseMoveSubmenu,
    closeMoveSubmenu,
    clearMoveSubmenuCloseTimer,
    resetPendingDelete,
    close,
  };
}
