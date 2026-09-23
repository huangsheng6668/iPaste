<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { AlertCircle } from "lucide-vue-next";
import ClipContextMenu from "./components/ClipContextMenu.vue";
import AutomationContextMenu from "./components/AutomationContextMenu.vue";
import AutomationDetailDialog from "./components/AutomationDetailDialog.vue";
import AutomationEditorDialog from "./components/AutomationEditorDialog.vue";
import AutomationConfirmDialog from "./components/AutomationConfirmDialog.vue";
import ClipViewerWindow from "./components/ClipViewerWindow.vue";
import OcrOverlayWindow from "./components/OcrOverlayWindow.vue";
import OcrResultWindow from "./components/OcrResultWindow.vue";
import ErrorToast from "./components/ErrorToast.vue";
import LanSyncPanel from "./components/LanSyncPanel.vue";
import SettingsWindow from "./components/SettingsWindow.vue";
import CommandSearchBar from "./components/CommandSearchBar.vue";
import ClipListPane from "./components/ClipListPane.vue";
import ClipInspectorPane from "./components/ClipInspectorPane.vue";
import KeyboardActionBar from "./components/KeyboardActionBar.vue";
import UpdateDialog from "./components/UpdateDialog.vue";
import { useUpdater } from "./composables/useUpdater";
import { useAppEvents } from "./composables/useAppEvents";
import { useAutomationFlow } from "./composables/useAutomationFlow";
import { useClipContextMenu } from "./composables/useClipContextMenu";
import { useClipListScroll } from "./composables/useClipListScroll";
import { useDragSort } from "./composables/useDragSort";
import { useInlineRename } from "./composables/useInlineRename";
import { usePanelKeyboard } from "./composables/usePanelKeyboard";
import { useQuickPreview } from "./composables/useQuickPreview";
import { t } from "./i18n";
import { contextItemKey, originalClipId } from "./lib/clipKeys";
import { isMacOs, isTauri } from "./lib/env";
import { categoryDisplayName, formatShortcut } from "./lib/format";
import { ipasteApi } from "./lib/ipasteApi";
import { useIpasteStore } from "./stores/ipasteStore";
import { IPASTE_EVENTS } from "./types/generated/events";
import type { AutomationAction, Category, CategoryItem, ClipViewItem } from "./types";

const store = useIpasteStore();
const updater = useUpdater();
const isSettingsWindow = new URLSearchParams(window.location.search).get("window") === "settings";
const isClipViewerWindow = new URLSearchParams(window.location.search).get("window") === "clip-viewer";
const isLanSyncWindow = new URLSearchParams(window.location.search).get("window") === "lan-sync";
const isOcrOverlayWindow = new URLSearchParams(window.location.search).get("window") === "ocr-overlay";
const isOcrResultWindow = new URLSearchParams(window.location.search).get("window") === "ocr-result";
// 主窗口 = 下方所有辅助窗口路由都不匹配时的默认渲染分支（template v-else）。
const isMainWindow =
  !isSettingsWindow && !isClipViewerWindow && !isLanSyncWindow && !isOcrOverlayWindow && !isOcrResultWindow;
const isPreservingCurrentApp = ref(false);
let unlistenShortcutOpened: UnlistenFn | null = null;
let unlistenPanelVisibilityChanged: UnlistenFn | null = null;
let lastUpdateCheckAt = 0;
let suppressNextItemSelect = false;

// 行内重命名：选中同步与落库路径（store.renameClip）由 App 注入。
const inlineRename = useInlineRename({
  onBegin: (item) => {
    const index = store.visibleItems.findIndex((visibleItem) => contextItemKey(visibleItem) === contextItemKey(item));
    if (index >= 0) {
      store.setSelectedIndex(index);
    }
  },
  onCommit: (item, name) => store.renameClip(item, name),
});
const {
  renamingKey: editingClipKey,
  renameValue: editingClipName,
  begin: startEditingClipName,
  commit: commitEditingClipName,
  cancel: cancelEditingClipName,
} = inlineRename;

const clipMenu = useClipContextMenu(store, {
  onStartRename: startEditingClipName,
  onFullClose: closeFloatingLayers,
});
const {
  contextMenu,
  editingCategoryId,
  showMoveSubmenu,
  showSendSubmenu,
  sendTargetList,
  pendingDeleteContextKey,
  pendingDeleteByKey,
  contextDeleteLabel,
  openClipContextMenu,
  pasteContextItem,
  copyContextItem,
  renameContextItem,
  addContextItemToCategory,
  deleteContextItem,
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
} = clipMenu;

