<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { AlertCircle, ClipboardCopy, Download, Inbox, Info, Pencil, Play, Trash2, Upload, Zap } from "lucide-vue-next";
import CategoryRail from "./components/CategoryRail.vue";
import ClipCard from "./components/ClipCard.vue";
import ClipContextMenu from "./components/ClipContextMenu.vue";
import AutomationCard from "./components/AutomationCard.vue";
import AutomationEditorDialog from "./components/AutomationEditorDialog.vue";
import AutomationConfirmDialog from "./components/AutomationConfirmDialog.vue";
import AutomationDetailPane from "./components/AutomationDetailPane.vue";
import ClipViewerWindow from "./components/ClipViewerWindow.vue";
import LanSyncPanel from "./components/LanSyncPanel.vue";
import QuickPreviewPanel from "./components/QuickPreviewPanel.vue";
import SettingsWindow from "./components/SettingsWindow.vue";
import TopBar from "./components/TopBar.vue";
import UpdateDialog from "./components/UpdateDialog.vue";
import { useUpdater } from "./composables/useUpdater";
import { useAppEvents } from "./composables/useAppEvents";
import { useAutomationFlow } from "./composables/useAutomationFlow";
import { useClipContextMenu } from "./composables/useClipContextMenu";
import { useQuickPreview } from "./composables/useQuickPreview";
import { t } from "./i18n";
import { contextItemKey, originalClipId } from "./lib/clipKeys";
import { isTauri } from "./lib/env";
import { categoryDisplayName, formatShortcut, typeLabel } from "./lib/format";
import { ipasteApi } from "./lib/ipasteApi";
import { useIpasteStore } from "./stores/ipasteStore";
import { IPASTE_EVENTS } from "./types/generated/events";
import type { AutomationAction, Category, CategoryItem, ClipViewItem } from "./types";

const store = useIpasteStore();
const updater = useUpdater();
const isSettingsWindow = new URLSearchParams(window.location.search).get("window") === "settings";
const isClipViewerWindow = new URLSearchParams(window.location.search).get("window") === "clip-viewer";
const isLanSyncWindow = new URLSearchParams(window.location.search).get("window") === "lan-sync";
const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const isPreservingCurrentApp = ref(false);
const categoryRailElement = ref<InstanceType<typeof CategoryRail> | null>(null);
const clipListElement = ref<HTMLElement | null>(null);
const editingClipKey = ref<string | null>(null);
const editingClipName = ref("");
const isClipListScrolling = ref(false);
const draggingItemKey = ref<string | null>(null);
const itemDropTargetKey = ref<string | null>(null);
const itemDropSide = ref<"before" | "after" | null>(null);
const itemDragOffset = ref({ x: 0, y: 0 });
let itemDragState: {
  key: string;
  id: string;
  startX: number;
  startY: number;
  width: number;
  height: number;
  hasMoved: boolean;
  targetKey: string | null;
  targetId: string | null;
  side: "before" | "after" | null;
} | null = null;
let unlistenShortcutOpened: UnlistenFn | null = null;
let unlistenPanelVisibilityChanged: UnlistenFn | null = null;
let clipListScrollTimer: number | null = null;
let selectionScrollFrame: number | null = null;
let searchReloadTimer: number | null = null;
let lastUpdateCheckAt = 0;
let suppressNextItemSelect = false;

const clipMenu = useClipContextMenu(store, { onStartRename: startEditingClipName });
const {
  contextMenu,
  editingCategoryId,
  showMoveSubmenu,
  pendingDeleteContextKey,
  pendingDeleteByKey,
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
} = clipMenu;

const quickPreview = useQuickPreview({
  visibleItems: () => store.visibleItems,
  isMenuOpen: () => Boolean(contextMenu.value),
  isEditing: () => editingClipKey.value !== null,
  isMacOs,
});
const {
  quickPreviewItem,
  clearQuickPreviewHover,
  hoverPreviewItem,
  clearHoveredPreviewItem,
  handleSelectionChange,
  handleQuickPreviewKeyup: handleKeyup,
  clearQuickPreviewTimer,
  isEditableTarget,
} = quickPreview;

const {
  automationEditorOpen,
  automationEditorAction,
  automationConfirmOpen,
  automationConfirmAction,
  automationContextMenu,
  automationDetailOpen,
  automationDetailAction,
  importFileInput,
  selectActionCard,
  runSelectedAction,
  executeAutomation,
  openAutomationEditor,
  saveAutomation,
  deleteAutomationAction,
  copyAutomationCommand,
  openAutomationContextMenu,
  closeAutomationContextMenu,
  openAutomationDetail,
  exportAllAutomations,
  triggerImport,
  onImportFileSelected,
  handleActionsKey,
  watchClosePanelRequest,
} = useAutomationFlow(store, hidePanelFromUi);

