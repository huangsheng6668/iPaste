import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";
import { clipMatchesSearch, clipMatchesTypeFilter } from "../lib/clipSearch";
import { errorMessage } from "../lib/appError";
import { contextItemKey, originalClipId } from "../lib/clipKeys";
import { orderCategoryItemsByIds } from "./lib/ordering";
import { clampIndex, indexForKey, moveIndex } from "./lib/selection";
import { showError } from "./uiStore";
import { useSettingsStore } from "./settingsStore";
import { useCloudSyncStore } from "./cloudSyncStore";
import { useCategoryStore } from "./categoryStore";
import type {
  AppSnapshot,
  CapturedEvent,
  Category,
  CategoryHitGroup,
  CategoryItem,
  ClipItem,
  ClipPage,
  ClipTypeFilter,
  ClipUpdatedEvent,
  ClipViewItem,
} from "../types";

const CLIP_PAGE_SIZE = 20;
/** 类型筛选的循环顺序（键盘 Ctrl/Cmd+G 与 selectTypeFilter 共用）。 */
const TYPE_FILTER_CYCLE: ClipTypeFilter[] = ["all", "text", "image"];

export const useIpasteStore = defineStore("ipaste", () => {
  const settings = useSettingsStore();
  // 保留天变更后的全量重载回调（原 updateRetentionDays 直接 await load()，行为保持）。
  settings.registerRetentionReloader(() => load());

  const sync = useCloudSyncStore();
  // 云端快照落地（hydrate + clampSelection）留在本 store，经注册注入同步执行器。
  sync.registerSnapshotApplier(() => applyCloudSnapshot());

  const category = useCategoryStore();

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
  /** 历史类型筛选（全部/文本/图片）；实际过滤在 SQL 层，这里只保存视图状态与徽章计数。 */
  const typeFilter = ref<ClipTypeFilter>("all");
  const clipTextCount = ref(0);
  const clipImageCount = ref(0);
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
    clipTextCount.value = snapshot.clipTextCount;
    clipImageCount.value = snapshot.clipImageCount;
    category.categories = snapshot.categories;
    category.categoryItems = snapshot.categoryItems;
    isListening.value = snapshot.isListening;
    isAppendCopyEnabled.value = snapshot.isAppendCopyEnabled;
    settings.applySnapshotSettings(snapshot);
  }

  /** 分页响应的计数落地（reload/loadMore/backfill 共用）：徽章计数跟随搜索上下文，由后端随页返回。 */
  function applyClipPageCounts(page: ClipPage) {
    visibleHistoryTotalCount.value = page.totalCount;
    clipTotalCount.value = page.allCount;
    clipTextCount.value = page.textCount;
    clipImageCount.value = page.imageCount;
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
      const page = await ipasteApi.listClips(clips.value.length, CLIP_PAGE_SIZE, search.value, typeFilter.value);
      const existingIds = new Set(clips.value.map((clip) => clip.id));
      clips.value = [
        ...clips.value,
        ...page.clips.filter((clip) => !existingIds.has(clip.id)),
      ];
      hasMoreClips.value = page.hasMore;
      applyClipPageCounts(page);
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
        ? await ipasteApi.searchWithFallback(0, CLIP_PAGE_SIZE, search.value, typeFilter.value)
        : null;
      const page = result ? null : await ipasteApi.listClips(0, CLIP_PAGE_SIZE, search.value, typeFilter.value);
      if (requestId !== clipRequestId) return;

      if (result?.kind === "history") {
        clips.value = result.page.clips;
        hasMoreClips.value = result.page.hasMore;
        applyClipPageCounts(result.page);
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
        applyClipPageCounts(page);
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
    const deleted = clips.value.find((clip) => clip.id === id);
    clips.value = clips.value.filter((clip) => clip.id !== id);
    if (deleted) {
      clipTotalCount.value = Math.max(0, clipTotalCount.value - 1);
      visibleHistoryTotalCount.value = Math.max(0, visibleHistoryTotalCount.value - 1);
      if (deleted.clipType === "image") {
        clipImageCount.value = Math.max(0, clipImageCount.value - 1);
      } else {
        clipTextCount.value = Math.max(0, clipTextCount.value - 1);
      }
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
      const page = await ipasteApi.listClips(clips.value.length, 1, search.value, typeFilter.value);
      const existingIds = new Set(clips.value.map((clip) => clip.id));
      clips.value = [...clips.value, ...page.clips.filter((clip) => !existingIds.has(clip.id))];
      hasMoreClips.value = page.hasMore;
      applyClipPageCounts(page);
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
    clipTextCount.value = 0;
    clipImageCount.value = 0;
    selectedIndex.value = 0;
    return deleted;
  }

  function upsertClip(clip: ClipItem, totalCount?: number, wasInserted = false) {
    const hadClip = clips.value.some((item) => item.id === clip.id);
    const hasSearch = Boolean(search.value.trim());
    const matchesCurrentSearch = clipMatchesSearch(clip, search.value);
    // 与当前视图口径一致才进窗口：搜索词与类型筛选双重约束（类型筛选下非目标类型只记账不进列表）。
    const matchesCurrentView = (!hasSearch || matchesCurrentSearch) && clipMatchesTypeFilter(clip, typeFilter.value);

    if (matchesCurrentView) {
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
    // 徽章计数跟随搜索上下文（不含类型筛选）：新条目匹配搜索即计入对应分段。
    if (wasInserted && matchesCurrentSearch) {
      if (clip.clipType === "image") {
        clipImageCount.value += 1;
      } else {
        clipTextCount.value += 1;
      }
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
      } else if (clipMatchesSearch(clip, search.value) && clipMatchesTypeFilter(clip, typeFilter.value)) {
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
        // 合并入口只有追加复制（纯文本），被合并条目恒为文本类。
        clipTextCount.value = Math.max(0, clipTextCount.value - 1);
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

  // —— 历史类型筛选（全部/文本/图片）——

  function selectTypeFilter(next: ClipTypeFilter) {
    if (next === typeFilter.value) return;
    typeFilter.value = next;
    selectedIndex.value = 0;
    fallbackGroups.value = [];
    if (selectedCategoryId.value === "history") {
      void reloadClips();
    }
  }

  /** Ctrl/Cmd+G 循环切换：all → text → image → all（Shift 反向）。 */
  function cycleTypeFilter(delta: number) {
    const length = TYPE_FILTER_CYCLE.length;
    const current = TYPE_FILTER_CYCLE.indexOf(typeFilter.value);
    const next = TYPE_FILTER_CYCLE[(current + delta + length) % length] ?? "all";
    selectTypeFilter(next);
  }

  /** 面板关闭时复位筛选（与 clearSearch 同节奏）：重新打开始终回到完整历史。 */
  function resetTypeFilter() {
    if (typeFilter.value === "all") return;
    typeFilter.value = "all";
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
    typeFilter,
    clipTextCount,
    clipImageCount,
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
    selectTypeFilter,
    cycleTypeFilter,
    resetTypeFilter,
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
  };
});
