<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { AlertCircle, ClipboardCopy, Download, Info, Pencil, Play, Trash2, Upload } from "lucide-vue-next";
import ClipContextMenu from "./components/ClipContextMenu.vue";
import AutomationEditorDialog from "./components/AutomationEditorDialog.vue";
import AutomationConfirmDialog from "./components/AutomationConfirmDialog.vue";
import AutomationDetailPane from "./components/AutomationDetailPane.vue";
import ClipViewerWindow from "./components/ClipViewerWindow.vue";
import OcrOverlayWindow from "./components/OcrOverlayWindow.vue";
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
import { usePanelKeyboard } from "./composables/usePanelKeyboard";
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
const isOcrOverlayWindow = new URLSearchParams(window.location.search).get("window") === "ocr-overlay";
const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const isPreservingCurrentApp = ref(false);
const editingClipKey = ref<string | null>(null);
const editingClipName = ref("");
let unlistenShortcutOpened: UnlistenFn | null = null;
let unlistenPanelVisibilityChanged: UnlistenFn | null = null;
let lastUpdateCheckAt = 0;
let suppressNextItemSelect = false;

const clipMenu = useClipContextMenu(store, { onStartRename: startEditingClipName, onFullClose: closeFloatingLayers });
const {
  contextMenu,
  editingCategoryId,
  showMoveSubmenu,
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
  showClipListScrollbar,
  resetClipListScroll,
  setupWatches,
  cleanup: cleanupClipListScroll,
} = useClipListScroll({ store });

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
  if (isClipViewerWindow) return;
  if (isSettingsWindow) return;
  if (isLanSyncWindow) return;
  if (isOcrOverlayWindow) return;

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
  if (isClipViewerWindow) return;
  if (isSettingsWindow) return;
  if (isLanSyncWindow) return;
  if (isOcrOverlayWindow) return;

  document.removeEventListener("keydown", handleKeydown, true);
  document.removeEventListener("keyup", handleKeyup, true);
  document.removeEventListener("selectionchange", handleSelectionChange);
  window.removeEventListener("blur", closeFloatingLayers);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
  clearMoveSubmenuCloseTimer();
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
  const allCategories = [
    { id: "history", name: t("category.history") },
    ...store.categories.map((c) => ({ id: c.id, name: categoryDisplayName(c.name) })),
    { id: "automation", name: t("automation.entry") },
  ];
  const currentIndex = allCategories.findIndex((c) => c.id === store.selectedCategoryId);
  const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % allCategories.length : 0;
  return allCategories[nextIndex]?.name || t("category.history");
});

async function focusEditingClipName() {
  await nextTick();
  window.setTimeout(() => {
    const input = document.querySelector<HTMLInputElement>(".clip-title-input");
    input?.focus();
    input?.select();
  }, 40);
}
</script>

<template>
  <SettingsWindow v-if="isSettingsWindow" />
  <ClipViewerWindow v-else-if="isClipViewerWindow" />
  <LanSyncPanel v-else-if="isLanSyncWindow" />
  <OcrOverlayWindow v-else-if="isOcrOverlayWindow" />

  <main
    v-else
    class="raycast-container"
    :class="{ 'app-shell-preserve-current-app': isPreservingCurrentApp }"
    @click="closeFloatingLayers"
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
        @select="selectClipCard"
        @apply="store.applyItem"
        @expand="openClipViewer"
        @open-context-menu="openClipContextMenu"
        @update-editing-name="updateEditingClipName"
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
        class="dialog-backdrop"
        @click.self="automationDetailOpen = false"
      >
        <div class="automation-detail-panel">
          <AutomationDetailPane
            :action="automationDetailAction"
            @run="runSelectedAction(automationDetailAction); automationDetailOpen = false"
          />
          <div class="flex justify-end border-t border-slate-200 px-4 py-2">
            <button
              type="button"
              class="btn-ghost"
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
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="triggerImport"
      >
        <Upload class="size-3.5" /> {{ t("automation.importAction") }}
      </button>
      <button
        type="button"
        class="context-menu-item"
        tabindex="-1"
        role="menuitem"
        @click="exportAllAutomations"
      >
        <Download class="size-3.5" /> {{ t("automation.exportAll") }}
      </button>
    </div>

    <input
      ref="importFileInput"
      type="file"
      accept=".json"
      class="hidden"
      @change="onImportFileSelected"
    >
  </main>
</template>