const categoryById = computed(() =>
  store.categories.reduce<Record<string, Category>>((categories, category) => {
    categories[category.id] = category;
    return categories;
  }, {}),
);

const categoriesByHash = computed(() =>
  store.categoryItems.reduce<Record<string, Category[]>>((groups, item) => {
    const category = categoryById.value[item.categoryId];
    if (!category) return groups;

    groups[item.contentHash] = [...(groups[item.contentHash] ?? []), category];
    return groups;
  }, {}),
);

const categoryItemCounts = computed(() =>
  store.categoryItems.reduce<Record<string, number>>((counts, item) => {
    counts[item.categoryId] = (counts[item.categoryId] ?? 0) + 1;
    return counts;
  }, {}),
);

const formattedShortcut = computed(() => `${formatShortcut("CommandOrControl+F")} ${t("shortcut.search")}`);
const isSideLayout = computed(() => store.panelLayout === "side");
const canReorderVisibleItems = computed(() =>
  store.selectedCategoryId !== "history" && !store.search.trim() && store.visibleItems.length > 1,
);

onMounted(async () => {
  if (isClipViewerWindow) return;
  if (isSettingsWindow) return;
  if (isLanSyncWindow) return;

  document.addEventListener("keydown", handleKeydown, true);
  document.addEventListener("keyup", handleKeyup, true);
  document.addEventListener("selectionchange", handleSelectionChange);
  window.addEventListener("blur", closeFloatingLayers);
  document.addEventListener("visibilitychange", handleVisibilityChange);

  await store.load();
  await store.loadAutomations();
  watchClosePanelRequest();
  await useAppEvents(store);
  if (isTauri) {
    scheduleSilentUpdateCheck();
  }
  if (isTauri) {
    unlistenShortcutOpened = await listen(IPASTE_EVENTS.shortcutOpened, closeFloatingLayers);
    unlistenPanelVisibilityChanged = await listen<{ visible: boolean; preservesCurrentApp: boolean; nativePanel?: boolean }>(
      IPASTE_EVENTS.panelVisibilityChanged,
      (event) => {
        applyPanelVisibility(event.payload);
      },
    );
  }
});

onUnmounted(() => {
  if (isClipViewerWindow) return;
  if (isSettingsWindow) return;
  if (isLanSyncWindow) return;

  document.removeEventListener("keydown", handleKeydown, true);
  document.removeEventListener("keyup", handleKeyup, true);
  document.removeEventListener("selectionchange", handleSelectionChange);
  window.removeEventListener("blur", closeFloatingLayers);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
  clearMoveSubmenuCloseTimer();
  clearClipListScrollTimer();
  clearSelectionScrollFrame();
  clearSearchReloadTimer();
  clearQuickPreviewTimer();
  cleanupItemDrag();
  unlistenShortcutOpened?.();
  unlistenPanelVisibilityChanged?.();
  unlistenShortcutOpened = null;
  unlistenPanelVisibilityChanged = null;
  document.body.classList.remove("ipaste-preserve-current-app");
});

watch(
  () => store.search,
  () => {
    store.clampSelection();
    if (store.selectedCategoryId === "history") {
      scheduleSearchReload();
    }
  },
);

watch(
  () => [store.selectedIndex, store.selectedCategoryId, store.search],
  () => scheduleSelectedClipScroll(),
  { flush: "post" },
);

watch(isPreservingCurrentApp, (preservesCurrentApp) => {
  document.body.classList.toggle("ipaste-preserve-current-app", preservesCurrentApp);
});

function applyPanelVisibility(
  payload: { visible: boolean; preservesCurrentApp: boolean; nativePanel?: boolean },
  activateDefault = false,
) {
  closeFloatingLayers();
  const nativePanel = payload.visible && Boolean(payload.nativePanel);
  isPreservingCurrentApp.value = payload.visible && payload.preservesCurrentApp && !nativePanel;
  if (!payload.visible) {
    store.clearSearch();
    resetClipListScroll();
    blurActiveElement();
    return;
  }

  if (activateDefault) {
    store.activatePanelDefault();
  }
  if (!nativePanel) {
    scheduleActiveElementBlur();
  }
  blurCategoryFocus();
  scheduleSilentUpdateCheck();
}