const quickPreview = useQuickPreview({
  visibleItems: () => store.visibleItems,
  isMenuOpen: () => Boolean(contextMenu.value),
  isEditing: () => editingClipKey.value !== null,
  isMacOs,
});
const {
  hoverPreviewItem,
  clearHoveredPreviewItem,
  handleSelectionChange,
  clearQuickPreviewTimer,
} = quickPreview;

const automationFlow = useAutomationFlow(store, hidePanelFromUi);
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
  watchClosePanelRequest,
} = automationFlow;

const panelKeyboard = usePanelKeyboard({
  store,
  quickPreview,
  clipMenu,
  automationFlow,
  closeFloatingLayers,
  hidePanelFromUi,
  finishEditingCategory,
  openClipViewer,
  isModalOpen: () =>
    automationEditorOpen.value ||
    automationConfirmOpen.value ||
    automationDetailOpen.value ||
    updater.updateDialogOpen.value,
  isEditingName: () => editingClipKey.value !== null || editingCategoryId.value !== null,
});
const { handleKeydown, handleKeyup } = panelKeyboard;

const {
  clipListElement,
  handleClipListScroll,
  showClipListScrollbar,
  resetClipListScroll,
  setupWatches,
  cleanup: cleanupClipListScroll,
} = useClipListScroll({ store });

// ClipListPane 的根节点才是滚动容器，函数 ref 经 prop 转发拿到它。
function setClipListElement(el: unknown) {
  clipListElement.value = el instanceof HTMLElement ? el : null;
}

const itemDrag = useDragSort<ClipViewItem>({
  canStart: ({ item, event }) =>
    canReorderVisibleItems.value && item.collection === "category" && event.button === 0,
  items: () => store.visibleItems.filter((item) => item.collection === "category"),
  itemKey: contextItemKey,
  itemId: (item) => item.id,
  targetFromPoint: itemTargetFromPoint,
  onReorder: (orderedIds) => store.reorderCategoryItems(store.selectedCategoryId, orderedIds),
  container: () => clipListElement.value,
  orientation: "vertical",
  isActive: () => canReorderVisibleItems.value,
  onDragStarted: () => {
    pendingDeleteContextKey.value = null;
    closeMoveSubmenu();
    closeSendSubmenu();
  },
  onDragFinished: () => {
    suppressNextItemSelect = true;
    window.setTimeout(() => {
      suppressNextItemSelect = false;
    }, 0);
  },
  onEdgeScroll: () => showClipListScrollbar(),
});
const {
  draggingKey: draggingItemKey,
  dropTargetKey: itemDropTargetKey,
  dropSide: itemDropSide,
  dragStyle: itemDragStyle,
  cleanup: cleanupItemDrag,
} = itemDrag;

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
const canReorderVisibleItems = computed(() =>
  store.selectedCategoryId !== "history" && !store.search.trim() && store.visibleItems.length > 1,
);

onMounted(async () => {
  if (!isMainWindow) return;

  document.addEventListener("keydown", handleKeydown, true);
  document.addEventListener("keyup", handleKeyup, true);
  document.addEventListener("selectionchange", handleSelectionChange);
  window.addEventListener("blur", closeFloatingLayers);
  document.addEventListener("visibilitychange", handleVisibilityChange);
  setupWatches();

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
  if (!isMainWindow) return;

  document.removeEventListener("keydown", handleKeydown, true);
  document.removeEventListener("keyup", handleKeyup, true);
  document.removeEventListener("selectionchange", handleSelectionChange);
  window.removeEventListener("blur", closeFloatingLayers);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
  clearMoveSubmenuCloseTimer();
  clearSendSubmenuCloseTimer();
  cleanupClipListScroll();
  clearQuickPreviewTimer();
  cleanupItemDrag();
  unlistenShortcutOpened?.();
  unlistenPanelVisibilityChanged?.();
  unlistenShortcutOpened = null;
  unlistenPanelVisibilityChanged = null;
  document.body.classList.remove("ipaste-preserve-current-app");
});

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

