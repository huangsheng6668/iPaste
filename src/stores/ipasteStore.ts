import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";
import { clipMatchesSearch } from "../lib/clipSearch";
import { errorMessage } from "../lib/appError";
import { contextItemKey, originalClipId } from "../lib/clipKeys";
import { orderCategoryItemsByIds } from "./lib/ordering";
import { clampIndex, indexForKey, moveIndex } from "./lib/selection";
import { showError } from "./uiStore";
import { useSettingsStore } from "./settingsStore";
import { useCloudSyncStore } from "./cloudSyncStore";
import { useCategoryStore } from "./categoryStore";
import { useAutomationStore } from "./automationStore";
import type {
  AppSnapshot,
  CapturedEvent,
  Category,
  CategoryHitGroup,
  CategoryItem,
  ClipItem,
  ClipUpdatedEvent,
  ClipViewItem,
} from "../types";

const CLIP_PAGE_SIZE = 20;

export const useIpasteStore = defineStore("ipaste", () => {
  const settings = useSettingsStore();
  // 保留天变更后的全量重载回调（原 updateRetentionDays 直接 await load()，行为保持）。
  settings.registerRetentionReloader(() => load());

  const sync = useCloudSyncStore();
  // 云端快照落地（hydrate + clampSelection）留在本 store，经注册注入同步执行器。
  sync.registerSnapshotApplier(() => applyCloudSnapshot());

  const category = useCategoryStore();
  const automation = useAutomationStore();

  const clips = ref<ClipItem[]>([]);
  const selectedCategoryId = ref<string>("history");
  const selectedIndex = ref(0);
  const search = ref("");
  const closePanelRequested = ref(false);
  const isListening = ref(true);
  const isAppendCopyEnabled = ref(false);
  const isLoading = ref(false);
  const isLoadingMoreClips = ref(false);
  const hasMoreClips = ref(false);
  const fallbackGroups = ref<CategoryHitGroup[]>([]);
  const clipTotalCount = ref(0);
  const visibleHistoryTotalCount = ref(0);
  const error = ref<string | null>(null);
  let clipRequestId = 0;

  const activeCategory = computed(() =>
    category.categories.find((entry) => entry.id === selectedCategoryId.value),
  );

  /** 键盘循环切换分类用的完整顺序：history → 自定义分类 → automation。 */
  const allCategoryIds = computed(() => ["history", ...category.categories.map((entry) => entry.id), "automation"]);

  const visibleItems = computed<ClipViewItem[]>(() => {
    const query = search.value.trim().toLowerCase();
    const source =
      selectedCategoryId.value === "history"
        ? clips.value.map((clip) => ({ ...clip, collection: "history" as const }))
        : category.categoryItems
            .filter((item) => item.categoryId === selectedCategoryId.value)
            .map((item) => ({ ...item, collection: "category" as const }));

    if (!query) return source;

    return source.filter((item) => clipMatchesSearch(item, query));
  });

  const selectedItem = computed(() => visibleItems.value[selectedIndex.value]);

  /** load 与 applyCloudSnapshot 共用的快照装配；设置字段委托 settingsStore。 */
  function hydrateFromSnapshot(snapshot: AppSnapshot) {
    clips.value = snapshot.clips;
    hasMoreClips.value = snapshot.hasMoreClips;
    clipTotalCount.value = snapshot.clipTotalCount;
    visibleHistoryTotalCount.value = snapshot.clipTotalCount;
    category.categories = snapshot.categories;
    category.categoryItems = snapshot.categoryItems;
    isListening.value = snapshot.isListening;
    isAppendCopyEnabled.value = snapshot.isAppendCopyEnabled;
    settings.applySnapshotSettings(snapshot);
  }

  async function load() {
    isLoading.value = true;
    error.value = null;

    try {
      const snapshot = await ipasteApi.snapshot();
      hydrateFromSnapshot(snapshot);

      if (!category.categories.some((entry) => entry.id === selectedCategoryId.value)) {
        selectedCategoryId.value = "history";
      }
      clampSelection();
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    } finally {
      isLoading.value = false;
    }
  }

  // —— 历史分页 ——

  async function loadMoreClips() {
    if (fallbackGroups.value.length > 0) return;
    if (selectedCategoryId.value !== "history" || isLoadingMoreClips.value || !hasMoreClips.value) return;

    isLoadingMoreClips.value = true;
    try {
      const page = await ipasteApi.listClips(clips.value.length, CLIP_PAGE_SIZE, search.value);
      const existingIds = new Set(clips.value.map((clip) => clip.id));
      clips.value = [
        ...clips.value,
        ...page.clips.filter((clip) => !existingIds.has(clip.id)),
      ];
      hasMoreClips.value = page.hasMore;
      visibleHistoryTotalCount.value = page.totalCount;
      clipTotalCount.value = page.allCount;
      clampSelection();
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    } finally {
      isLoadingMoreClips.value = false;
    }
  }

  async function reloadClips() {
    const requestId = ++clipRequestId;

    try {
      const isHistorySearch = selectedCategoryId.value === "history" && search.value.trim() !== "";
      const result = isHistorySearch
        ? await ipasteApi.searchWithFallback(0, CLIP_PAGE_SIZE, search.value)
        : null;
      const page = result ? null : await ipasteApi.listClips(0, CLIP_PAGE_SIZE, search.value);
      if (requestId !== clipRequestId) return;

      if (result?.kind === "history") {
        clips.value = result.page.clips;
        hasMoreClips.value = result.page.hasMore;
        visibleHistoryTotalCount.value = result.page.totalCount;
        clipTotalCount.value = result.page.allCount;
        fallbackGroups.value = [];
      } else if (result?.kind === "categoryHits") {
        clips.value = [];
        hasMoreClips.value = false;
        visibleHistoryTotalCount.value = 0;
        clipTotalCount.value = 0;
        fallbackGroups.value = result.groups;
      } else if (page) {
        clips.value = page.clips;
        hasMoreClips.value = page.hasMore;
        visibleHistoryTotalCount.value = page.totalCount;
        clipTotalCount.value = page.allCount;
        fallbackGroups.value = [];
      }
      selectedIndex.value = 0;
    } catch (unknownError) {
      if (requestId === clipRequestId) {
        error.value = errorMessage(unknownError);
      }
    }
  }

  async function deleteClip(id: string) {
    await ipasteApi.deleteClip(id);
    const hadClip = clips.value.some((clip) => clip.id === id);
    clips.value = clips.value.filter((clip) => clip.id !== id);
    if (hadClip) {
      clipTotalCount.value = Math.max(0, clipTotalCount.value - 1);
      visibleHistoryTotalCount.value = Math.max(0, visibleHistoryTotalCount.value - 1);
      await backfillClips();
    }
    clampSelection();
  }

  /**
   * 删除补位：已加载窗口少了条目且服务端还有下一页时，按缺口从下一页顶部取回，
   * 保持窗口条数不减。否则连续删除后窗口可能缩到 0，而剩余历史只能靠触底滚动
   * 加载——空列表无法触发滚动，后续记录就再也看不到了。
   */
  async function backfillClips() {
    if (fallbackGroups.value.length > 0 || !hasMoreClips.value) return;

    try {
      const page = await ipasteApi.listClips(clips.value.length, 1, search.value);
      const existingIds = new Set(clips.value.map((clip) => clip.id));
      clips.value = [...clips.value, ...page.clips.filter((clip) => !existingIds.has(clip.id))];
      hasMoreClips.value = page.hasMore;
      visibleHistoryTotalCount.value = page.totalCount;
      clipTotalCount.value = page.allCount;
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    }
  }

  async function clearHistory() {
    const deleted = await ipasteApi.clearClips();
    clips.value = [];
    hasMoreClips.value = false;
    clipTotalCount.value = 0;
    visibleHistoryTotalCount.value = 0;
    selectedIndex.value = 0;
    return deleted;
  }

  function upsertClip(clip: ClipItem, totalCount?: number, wasInserted = false) {
    const hadClip = clips.value.some((item) => item.id === clip.id);
    const hasSearch = Boolean(search.value.trim());
    const matchesCurrentSearch = clipMatchesSearch(clip, search.value);

    if (!hasSearch || matchesCurrentSearch) {
      clips.value = [clip, ...clips.value.filter((item) => item.id !== clip.id)].slice(0, 120);
    }

    if (typeof totalCount === "number") {
      clipTotalCount.value = totalCount;
      if (!hasSearch) {
        visibleHistoryTotalCount.value = totalCount;
      } else if (wasInserted && clipMatchesSearch(clip, search.value)) {
        visibleHistoryTotalCount.value += 1;
      }
    } else if (!hadClip && !hasMoreClips.value) {
      clipTotalCount.value += 1;
      visibleHistoryTotalCount.value += 1;
    }
    if (!hasSearch) {
      hasMoreClips.value = hasMoreClips.value || clips.value.length >= CLIP_PAGE_SIZE;
    }
    if (selectedCategoryId.value === "history") {
      selectedIndex.value = 0;
    }
  }

  // —— 分类 ——（数据 CRUD 在 categoryStore；选择/计数等面板编排留在本层包装）

  async function createCategory(name: string, options: { select?: boolean } = {}) {
    const created = await category.createCategory(name);
    if (options.select ?? true) {
      selectedCategoryId.value = created.id;
      selectedIndex.value = 0;
    }
    return created;
  }

  async function createCategoryWithClip(name: string, clipId: string, options: { select?: boolean } = {}) {
    const { category: created, item } = await category.createCategoryWithClip(name, clipId);
    clips.value = clips.value.map((clip) =>
      clip.id === clipId ? { ...clip, favoriteCount: clip.favoriteCount + 1 } : clip,
    );
    if (options.select ?? true) {
      selectedCategoryId.value = created.id;
      selectedIndex.value = 0;
      fallbackGroups.value = [];
    }
    return { category: created, item };
  }

  async function renameCategory(target: Category, name: string) {
    await category.renameCategory(target, name);
  }

  async function updateCategoryColor(target: Category, color: string) {
    await category.updateCategoryColor(target, color);
  }

  async function deleteCategory(id: string) {
    await category.deleteCategory(id);
    selectedCategoryId.value = "history";
    selectedIndex.value = 0;
    fallbackGroups.value = [];
  }

  async function addToCategory(clipId: string, categoryId: string) {
    const { created } = await category.addToCategory(clipId, categoryId);
    if (created) {
      clips.value = clips.value.map((clip) =>
        clip.id === clipId ? { ...clip, favoriteCount: clip.favoriteCount + 1 } : clip,
      );
    }
  }

  async function removeCategoryItem(id: string) {
    await category.removeCategoryItem(id);
    clampSelection();
  }

  async function reorderCategories(categoryIds: string[]) {
    await category.reorderCategories(categoryIds);
  }

  /** 条目重排与选中恢复强耦合（选中键随乐观顺序移动），整体留在本 store。 */
  async function reorderCategoryItems(categoryId: string, itemIds: string[]) {
    const targetItems = category.categoryItems.filter((item) => item.categoryId === categoryId);
    if (itemIds.length !== targetItems.length) return;

    const previous = category.categoryItems;
    const selectedItemKey = selectedItem.value?.collection === "category" ? contextItemKey(selectedItem.value) : null;
    category.categoryItems = orderCategoryItemsByIds(previous, categoryId, itemIds);
    restoreCategorySelection(selectedItemKey);

    try {
      category.categoryItems = await ipasteApi.reorderCategoryItems(categoryId, itemIds);
      restoreCategorySelection(selectedItemKey);
      sync.syncCloudInBackground();
    } catch (unknownError) {
      category.categoryItems = previous;
      restoreCategorySelection(selectedItemKey);
      showError(unknownError);
      throw unknownError;
    }
  }

  // —— 条目操作与面板命令 ——

  async function renameClip(item: ClipViewItem, displayName: string | null) {
    const next = await ipasteApi.renameClip(item.id, item.collection, displayName);
    // 浏览器 dev 下 mock 未命中时后端返回 undefined；Tauri 下命令成功必有结果。
    if (!next) return;
    patchItem(item.collection, next);
    if (item.collection === "category") {
      sync.syncCloudInBackground();
    }
  }

  async function updateClipContent(item: ClipViewItem, text: string) {
    const next = await ipasteApi.updateClipContent(item.id, item.collection, text);
    // 浏览器 dev 下 mock 未命中时后端返回 undefined；Tauri 下命令成功必有结果。
    if (!next) return next;
    patchItem(item.collection, next);
    if (item.collection === "category") {
      sync.syncCloudInBackground();
    }
    return next;
  }

  async function applySelected() {
    if (!selectedItem.value) return;
    try {
      await ipasteApi.applyClip(
        originalClipId(selectedItem.value),
        selectedItem.value.clipType,
        selectedItem.value.text,
      );
    } catch (unknownError) {
      showError(unknownError);
    }
  }

  async function applyItem(item: ClipViewItem) {
    try {
      await ipasteApi.applyClip(originalClipId(item), item.clipType, item.text);
    } catch (unknownError) {
      showError(unknownError);
    }
  }

  async function copyItem(item: ClipViewItem) {
    await ipasteApi.copyClip(item.clipType, item.text);
  }

  async function setAppendCopyEnabled(enabled: boolean) {
    try {
      isAppendCopyEnabled.value = await ipasteApi.setAppendCopyEnabled(enabled);
    } catch (unknownError) {
      showError(unknownError);
      throw unknownError;
    }
  }

  async function toggleAppendCopy() {
    await setAppendCopyEnabled(!isAppendCopyEnabled.value);
  }

  async function hidePanel() {
    await ipasteApi.hidePanel();
  }

  async function showSettings() {
    await ipasteApi.showSettings();
  }

  function patchItem(collection: "history" | "category", item: ClipItem | CategoryItem) {
    if (collection === "history") {
      const clip = item as ClipItem;
      const hasClip = clips.value.some((entry) => entry.id === clip.id);
      if (hasClip) {
        clips.value = clips.value.map((entry) => (entry.id === clip.id ? clip : entry));
      } else if (clipMatchesSearch(clip, search.value)) {
        clips.value = [clip, ...clips.value].slice(0, 120);
      }
      return;
    }

    category.patchCategoryItem(item as CategoryItem);
  }

  // —— 事件落库动作（useAppEvents 调用；内部记账字段不再暴露给事件层直写）——

  /** clipboard-captured 事件的落库入口。 */
  function applyCaptured(payload: CapturedEvent) {
    upsertClip(payload.clip, payload.clipTotalCount, payload.wasInserted);
  }

  /** clip-updated 事件的落库入口：合并来源先移除旧条目并递减计数，再 patch 新条目。 */
  function applyClipUpdate(payload: ClipUpdatedEvent) {
    if (payload.mergedFromId && payload.mergedFromId !== payload.item.id) {
      if (payload.collection === "history") {
        clips.value = clips.value.filter((clip) => clip.id !== payload.mergedFromId);
        clipTotalCount.value = Math.max(0, clipTotalCount.value - 1);
        visibleHistoryTotalCount.value = Math.max(0, visibleHistoryTotalCount.value - 1);
      } else {
        category.categoryItems = category.categoryItems.filter((item) => item.id !== payload.mergedFromId);
      }
    }
    patchItem(payload.collection, payload.item);
    if (payload.collection === "category") {
      sync.syncCloudInBackground();
    }
  }

  // —— 云同步 ——（执行与计时器在 cloudSyncStore；快照落地留在本 store）

  async function applyCloudSnapshot() {
    const snapshot = await ipasteApi.syncCloudNow();
    hydrateFromSnapshot(snapshot);
    clampSelection();
  }

  // —— 选择与导航 ——

  function selectCategory(id: string) {
    selectedCategoryId.value = id;
    selectedIndex.value = 0;
    fallbackGroups.value = [];
    if (id === "history") {
      void reloadClips();
    }
  }

  function clearSearch() {
    if (!search.value) return;

    search.value = "";
    fallbackGroups.value = [];
    selectedIndex.value = 0;
  }

  function activatePanelDefault() {
    if (settings.panelOpenBehavior === "history" || !category.categories.some((entry) => entry.id === selectedCategoryId.value)) {
      selectCategory("history");
      return;
    }

    selectedIndex.value = 0;
  }

  function moveSelection(delta: number) {
    selectedIndex.value = moveIndex(selectedIndex.value, delta, visibleItems.value.length);
  }

  function setSelectedIndex(index: number) {
    selectedIndex.value = index;
  }

  function clampSelection() {
    selectedIndex.value = clampIndex(selectedIndex.value, visibleItems.value.length);
  }

  function restoreCategorySelection(itemKey: string | null) {
    const index = itemKey ? indexForKey(visibleItems.value, contextItemKey, itemKey) : -1;
    if (index >= 0) {
      selectedIndex.value = index;
      return;
    }

    clampSelection();
  }

  return {
    clips,
    categories: computed({ get: () => category.categories, set: (value) => (category.categories = value) }),
    categoryItems: computed({ get: () => category.categoryItems, set: (value) => (category.categoryItems = value) }),
    selectedCategoryId,
    selectedIndex,
    search,
    isListening,
    isAppendCopyEnabled,
    isLoading,
    isLoadingMoreClips,
    hasMoreClips,
    fallbackGroups,
    clipTotalCount,
    visibleHistoryTotalCount,
    error,
    activeCategory,
    visibleItems,
    selectedItem,
    allCategoryIds,
    load,
    reloadClips,
    loadMoreClips,
    createCategory,
    createCategoryWithClip,
    renameCategory,
    updateCategoryColor,
    deleteCategory,
    addToCategory,
    reorderCategories,
    reorderCategoryItems,
    removeCategoryItem,
    deleteClip,
    clearHistory,
    renameClip,
    updateClipContent,
    applySelected,
    applyItem,
    copyItem,
    setAppendCopyEnabled,
    toggleAppendCopy,
    hidePanel,
    showSettings,
    selectCategory,
    clearSearch,
    activatePanelDefault,
    moveSelection,
    setSelectedIndex,
    clampSelection,
    upsertClip,
    patchItem,
    applyCaptured,
    applyClipUpdate,
    closePanelRequested,
    // —— automation 域转发（实现在 automationStore；可写 computed 兼容既有赋值点）——
    automations: computed({ get: () => automation.automations, set: (value) => (automation.automations = value) }),
    selectedActionIndex: computed({
      get: () => automation.selectedActionIndex,
      set: (value) => (automation.selectedActionIndex = value),
    }),
    actionsQuery: computed({ get: () => automation.actionsQuery, set: (value) => (automation.actionsQuery = value) }),
    runningAutomationLogs: computed({
      get: () => automation.runningAutomationLogs,
      set: (value) => (automation.runningAutomationLogs = value),
    }),
    visibleActions: computed(() => automation.visibleActions),
    loadAutomations: automation.loadAutomations,
    createAutomation: automation.createAutomation,
    updateAutomation: automation.updateAutomation,
    deleteAutomation: automation.deleteAutomation,
    runAutomation: automation.runAutomation,

    // —— 设置域转发（实现在 settingsStore；可写 computed 保持外部赋值兼容，Task 17 移除）——
    shortcut: computed({ get: () => settings.shortcut, set: (value) => (settings.shortcut = value) }),
    ocrShortcut: computed({ get: () => settings.ocrShortcut, set: (value) => (settings.ocrShortcut = value) }),
    retentionDays: computed({ get: () => settings.retentionDays, set: (value) => (settings.retentionDays = value) }),
    appendCopyTimeoutMinutes: computed({
      get: () => settings.appendCopyTimeoutMinutes,
      set: (value) => (settings.appendCopyTimeoutMinutes = value),
    }),
    panelOpenBehavior: computed({
      get: () => settings.panelOpenBehavior,
      set: (value) => (settings.panelOpenBehavior = value),
    }),
    panelLayout: computed({ get: () => settings.panelLayout, set: (value) => (settings.panelLayout = value) }),
    ocrMode: computed({ get: () => settings.ocrMode, set: (value) => (settings.ocrMode = value) }),
    ocrEngine: computed({ get: () => settings.ocrEngine, set: (value) => (settings.ocrEngine = value) }),
    language: computed({ get: () => settings.language, set: (value) => (settings.language = value) }),
    cloud: computed({ get: () => settings.cloud, set: (value) => (settings.cloud = value) }),
    cloudOcr: computed({ get: () => settings.cloudOcr, set: (value) => (settings.cloudOcr = value) }),
    applySettings: settings.applySettings,
    updateRetentionDays: settings.updateRetentionDays,
    updateAppendCopyTimeout: settings.updateAppendCopyTimeout,
    updateShortcut: settings.updateShortcut,
    updateOcrShortcut: settings.updateOcrShortcut,
    updatePanelOpenBehavior: settings.updatePanelOpenBehavior,
    updatePanelLayout: settings.updatePanelLayout,
    updateOcrMode: settings.updateOcrMode,
    updateOcrEngine: settings.updateOcrEngine,
    updateLanguage: settings.updateLanguage,
    saveOpenaiOcrConfig: settings.saveOpenaiOcrConfig,
    clearOpenaiOcrConfig: settings.clearOpenaiOcrConfig,
    testOpenaiOcr: settings.testOpenaiOcr,
    saveCloudSettings: sync.saveCloudSettings,
    disableCloudSync: sync.disableCloudSync,
    testCloudSettings: sync.testCloudSettings,
    syncCloudNow: sync.syncCloudNow,
    syncCloudInBackground: sync.syncCloudInBackground,
  };
});