async function createCategory() {
  const category = await store.createCategory(t("category.newCategory"));
  editingCategoryId.value = category.id;
}

async function renameCategory(category: Category, name: string) {
  if (!name || name === category.name) return;
  await store.renameCategory(category, name);
}

async function updateCategoryColor(category: Category, color: string) {
  await store.updateCategoryColor(category, color);
}

async function editCategory(id: string) {
  editingCategoryId.value = id;
}

function finishEditingCategory() {
  editingCategoryId.value = null;
}

async function deleteCategory(id: string) {
  await store.deleteCategory(id);
}

async function reorderCategories(categoryIds: string[]) {
  await store.reorderCategories(categoryIds);
}

function itemCategoryTags(item: ClipViewItem) {
  if (item.collection === "history") return categoriesByHash.value[item.contentHash] ?? [];

  const categoryId = "categoryId" in item ? item.categoryId : store.selectedCategoryId;
  const category = categoryById.value[categoryId];
  return category ? [category] : [];
}

function toCategoryClipViewItem(item: CategoryItem): ClipViewItem {
  return { ...item, collection: "category" };
}

async function applyFallbackItem(item: ClipViewItem) {
  await store.applyItem(item);
}

function startItemDrag(payload: { item: ClipViewItem; index: number; event: PointerEvent }) {
  if (!canReorderVisibleItems.value || payload.item.collection !== "category" || payload.event.button !== 0) {
    payload.event.preventDefault();
    return;
  }

  payload.event.preventDefault();
  const key = contextItemKey(payload.item);
  const dragSource = (payload.event.currentTarget ?? payload.event.target) as Element | null;
  const card = dragSource?.closest<HTMLElement>("[data-item-key]");
  const rect = card?.getBoundingClientRect();
  pendingDeleteContextKey.value = null;
  closeMoveSubmenu();
  itemDragState = {
    key,
    id: payload.item.id,
    startX: payload.event.clientX,
    startY: payload.event.clientY,
    width: rect?.width ?? 0,
    height: rect?.height ?? 0,
    hasMoved: false,
    targetKey: null,
    targetId: null,
    side: null,
  };
  itemDragOffset.value = { x: 0, y: 0 };
  window.addEventListener("pointermove", handleItemPointerMove);
  window.addEventListener("pointerup", finishItemDrag);
  window.addEventListener("pointercancel", cancelItemDrag);
}

function handleItemPointerMove(event: PointerEvent) {
  const state = itemDragState;
  if (!state || !canReorderVisibleItems.value) return;

  event.preventDefault();
  if (Math.hypot(event.clientX - state.startX, event.clientY - state.startY) > 3) {
    if (!state.hasMoved) {
      draggingItemKey.value = state.key;
    }
    state.hasMoved = true;
  }
  if (!state.hasMoved) return;

  itemDragOffset.value = {
    x: event.clientX - state.startX,
    y: event.clientY - state.startY,
  };

  const target = itemTargetFromPoint(event.clientX, event.clientY);
  if (!target || target.key === state.key) {
    state.targetKey = null;
    state.targetId = null;
    state.side = null;
    itemDropTargetKey.value = null;
    itemDropSide.value = null;
    return;
  }

  const side = event.clientY < target.rect.top + target.rect.height / 2 ? "before" : "after";
  state.targetKey = target.key;
  state.targetId = target.id;
  state.side = side;
  itemDropTargetKey.value = target.key;
  itemDropSide.value = side;
  scrollItemsNearPointer(event.clientY);
  showClipListScrollbar();
}

async function finishItemDrag(event?: PointerEvent) {
  event?.preventDefault();

  const state = itemDragState;
  if (state?.hasMoved) {
    suppressNextItemSelect = true;
    window.setTimeout(() => {
      suppressNextItemSelect = false;
    }, 0);
  }
  cleanupItemDrag();
  if (!state?.hasMoved || !state.targetKey || !state.targetId || !state.side || state.key === state.targetKey) return;
  const currentItems = store.visibleItems.filter((item) => item.collection === "category");
  const draggedItem = currentItems.find((item) => contextItemKey(item) === state.key);
  if (!draggedItem) return;

  const nextIds = currentItems
    .filter((item) => contextItemKey(item) !== state.key)
    .map((item) => item.id);
  const targetIndex = nextIds.indexOf(state.targetId);
  if (targetIndex < 0) return;

  nextIds.splice(state.side === "after" ? targetIndex + 1 : targetIndex, 0, draggedItem.id);
  const currentIds = currentItems.map((item) => item.id);
  if (nextIds.join("\n") === currentIds.join("\n")) return;

  await store.reorderCategoryItems(store.selectedCategoryId, nextIds);
}

