import { ref } from "vue";
import { t } from "../i18n";
import { contextItemKey, originalClipId } from "../lib/clipKeys";
import { SEND_TARGET_ALL } from "../lib/deviceDisplay";
import { ipasteApi, type LanClipSource } from "../lib/ipasteApi";
import { showError } from "../stores/uiStore";
import type { useIpasteStore } from "../stores/ipasteStore";
import type { ClipViewItem } from "../types";

type IpasteStore = ReturnType<typeof useIpasteStore>;

type ClipContextMenuOptions = {
  onStartRename: (item: ClipViewItem) => void | Promise<void>;
  /** 菜单动作执行前的全量浮层关闭（quick preview、分类栏）；缺省回落到菜单自身 close()。 */
  onFullClose?: () => void;
  /** 右键菜单打开与「发送到」子菜单展开时刷新在线设备目标列表（App.vue 负责拉取 deviceList）。 */
  refreshSendTargets?: () => void;
};

const CATEGORY_COLORS = ["#0D9488", "#2563EB", "#7C3AED", "#D97706", "#DC2626", "#475569"];

/**
 * 剪贴卡片右键菜单（含“移动到分类”“发送到设备”两个子菜单）的开关状态与业务动作
 * （原 App.vue context menu 段）。定位（positionContextMenu/positionMoveSubmenu/
 * positionSendSubmenu 与元素 ref）由 ClipContextMenu.vue 负责：组件 watch
 * contextMenu/showMoveSubmenu/showSendSubmenu prop 触发。全量浮层关闭（quick preview、
 * 分类栏）仍由 App.vue 的 closeFloatingLayers 编排。
 */
export function useClipContextMenu(store: IpasteStore, options: ClipContextMenuOptions) {
  const contextMenu = ref<{ item: ClipViewItem; index: number; x: number; y: number } | null>(null);
  const showMoveSubmenu = ref(false);
  const showSendSubmenu = ref(false);
  const pendingDeleteContextKey = ref<string | null>(null);
  const pendingDeleteByKey = ref<string | null>(null);
  const editingCategoryId = ref<string | null>(null);
  let moveSubmenuCloseTimer: number | null = null;
  let sendSubmenuCloseTimer: number | null = null;

  function openFallbackContextMenu(payload: { item: ClipViewItem; index: number; x: number; y: number }) {
    contextMenu.value = payload;
  }

  function openClipContextMenu(payload: { item: ClipViewItem; index: number; x: number; y: number }) {
    store.setSelectedIndex(payload.index);
    pendingDeleteContextKey.value = null;
    pendingDeleteByKey.value = null;
    contextMenu.value = payload;
    // 预热「发送到」目标列表：右键即拉取，悬停展开子菜单时已就绪。
    options.refreshSendTargets?.();
  }

  async function pasteContextItem() {
    const item = contextMenu.value?.item;
    fullClose();
    if (!item) return;
    await store.applyItem(item);
  }

  async function copyContextItem() {
    const item = contextMenu.value?.item;
    fullClose();
    if (!item) return;
    await store.copyItem(item);
  }

  async function renameContextItem() {
    const item = contextMenu.value?.item;
    fullClose();
    if (!item) return;

    await options.onStartRename(item);
  }

  async function addContextItemToCategory(categoryId: string) {
    const item = contextMenu.value?.item;
    if (!item) return;
    fullClose();
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

    fullClose();
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

    fullClose();
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
    // 两个子菜单互斥：展开移动子菜单时立即收起发送子菜单，避免层叠。
    closeSendSubmenu();
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

  function openSendSubmenu() {
    clearSendSubmenuCloseTimer();
    closeMoveSubmenu();
    showSendSubmenu.value = true;
    // 展开即刷新：设备在线态可能自右键预热后又变化。
    options.refreshSendTargets?.();
  }

  function scheduleCloseSendSubmenu() {
    clearSendSubmenuCloseTimer();
    sendSubmenuCloseTimer = window.setTimeout(() => {
      showSendSubmenu.value = false;
      sendSubmenuCloseTimer = null;
    }, 120);
  }

  function closeSendSubmenu() {
    clearSendSubmenuCloseTimer();
    showSendSubmenu.value = false;
  }

  function clearSendSubmenuCloseTimer() {
    if (sendSubmenuCloseTimer === null) return;
    window.clearTimeout(sendSubmenuCloseTimer);
    sendSubmenuCloseTimer = null;
  }

  /** 发送当前右键条目到目标设备（`__all__` 哨兵 → target=null 广播全部在线）。 */
  async function sendClipTo(targetId: string) {
    const item = contextMenu.value?.item;
    fullClose();
    if (!item) return;
    try {
      await ipasteApi.deviceSendClip(
        targetId === SEND_TARGET_ALL ? null : targetId,
        clipSourceOf(item),
      );
    } catch (unknownError) {
      showError(unknownError);
    }
  }

  /** history 条目走 clips 表 id；分组条目走 category_items.id + 所属分组（后端附带分组名/颜色）。 */
  function clipSourceOf(item: ClipViewItem): LanClipSource {
    return item.collection === "history"
      ? { kind: "item", id: item.id }
      : { kind: "categoryItem", id: item.id, categoryId: item.categoryId };
  }

  function resetPendingDelete() {
    pendingDeleteContextKey.value = null;
  }

  function close() {
    contextMenu.value = null;
    pendingDeleteContextKey.value = null;
    pendingDeleteByKey.value = null;
    closeMoveSubmenu();
    closeSendSubmenu();
  }

  function fullClose() {
    if (options.onFullClose) {
      options.onFullClose();
      return;
    }
    close();
  }

  return {
    contextMenu,
    showMoveSubmenu,
    showSendSubmenu,
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
    sendClipTo,
    openMoveSubmenu,
    scheduleCloseMoveSubmenu,
    closeMoveSubmenu,
    clearMoveSubmenuCloseTimer,
    openSendSubmenu,
    scheduleCloseSendSubmenu,
    closeSendSubmenu,
    clearSendSubmenuCloseTimer,
    resetPendingDelete,
    close,
  };
}