function itemCategoryTags(item: ClipViewItem) {
  if (item.collection === "history") return categoriesByHash.value[item.contentHash] ?? [];

  const categoryId = "categoryId" in item ? item.categoryId : store.selectedCategoryId;
  const category = categoryById.value[categoryId];
  return category ? [category] : [];
}

function toCategoryClipViewItem(item: CategoryItem): ClipViewItem {
  return { ...item, collection: "category" };
}

function startItemDrag(payload: { item: ClipViewItem; index: number; event: PointerEvent }) {
  payload.event.preventDefault();
  itemDrag.start(payload);
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

function selectClipCard(index: number) {
  if (suppressNextItemSelect) {
    suppressNextItemSelect = false;
    return;
  }

  pendingDeleteByKey.value = null;
  store.setSelectedIndex(index);
}

async function openClipViewer(item: ClipViewItem, autoRecognize = false) {
  await ipasteApi.openClipViewer(item, originalClipId(item), autoRecognize);
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
}

// 输入框保留原生右键（剪切/复制/粘贴），空白区域屏蔽 WebView 默认菜单
function suppressDefaultContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (target?.closest("input, textarea, [contenteditable='true']")) return;
  if (event.defaultPrevented) return;
  event.preventDefault();
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

const selectedClipItem = computed(() => store.visibleItems[store.selectedIndex] ?? null);
const selectedAutomationAction = computed(() => store.visibleActions[store.selectedActionIndex] ?? null);

const nextCategoryLabel = computed(() => {
  const categoryNamesById = new Map<string, string>([
    ["history", t("category.history")],
    ...store.categories.map((c) => [c.id, categoryDisplayName(c.name)] as const),
    ["automation", t("automation.entry")],
  ]);
  const ids = store.allCategoryIds;
  const currentIndex = ids.indexOf(store.selectedCategoryId);
  const nextId = ids[currentIndex >= 0 ? (currentIndex + 1) % ids.length : 0] ?? "history";
  return categoryNamesById.get(nextId) || t("category.history");
});
</script>

<template>
  <SettingsWindow v-if="isSettingsWindow" />
  <ClipViewerWindow v-else-if="isClipViewerWindow" />
  <LanSyncPanel v-else-if="isLanSyncWindow" />
  <OcrOverlayWindow v-else-if="isOcrOverlayWindow" />
  <OcrResultWindow v-else-if="isOcrResultWindow" />

  <main
    v-else
    class="raycast-container"
    :class="{ 'app-shell-preserve-current-app': isPreservingCurrentApp }"
    @click="closeFloatingLayers"
    @contextmenu="suppressDefaultContextMenu"
  >
    <!-- Top Command Search & Category Pill Tabs -->
    <CommandSearchBar
      :search-query="store.search"
      :shortcut="formattedShortcut"
      :categories="store.categories"
      :selected-category-id="store.selectedCategoryId"
      :editing-category-id="editingCategoryId"
      :history-count="store.clipTotalCount"
      :category-counts="categoryItemCounts"
      :settings-open="false"
      :append-copy-enabled="store.isAppendCopyEnabled"
      :append-copy-timeout-minutes="store.appendCopyTimeoutMinutes"
      :has-update="updater.hasAvailableUpdate.value"
      :checking-update="updater.updateStatus.value === 'checking'"
      @update:search-query="store.search = $event"
      @select-category="store.selectCategory"
      @create-category="createCategory"
      @edit-category="editCategory"
      @rename-category="renameCategory"
      @recolor-category="updateCategoryColor"
      @delete-category="deleteCategory"
      @toggle-settings="store.showSettings"
      @toggle-append-copy="store.toggleAppendCopy"
      @open-update="updater.openUpdateDialog"
      @close="hidePanelFromUi"
    />

    <!-- Update Dialog -->
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

    <!-- Error Banner if any -->
    <div
      v-if="store.error"
      class="error-banner m-2"
    >
      <AlertCircle class="size-4" />
      <span class="min-w-0 flex-1 truncate">{{ store.error }}</span>
    </div>

    <!-- Main Dual-Column Split View -->
    <div class="raycast-main-split">
      <!-- Left Pane: Mini Cards Stream -->
      <ClipListPane
        :list-ref="setClipListElement"
        :items="store.visibleItems"
        :selected-index="store.selectedCategoryId === 'automation' ? store.selectedActionIndex : store.selectedIndex"
        :selected-category-id="store.selectedCategoryId"
        :is-loading-more="store.isLoadingMoreClips"
        :can-reorder="canReorderVisibleItems"
        :editing-clip-key="editingClipKey"
        :editing-clip-name="editingClipName"
        :pending-delete-key="pendingDeleteByKey"
        :dragging-item-key="draggingItemKey"
        :item-drop-target-key="itemDropTargetKey"
        :item-drop-side="itemDropSide"
        :visible-actions="store.visibleActions"
        :fallback-groups="store.fallbackGroups"
        :item-category-tags="itemCategoryTags"
        :item-drag-style="itemDragStyle"
        :to-category-clip-view-item="toCategoryClipViewItem"
        @scroll="handleClipListScroll"
        @select="selectClipCard"
        @apply="store.applyItem"
        @expand="openClipViewer"
        @open-context-menu="openClipContextMenu"
        @update-editing-name="editingClipName = $event"
        @commit-rename="commitEditingClipName"
        @cancel-rename="cancelEditingClipName"
        @reorder-pointer-down="startItemDrag"
        @hover-preview="hoverPreviewItem"
        @leave-preview="clearHoveredPreviewItem"
        @select-action="(action) => selectActionCard(store.visibleActions.findIndex((a: AutomationAction) => a.id === action.id))"
        @run-action="runSelectedAction"
        @edit-action="openAutomationEditor"
        @delete-action="deleteAutomationAction"
        @copy-action="copyAutomationCommand"
        @open-action-context-menu="openAutomationContextMenu($event.action, { clientX: $event.x, clientY: $event.y } as MouseEvent)"
        @create-action="openAutomationEditor(null)"
      />

      <!-- Right Pane: Real-Time Inspector Preview -->
      <ClipInspectorPane
        :item="selectedClipItem"
        :automation-action="selectedAutomationAction"
        :mode="store.selectedCategoryId === 'automation' ? 'actions' : 'clip'"
        @copy="store.copyItem"
        @apply="store.applyItem"
        @expand="openClipViewer"
        @ocr="(item) => openClipViewer(item, true)"
        @run-automation="runSelectedAction"
      />
    </div>

    <!-- Bottom Keyboard Action Bar -->
    <KeyboardActionBar
      :mode="store.selectedCategoryId === 'automation' ? 'automation' : 'history'"
      :is-mac="isMacOs"
      :next-category-name="nextCategoryLabel"
    />

    <!-- Overlays & Dialogs -->
    <ErrorToast />

    <ClipContextMenu
      v-if="contextMenu"
      :context-menu="contextMenu"
      :categories="store.categories"
      :delete-label="contextDeleteLabel(contextMenu.item)"
      :delete-confirming="pendingDeleteContextKey === contextItemKey(contextMenu.item)"
      :show-move-submenu="showMoveSubmenu"
      :send-targets="sendTargetList"
      :show-send-submenu="showSendSubmenu"
      @paste="pasteContextItem"
      @copy="copyContextItem"
      @rename="renameContextItem"
      @move-to="addContextItemToCategory"
      @create-category="createCategoryForContextItem"
      @delete="deleteContextItem"
      @open-move-submenu="openMoveSubmenu"
      @schedule-close-move-submenu="scheduleCloseMoveSubmenu"
      @send-clip="sendClipTo"
      @open-send-submenu="openSendSubmenu"
      @schedule-close-send-submenu="scheduleCloseSendSubmenu"
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

    <AutomationDetailDialog
      :open="automationDetailOpen"
      :action="automationDetailAction"
      @run="(action) => { runSelectedAction(action); automationDetailOpen = false; }"
      @close="automationDetailOpen = false"
    />

    <AutomationContextMenu
      v-if="automationContextMenu"
      :automation="automationContextMenu.action"
      :x="automationContextMenu.x"
      :y="automationContextMenu.y"
      @run="runSelectedAction(automationContextMenu.action)"
      @edit="openAutomationEditor(automationContextMenu.action)"
      @copy="copyAutomationCommand(automationContextMenu.action)"
      @detail="openAutomationDetail(automationContextMenu.action)"
      @delete="deleteAutomationAction(automationContextMenu.action)"
      @import="triggerImport"
      @export="exportAllAutomations"
      @close="closeAutomationContextMenu"
    />

    <input
      ref="importFileInput"
      type="file"
      accept=".json"
      class="hidden"
      @change="onImportFileSelected"
    >
  </main>
</template>