function cancelItemDrag() {
  cleanupItemDrag();
}

function cleanupItemDrag() {
  window.removeEventListener("pointermove", handleItemPointerMove);
  window.removeEventListener("pointerup", finishItemDrag);
  window.removeEventListener("pointercancel", cancelItemDrag);
  itemDragState = null;
  draggingItemKey.value = null;
  itemDropTargetKey.value = null;
  itemDropSide.value = null;
  itemDragOffset.value = { x: 0, y: 0 };
}

function itemTargetFromPoint(clientX: number, clientY: number) {
  const element = document.elementFromPoint(clientX, clientY);
  const card = element instanceof Element ? element.closest<HTMLElement>("[data-item-key]") : null;
  if (!card || !clipListElement.value?.contains(card)) return null;

  const key = card.dataset.itemKey;
  const id = card.dataset.itemId;
  if (!key || !id) return null;
  return {
    key,
    id,
    rect: card.getBoundingClientRect(),
  };
}

function scrollItemsNearPointer(clientY: number) {
  const list = clipListElement.value;
  if (!list) return;

  const rect = list.getBoundingClientRect();
  const edge = 48;
  if (clientY < rect.top + edge) {
    list.scrollTop -= 14;
  } else if (clientY > rect.bottom - edge) {
    list.scrollTop += 14;
  }
}

function itemDragStyle(item: ClipViewItem) {
  if (draggingItemKey.value !== contextItemKey(item)) return undefined;
  const state = itemDragState;
  return {
    transform: `translate(${itemDragOffset.value.x}px, ${itemDragOffset.value.y}px)`,
    width: state?.width ? `${state.width}px` : undefined,
    height: state?.height ? `${state.height}px` : undefined,
  };
}

function selectClipCard(index: number) {
  if (suppressNextItemSelect) {
    suppressNextItemSelect = false;
    return;
  }

  pendingDeleteByKey.value = null;
  store.setSelectedIndex(index);
}

async function startEditingClipName(item: ClipViewItem) {
  const index = store.visibleItems.findIndex((visibleItem) => contextItemKey(visibleItem) === contextItemKey(item));
  if (index >= 0) {
    store.setSelectedIndex(index);
  }

  editingClipKey.value = contextItemKey(item);
  editingClipName.value = item.displayName?.trim() || typeLabel(item.clipType);
  await focusEditingClipName();
}

function updateEditingClipName(value: string) {
  editingClipName.value = value;
}

async function commitEditingClipName(item: ClipViewItem) {
  if (editingClipKey.value !== contextItemKey(item)) return;

  const name = editingClipName.value.trim();
  editingClipKey.value = null;
  editingClipName.value = "";
  await store.renameClip(item, name || null);
}

function cancelEditingClipName() {
  editingClipKey.value = null;
  editingClipName.value = "";
}

async function openClipViewer(item: ClipViewItem) {
  await ipasteApi.openClipViewer(item, originalClipId(item));
}

function handleKeydown(event: KeyboardEvent) {
  if (event.defaultPrevented) return;

  if (event.key !== "Backspace") {
    pendingDeleteByKey.value = null;
  }

  if (quickPreview.handleQuickPreviewKeydown(event)) return;

  if (handleCategoryShortcut(event)) return;

  if (event.key === "Tab") {
    event.preventDefault();
    return;
  }

  if (isEditableTarget(event.target)) return;

  if (contextMenu.value) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeFloatingLayers();
    }
    return;
  }

  if (event.key === "Backspace") {
    event.preventDefault();
    const item = store.visibleItems[store.selectedIndex];
    if (!item) return;

    const key = contextItemKey(item);
    if (pendingDeleteByKey.value === key) {
      void deleteSelectedItem(item);
      return;
    }

    pendingDeleteByKey.value = key;
    return;
  }

  if (handlePanelKey(event.key)) {
    event.preventDefault();
    return;
  }

  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
    event.preventDefault();
    focusSearch();
  }
}

function handlePanelKey(key: string) {
  if (store.selectedCategoryId === "actions") {
    return handleActionsKey(key);
  }
  if (contextMenu.value) {
    if (key === "Escape") {
      closeFloatingLayers();
      return true;
    }
    return false;
  }

  if (key === "ArrowDown") {
    store.moveSelection(2);
    return true;
  }

  if (key === "ArrowUp") {
    store.moveSelection(-2);
    return true;
  }

  if (key === "ArrowRight") {
    store.moveSelection(1);
    return true;
  }

  if (key === "ArrowLeft") {
    store.moveSelection(-1);
    return true;
  }

  if (key === "Enter") {
    void store.applySelected();
    return true;
  }

  if (key === "Escape") {
    void hidePanelFromUi();
    return true;
  }

  return false;
}

function handleCategoryShortcut(event: KeyboardEvent) {
  if (!(event.metaKey || event.ctrlKey) || event.altKey || !/^[1-9]$/.test(event.key)) {
    return false;
  }

  event.preventDefault();

  const categoryIds = ["history", ...store.categories.map((category) => category.id)];
  const targetCategoryId = categoryIds[Number(event.key) - 1];
  if (!targetCategoryId) return true;

  closeFloatingLayers();
  finishEditingCategory();
  if (targetCategoryId !== store.selectedCategoryId) {
    store.selectCategory(targetCategoryId);
  }
  return true;
}

function focusSearch() {
  const input = document.querySelector<HTMLInputElement>(".search-box input");
  input?.focus();
  input?.select();
}

async function hidePanelFromUi() {
  blurActiveElement();
  await store.hidePanel();
}

function scheduleActiveElementBlur() {
  void nextTick(() => {
    window.requestAnimationFrame(blurActiveElement);
  });
}

function blurActiveElement() {
  const activeElement = document.activeElement;
  if (activeElement instanceof HTMLElement && activeElement !== document.body) {
    activeElement.blur();
  }
}

function blurCategoryFocus() {
  window.requestAnimationFrame(() => {
    const activeElement = document.activeElement;
    if (activeElement instanceof HTMLElement && activeElement.closest(".tag-strip")) {
      activeElement.blur();
    }
  });
}

function closeFloatingLayers() {
  clipMenu.close();
  quickPreview.resetQuickPreviewState();
  categoryRailElement.value?.closeFloatingLayers();
}

function handleVisibilityChange() {
  if (document.hidden) {
    closeFloatingLayers();
  } else {
    scheduleSilentUpdateCheck();
  }
}

function scheduleSilentUpdateCheck() {
  if (!isTauri) return;

  const now = Date.now();
  if (now - lastUpdateCheckAt < 30 * 60 * 1000) return;
  lastUpdateCheckAt = now;
  void updater.checkForUpdate({ silent: true });
}

async function focusEditingClipName() {
  await nextTick();
  window.setTimeout(() => {
    const input = document.querySelector<HTMLInputElement>(".clip-title-input");
    input?.focus();
    input?.select();
  }, 40);
}

function showClipListScrollbar() {
  clearClipListScrollTimer();
  isClipListScrolling.value = true;
  clipListScrollTimer = window.setTimeout(() => {
    isClipListScrolling.value = false;
    clipListScrollTimer = null;
  }, 780);
}

function handleClipListScroll() {
  showClipListScrollbar();

  const list = clipListElement.value;
  if (!list || store.selectedCategoryId !== "history" || !store.hasMoreClips) return;

  const distanceToBottom = list.scrollHeight - list.scrollTop - list.clientHeight;
  if (distanceToBottom < 160) {
    void store.loadMoreClips();
  }
}

function clearClipListScrollTimer() {
  if (clipListScrollTimer === null) return;
  window.clearTimeout(clipListScrollTimer);
  clipListScrollTimer = null;
}

function resetClipListScroll() {
  clearClipListScrollTimer();
  isClipListScrolling.value = false;

  if (clipListElement.value) {
    clipListElement.value.scrollTop = 0;
  }
}

function scheduleSelectedClipScroll() {
  clearSelectionScrollFrame();
  selectionScrollFrame = window.requestAnimationFrame(() => {
    selectionScrollFrame = null;
    scrollSelectedClipIntoView();
  });
}

function clearSelectionScrollFrame() {
  if (selectionScrollFrame === null) return;
  window.cancelAnimationFrame(selectionScrollFrame);
  selectionScrollFrame = null;
}

function scheduleSearchReload() {
  clearSearchReloadTimer();
  searchReloadTimer = window.setTimeout(() => {
    searchReloadTimer = null;
    void store.reloadClips();
  }, 160);
}

function clearSearchReloadTimer() {
  if (searchReloadTimer === null) return;
  window.clearTimeout(searchReloadTimer);
  searchReloadTimer = null;
}

function scrollSelectedClipIntoView() {
  const list = clipListElement.value;
  const selectedCard = list?.querySelector<HTMLElement>(".clip-card-selected");
  if (!list || !selectedCard) return;

  const listRect = list.getBoundingClientRect();
  const cardRect = selectedCard.getBoundingClientRect();
  const edgePadding = 16;
  const visibleTop = listRect.top + edgePadding;
  const visibleBottom = listRect.bottom - edgePadding;

  if (cardRect.top < visibleTop) {
    list.scrollBy({ top: cardRect.top - visibleTop, behavior: "auto" });
    showClipListScrollbar();
    return;
  }

  if (cardRect.bottom > visibleBottom) {
    list.scrollBy({ top: cardRect.bottom - visibleBottom, behavior: "auto" });
    showClipListScrollbar();
  }
}
</script>

<template>
  <SettingsWindow v-if="isSettingsWindow" />
  <ClipViewerWindow v-else-if="isClipViewerWindow" />
  <LanSyncPanel v-else-if="isLanSyncWindow" />

  <main
    v-else
    class="app-shell"
    :class="{ 'app-shell-preserve-current-app': isPreservingCurrentApp }"
    @click="closeFloatingLayers"
  >
    <section class="flex min-w-0 flex-1 flex-col">
      <div class="relative">
        <TopBar
          v-model="store.search"
          :shortcut="formattedShortcut"
          :settings-open="false"
          :append-copy-enabled="store.isAppendCopyEnabled"
          :append-copy-timeout-minutes="store.appendCopyTimeoutMinutes"
          :has-update="updater.hasAvailableUpdate.value"
          @toggle-settings="store.showSettings"
          @toggle-append-copy="store.toggleAppendCopy"
          @open-update="updater.openUpdateDialog"
          @close="hidePanelFromUi"
        />
      </div>

      <UpdateDialog
        :open="updater.updateDialogOpen.value"
        :status="updater.updateStatus.value"
        :update="updater.availableUpdate.value"
        :error="updater.updateError.value"
        :error-phase="updater.updateErrorPhase.value"
        :downloaded-bytes="updater.updateDownloadedBytes.value"
        :total-bytes="updater.updateTotalBytes.value"
        @dismiss="updater.dismissUpdateDialog"
        @install="updater.installAvailableUpdate"
        @relaunch="updater.relaunchForUpdate"
      />

      <section
        class="main-content"
        :class="{ 'main-content-side': isSideLayout }"
      >
        <CategoryRail
          ref="categoryRailElement"
          :categories="store.categories"
          :selected-category-id="store.selectedCategoryId"
          :editing-category-id="editingCategoryId"
          :history-count="store.clipTotalCount"
          :category-counts="categoryItemCounts"
          :orientation="isSideLayout ? 'vertical' : 'horizontal'"
          @select="store.selectCategory"
          @create="createCategory"
          @edit="editCategory"
          @rename="renameCategory"
          @recolor="updateCategoryColor"
          @finish-editing="finishEditingCategory"
          @delete="deleteCategory"
          @reorder="reorderCategories"
        />

        <section class="clip-area">
          <div
            v-if="store.error"
            class="mx-4 mt-4 flex items-center gap-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700"
          >
            <AlertCircle class="size-4" />
            <span class="min-w-0 flex-1 truncate">{{ store.error }}</span>
          </div>

          <div
            ref="clipListElement"
            class="clip-list-scroll subtle-scrollbar min-h-0 flex-1 overflow-y-auto p-4"
            :class="{
              'subtle-scrollbar-active': isClipListScrolling,
              'clip-list-scroll-previewing': quickPreviewItem,
            }"
            @scroll="handleClipListScroll"
            @pointerleave="clearQuickPreviewHover"
          >
            <div
              v-if="store.isLoading"
              class="clip-card-grid"
            >
              <div
                v-for="index in 9"
                :key="index"
                class="h-40 animate-pulse rounded-lg border border-slate-200 bg-white"
              />
            </div>

            <div
              v-else-if="store.selectedCategoryId === 'actions'"
              class="clip-card-grid"
            >
              <AutomationCard
                v-for="(action, index) in store.visibleActions"
                :key="action.id"
                :action="action"
                :selected="store.selectedActionIndex === index"
                @click="selectActionCard(index)"
                @run="runSelectedAction(action)"
                @edit="openAutomationEditor(action)"
                @delete="deleteAutomationAction(action)"
                @copy="copyAutomationCommand(action)"
                @open-context-menu="openAutomationContextMenu(action, $event)"
              />
              <div
                v-if="!store.visibleActions.length"
                class="flex h-full min-h-[360px] flex-col items-center justify-center rounded-lg border border-dashed border-slate-300 bg-white/70 text-center"
              >
                <Zap class="size-10 text-slate-300" />
                <h2 class="mt-3 text-base font-semibold text-slate-900">
                  {{ t("automation.entry") }}
                </h2>
                <p class="mt-1 max-w-sm text-sm text-slate-500">
                  {{ t("automation.noActions") }}
                </p>
                <button
                  type="button"
                  class="mt-4 rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white hover:bg-slate-700"
                  @click="openAutomationEditor(null)"
                >
                  {{ t("automation.newAction") }}
                </button>
              </div>
            </div>

            <div
              v-else-if="store.fallbackGroups.length"
              class="fallback-groups"
            >
              <section
                v-for="group in store.fallbackGroups"
                :key="group.category.id"
                class="fallback-group"
              >
                <header class="fallback-group-header">
                  <span
                    class="fallback-group-dot"
                    :style="{ backgroundColor: group.category.color }"
                  />
                  <span class="fallback-group-name truncate">{{ categoryDisplayName(group.category.name) }}</span>
                  <span class="fallback-group-count">{{ group.items.length }}</span>
                </header>
                <div class="fallback-group-items clip-card-grid">
                  <div
                    v-for="item in group.items"
                    :key="item.id"
                    class="fallback-item"
                  >
                    <ClipCard
                      :item="toCategoryClipViewItem(item)"
                      :index="0"
                      :selected="false"
                      :category-tags="[]"
                      :editing-name="null"
                      :reorder-enabled="false"
                      @apply="applyFallbackItem"
                      @expand="openClipViewer"
                      @open-context-menu="openFallbackContextMenu"
                    />
                    <span class="source-tag">
                      {{ t("search.fromCategory", { name: categoryDisplayName(group.category.name) }) }}
                    </span>
                  </div>
                </div>
              </section>
            </div>

            <div
              v-else-if="store.visibleItems.length"
              class="clip-card-grid"
            >
              <ClipCard
                v-for="(item, index) in store.visibleItems"
                :key="`${item.collection}-${item.id}`"
                :item="item"
                :index="index"
                :data-item-key="contextItemKey(item)"
                :data-item-id="item.id"
                :selected="store.selectedIndex === index"
                :category-tags="itemCategoryTags(item)"
                :editing-name="editingClipKey === contextItemKey(item) ? editingClipName : null"
                :reorder-enabled="canReorderVisibleItems && item.collection === 'category'"
                :delete-confirming="pendingDeleteByKey === contextItemKey(item)"
                :style="itemDragStyle(item)"
                :class="{
                  'clip-card-dragging': draggingItemKey === contextItemKey(item),
                  'clip-card-drop-before': itemDropTargetKey === contextItemKey(item) && itemDropSide === 'before',
                  'clip-card-drop-after': itemDropTargetKey === contextItemKey(item) && itemDropSide === 'after',
                  'clip-card-delete-confirming': pendingDeleteByKey === contextItemKey(item),
                }"
                @select="selectClipCard"
                @apply="store.applyItem"
                @expand="openClipViewer"
                @open-context-menu="openClipContextMenu"
                @update-editing-name="updateEditingClipName"
                @commit-rename="commitEditingClipName"
                @cancel-rename="cancelEditingClipName"
                @reorder-pointer-down="startItemDrag"
                @pointerenter="hoverPreviewItem(item)"
                @pointerleave="clearHoveredPreviewItem(item)"
              />
              <div
                v-if="store.selectedCategoryId === 'history' && store.isLoadingMoreClips"
                class="clip-grid-full h-24 animate-pulse rounded-lg border border-slate-200 bg-white"
              />
            </div>

            <div
              v-else
              class="flex h-full min-h-[360px] flex-col items-center justify-center rounded-lg border border-dashed border-slate-300 bg-white/70 text-center"
            >
              <Inbox class="size-10 text-slate-300" />
              <h2 class="mt-3 text-base font-semibold text-slate-900">
                {{ t("empty.title") }}
              </h2>
              <p class="mt-1 max-w-sm text-sm text-slate-500">
                {{ t("empty.description") }}
              </p>
            </div>
          </div>

          <QuickPreviewPanel
            v-if="quickPreview.quickPreviewItem.value"
            :item="quickPreview.quickPreviewItem.value"
            :title="quickPreview.quickPreviewTitle.value"
            :label="quickPreview.quickPreviewAriaLabel.value"
            :time="quickPreview.quickPreviewTime.value"
            :size="quickPreview.quickPreviewSize.value"
            :content="quickPreview.quickPreviewContent.value"
            :image-src="quickPreview.quickPreviewImageSrc.value"
            :color-value="quickPreview.quickPreviewColorValue.value"
            :locked="quickPreview.isQuickPreviewLocked.value"
            :selected-text="quickPreview.quickPreviewSelectedText.value"
            @lock="quickPreview.lockQuickPreview"
            @copy="quickPreview.copyQuickPreviewItem"
            @paste="quickPreview.pasteQuickPreviewSelection"
            @close="quickPreview.closeQuickPreview"
          />
        </section>
      </section>
    </section>

    <ClipContextMenu
      v-if="contextMenu"
      :context-menu="contextMenu"
      :categories="store.categories"
      :delete-label="contextDeleteLabel(contextMenu.item)"
      :delete-confirming="pendingDeleteContextKey === contextItemKey(contextMenu.item)"
      :show-move-submenu="showMoveSubmenu"
      @paste="pasteContextItem"
      @copy="copyContextItem"
      @rename="renameContextItem"
      @move-to="addContextItemToCategory"
      @create-category="createCategoryForContextItem"
      @delete="deleteContextItem"
      @open-move-submenu="openMoveSubmenu"
      @schedule-close-move-submenu="scheduleCloseMoveSubmenu"
      @reset-pending-delete="resetPendingDelete"
    />

    <AutomationEditorDialog
      :open="automationEditorOpen"
      :action="automationEditorAction"
      @save="saveAutomation"
      @cancel="automationEditorOpen = false"
    />

    <AutomationConfirmDialog
      :open="automationConfirmOpen"
      :action="automationConfirmAction"
      @confirm="void executeAutomation(automationConfirmAction as AutomationAction); automationConfirmOpen = false"
      @cancel="automationConfirmOpen = false"
    />

    <Teleport to="body">
      <div
        v-if="automationDetailOpen && automationDetailAction"
        class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40"
        @click.self="automationDetailOpen = false"
      >
        <div class="flex h-[80vh] w-[520px] max-w-[90vw] flex-col overflow-hidden rounded-xl bg-white shadow-xl">
          <AutomationDetailPane
            :action="automationDetailAction"
            @run="runSelectedAction(automationDetailAction); automationDetailOpen = false"
          />
          <div class="flex justify-end border-t border-slate-200 px-4 py-2">
            <button
              type="button"
              class="rounded-lg px-3 py-1 text-sm text-slate-600 hover:bg-slate-100"
              @click="automationDetailOpen = false"
            >
              {{ t("common.cancel") }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>

    <div
      v-if="automationContextMenu"
      class="clip-context-menu"
      :style="{ left: `${automationContextMenu.x}px`, top: `${automationContextMenu.y}px` }"
      @click.stop
      @contextmenu.prevent.stop
    >
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="runSelectedAction(automationContextMenu.action); closeAutomationContextMenu()"
      >
        <Play class="size-3.5" /> {{ t("automation.run") }}
      </button>
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="openAutomationEditor(automationContextMenu.action); closeAutomationContextMenu()"
      >
        <Pencil class="size-3.5" /> {{ t("automation.edit") }}
      </button>
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="copyAutomationCommand(automationContextMenu.action); closeAutomationContextMenu()"
      >
        <ClipboardCopy class="size-3.5" /> {{ t("automation.copy") }}
      </button>
      <div class="context-menu-separator" />
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="openAutomationDetail(automationContextMenu.action); closeAutomationContextMenu()"
      >
        <Info class="size-3.5" /> {{ t("automation.detailStatus") }}
      </button>
      <div class="context-menu-separator" />
      <button
        type="button"
        class="context-menu-item context-menu-item-strong"
        tabindex="-1"
        role="menuitem"
        @click="deleteAutomationAction(automationContextMenu.action); closeAutomationContextMenu()"
      >
        <Trash2 class="size-3.5" /> {{ t("automation.delete") }}
      </button>
      <div class="context-menu-separator" />
      <button type="button" class="context-menu-item" tabindex="-1" role="menuitem" @click="triggerImport">
        <Upload class="size-3.5" /> {{ t("automation.importAction") }}
      </button>
      <button type="button" class="context-menu-item" tabindex="-1" role="menuitem" @click="exportAllAutomations">
        <Download class="size-3.5" /> {{ t("automation.exportAll") }}
      </button>
    </div>

    <input
      ref="importFileInput"
      type="file"
      accept=".json"
      class="hidden"
      @change="onImportFileSelected"
    />
  </main>
</template>
