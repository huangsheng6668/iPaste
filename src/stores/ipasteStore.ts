import { listen } from "@tauri-apps/api/event";
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { cleanLanguage, setLanguage } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { clipMatchesSearch } from "../lib/clipSearch";
import { errorMessage } from "../lib/appError";
import { IPASTE_EVENTS } from "../types/generated/events";
import { filterAutomations } from "./lib/automationFilter";
import {
  compareSortOrder,
  compareCategoryItemOrder,
  orderCategoriesByIds,
  orderCategoryItemsByIds,
} from "./lib/ordering";
import {
  DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
  DEFAULT_LANGUAGE,
  DEFAULT_OCR_MODE,
  DEFAULT_PANEL_LAYOUT,
  DEFAULT_RETENTION_DAYS,
  cleanAppendCopyTimeoutMinutes,
  cleanOcrMode,
  cleanPanelLayout,
  isSettingsCommandMissing,
} from "./lib/settings";
import type {
  AppSettings,
  AppendCopyChangedEvent,
  AutomationAction,
  AutomationInput,
  AutomationRunFinishedEvent,
  AutomationRunOutputEvent,
  AutomationRunStartedEvent,
  CapturedEvent,
  Category,
  CategoryHitGroup,
  CategoryItem,
  ClipItem,
  ClipUpdatedEvent,
  ClipViewItem,
  CloudSettings,
  ListeningChangedEvent,
  Language,
  OcrMode,
  PanelLayout,
  PanelOpenBehavior,
  SettingsChangedEvent,
} from "../types";

const CATEGORY_COLORS = ["#0D9488", "#2563EB", "#7C3AED", "#D97706", "#DC2626", "#475569"];
const CLIP_PAGE_SIZE = 20;
const isTauri = "__TAURI_INTERNALS__" in window;

export const useIpasteStore = defineStore("ipaste", () => {
  const clips = ref<ClipItem[]>([]);
  const categories = ref<Category[]>([]);
  const categoryItems = ref<CategoryItem[]>([]);
  const selectedCategoryId = ref<string>("history");
  const selectedIndex = ref(0);
  const search = ref("");
  const automations = ref<AutomationAction[]>([]);
  const selectedActionIndex = ref(0);
  const actionsQuery = ref("");
  const runningAutomationLogs = ref<Record<string, { stdout: string; stderr: string }>>({});
  const closePanelRequested = ref(false);
  const shortcut = ref("CommandOrControl+Shift+V");
  const isListening = ref(true);
  const isAppendCopyEnabled = ref(false);
  const isLoading = ref(false);
  const isLoadingMoreClips = ref(false);
  const hasMoreClips = ref(false);
  const fallbackGroups = ref<CategoryHitGroup[]>([]);
  const clipTotalCount = ref(0);
  const visibleHistoryTotalCount = ref(0);
  const error = ref<string | null>(null);
  const retentionDays = ref(DEFAULT_RETENTION_DAYS);
  const appendCopyTimeoutMinutes = ref(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
  const panelOpenBehavior = ref<PanelOpenBehavior>("history");
  const panelLayout = ref<PanelLayout>(DEFAULT_PANEL_LAYOUT);
  const ocrMode = ref<OcrMode>(DEFAULT_OCR_MODE);
  const language = ref<Language>(DEFAULT_LANGUAGE);
  const cloud = ref<CloudSettings>({
    apiAddress: "",
    apiKey: "",
    enabled: false,
    lastConnectedAt: null,
  });
  let backgroundSyncTimer: number | null = null;
  let clipRequestId = 0;

  const activeCategory = computed(() =>
    categories.value.find((category) => category.id === selectedCategoryId.value),
  );

  const visibleItems = computed<ClipViewItem[]>(() => {
    const query = search.value.trim().toLowerCase();
    const source =
      selectedCategoryId.value === "history"
        ? clips.value.map((clip) => ({ ...clip, collection: "history" as const }))
        : categoryItems.value
            .filter((item) => item.categoryId === selectedCategoryId.value)
            .map((item) => ({ ...item, collection: "category" as const }));

    if (!query) return source;

    return source.filter((item) => clipMatchesSearch(item, query));
  });

  const selectedItem = computed(() => visibleItems.value[selectedIndex.value]);

  async function load() {
    isLoading.value = true;
    error.value = null;

    try {
      const snapshot = await ipasteApi.snapshot();
      clips.value = snapshot.clips;
      hasMoreClips.value = snapshot.hasMoreClips;
      clipTotalCount.value = snapshot.clipTotalCount;
      visibleHistoryTotalCount.value = snapshot.clipTotalCount;
      categories.value = snapshot.categories;
      categoryItems.value = snapshot.categoryItems;
      shortcut.value = snapshot.shortcut;
      isListening.value = snapshot.isListening;
      isAppendCopyEnabled.value = snapshot.isAppendCopyEnabled;
      retentionDays.value = snapshot.settings.retentionDays;
      appendCopyTimeoutMinutes.value = cleanAppendCopyTimeoutMinutes(snapshot.settings.appendCopyTimeoutMinutes);
      panelOpenBehavior.value = snapshot.settings.panelOpenBehavior;
      panelLayout.value = cleanPanelLayout(snapshot.settings.panelLayout);
      ocrMode.value = cleanOcrMode(snapshot.settings.ocrMode);
      language.value = cleanLanguage(snapshot.settings.language);
      setLanguage(language.value);
      cloud.value = snapshot.settings.cloud;

      if (!categories.value.some((category) => category.id === selectedCategoryId.value)) {
        selectedCategoryId.value = "history";
      }
      clampSelection();
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    } finally {
      isLoading.value = false;
    }
  }

  async function bindEvents() {
    if (!isTauri) return;

    await listen<CapturedEvent>(IPASTE_EVENTS.clipboardCaptured, (event) => {
      upsertClip(event.payload.clip, event.payload.clipTotalCount, event.payload.wasInserted);
    });

    // 局域网同步收到条目（历史或分组）后，主窗口也要刷新列表：lan-clip-received
    // 广播到所有窗口，但此前只有 LAN 同步窗口监听，主窗口列表不更新，
    // 表现为「B 端提示已接收但列表里没有新条目」。
    await listen(IPASTE_EVENTS.lanClipReceived, () => {
      void load();
    });

    await listen<ListeningChangedEvent>(IPASTE_EVENTS.listeningChanged, (event) => {
      isListening.value = event.payload.isListening;
    });

    await listen<AppendCopyChangedEvent>(IPASTE_EVENTS.appendCopyChanged, (event) => {
      isAppendCopyEnabled.value = event.payload.isEnabled;
    });

    await listen<ClipUpdatedEvent>(IPASTE_EVENTS.clipUpdated, (event) => {
      if (event.payload.mergedFromId && event.payload.mergedFromId !== event.payload.item.id) {
        if (event.payload.collection === "history") {
          clips.value = clips.value.filter((clip) => clip.id !== event.payload.mergedFromId);
          clipTotalCount.value = Math.max(0, clipTotalCount.value - 1);
          visibleHistoryTotalCount.value = Math.max(0, visibleHistoryTotalCount.value - 1);
        } else {
          categoryItems.value = categoryItems.value.filter((item) => item.id !== event.payload.mergedFromId);
        }
      }
      patchItem(event.payload.collection, event.payload.item);
      if (event.payload.collection === "category") {
        syncCloudInBackground();
      }
    });

    await listen<SettingsChangedEvent>(IPASTE_EVENTS.settingsChanged, (event) => {
      applySettings(event.payload.settings);
    });

    await listen<{ visible: boolean }>(IPASTE_EVENTS.panelVisibilityChanged, (event) => {
      if (event.payload.visible) {
        // 每次面板显示时刷新快照：LAN 同步收到的条目/分类在面板隐藏期间落库，
        // 若事件驱动的刷新错过（如 webview 重建），这里兜底保证数据可见。
        void load();
        activatePanelDefault();
      }
    });

    // 捕获失败此前是死事件（Rust 发、无人听）：保留排障信号但不打扰 UI。
    await listen<{ message?: string }>(IPASTE_EVENTS.captureError, (event) => {
      console.warn("[ipaste] clipboard capture error:", event.payload);
    });
  }

  async function createCategory(name: string, options: { select?: boolean } = {}) {
    const color = CATEGORY_COLORS[categories.value.length % CATEGORY_COLORS.length];
    const category = await ipasteApi.createCategory(name, color);
    categories.value = [...categories.value, category].sort(compareSortOrder);
    syncCloudInBackground();
    if (options.select ?? true) {
      selectedCategoryId.value = category.id;
      selectedIndex.value = 0;
    }
    return category;
  }

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

  async function createCategoryWithClip(name: string, clipId: string, options: { select?: boolean } = {}) {
    const color = CATEGORY_COLORS[categories.value.length % CATEGORY_COLORS.length];
    const { category, item } = await ipasteApi.createCategoryWithClip(name, color, clipId);
    categories.value = [...categories.value, category].sort(compareSortOrder);
    categoryItems.value = [...categoryItems.value, item].sort(compareCategoryItemOrder);
    clips.value = clips.value.map((clip) =>
      clip.id === clipId ? { ...clip, favoriteCount: clip.favoriteCount + 1 } : clip,
    );
    syncCloudInBackground();
    if (options.select ?? true) {
      selectedCategoryId.value = category.id;
      selectedIndex.value = 0;
      fallbackGroups.value = [];
    }
    return { category, item };
  }

  async function renameCategory(category: Category, name: string) {
    const next = await ipasteApi.updateCategory(category.id, name, category.color);
    categories.value = categories.value.map((item) => (item.id === next.id ? next : item));
    syncCloudInBackground();
  }

  async function updateCategoryColor(category: Category, color: string) {
    const next = await ipasteApi.updateCategory(category.id, category.name, color);
    categories.value = categories.value.map((item) => (item.id === next.id ? next : item));
    syncCloudInBackground();
  }

  async function deleteCategory(id: string) {
    await ipasteApi.deleteCategory(id);
    categories.value = categories.value.filter((category) => category.id !== id);
    categoryItems.value = categoryItems.value.filter((item) => item.categoryId !== id);
    selectedCategoryId.value = "history";
    selectedIndex.value = 0;
    fallbackGroups.value = [];
    syncCloudInBackground();
  }

  async function addToCategory(clipId: string, categoryId: string) {
    const item = await ipasteApi.addClipToCategory(clipId, categoryId);
    const existing = categoryItems.value.some((categoryItem) => categoryItem.id === item.id);
    if (!existing) {
      categoryItems.value = [...categoryItems.value, item].sort(compareCategoryItemOrder);
      clips.value = clips.value.map((clip) =>
        clip.id === clipId ? { ...clip, favoriteCount: clip.favoriteCount + 1 } : clip,
      );
      syncCloudInBackground();
    }
  }

  async function removeCategoryItem(id: string) {
    await ipasteApi.removeCategoryItem(id);
    categoryItems.value = categoryItems.value.filter((item) => item.id !== id);
    clampSelection();
    syncCloudInBackground();
  }

  async function deleteClip(id: string) {
    await ipasteApi.deleteClip(id);
    const hadClip = clips.value.some((clip) => clip.id === id);
    clips.value = clips.value.filter((clip) => clip.id !== id);
    if (hadClip) {
      clipTotalCount.value = Math.max(0, clipTotalCount.value - 1);
      visibleHistoryTotalCount.value = Math.max(0, visibleHistoryTotalCount.value - 1);
    }
    clampSelection();
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

  async function renameClip(item: ClipViewItem, displayName: string | null) {
    const next = await ipasteApi.renameClip(item.id, item.collection, displayName);
    patchItem(item.collection, next);
    if (item.collection === "category") {
      syncCloudInBackground();
    }
  }

  async function reorderCategories(categoryIds: string[]) {
    if (categoryIds.length !== categories.value.length) return;

    const previous = categories.value;
    categories.value = orderCategoriesByIds(previous, categoryIds);

    try {
      categories.value = await ipasteApi.reorderCategories(categoryIds);
      syncCloudInBackground();
    } catch (unknownError) {
      categories.value = previous;
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function reorderCategoryItems(categoryId: string, itemIds: string[]) {
    const targetItems = categoryItems.value.filter((item) => item.categoryId === categoryId);
    if (itemIds.length !== targetItems.length) return;

    const previous = categoryItems.value;
    const selectedItemId = selectedItem.value?.collection === "category" ? selectedItem.value.id : null;
    categoryItems.value = orderCategoryItemsByIds(previous, categoryId, itemIds);
    restoreCategorySelection(selectedItemId);

    try {
      categoryItems.value = await ipasteApi.reorderCategoryItems(categoryId, itemIds);
      restoreCategorySelection(selectedItemId);
      syncCloudInBackground();
    } catch (unknownError) {
      categoryItems.value = previous;
      restoreCategorySelection(selectedItemId);
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function updateClipContent(item: ClipViewItem, text: string) {
    const next = await ipasteApi.updateClipContent(item.id, item.collection, text);
    patchItem(item.collection, next);
    if (item.collection === "category") {
      syncCloudInBackground();
    }
    return next;
  }

  async function applySelected() {
    if (!selectedItem.value) return;
    error.value = null;
    try {
      await ipasteApi.applyClip(
        originalClipId(selectedItem.value),
        selectedItem.value.clipType,
        selectedItem.value.text,
      );
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    }
  }

  async function applyItem(item: ClipViewItem) {
    error.value = null;
    try {
      await ipasteApi.applyClip(originalClipId(item), item.clipType, item.text);
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
    }
  }

  async function copyItem(item: ClipViewItem) {
    await ipasteApi.copyClip(item.clipType, item.text);
  }

  async function setAppendCopyEnabled(enabled: boolean) {
    try {
      isAppendCopyEnabled.value = await ipasteApi.setAppendCopyEnabled(enabled);
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
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

  async function updateRetentionDays(days: number) {
    const settings = await ipasteApi.updateSettings(days);
    applySettings(settings);
    await load();
  }

  async function updateAppendCopyTimeout(minutes: number) {
    const nextMinutes = cleanAppendCopyTimeoutMinutes(minutes);
    appendCopyTimeoutMinutes.value = nextMinutes;

    try {
      const settings = await ipasteApi.updateAppendCopyTimeout(nextMinutes);
      applySettings(settings);
    } catch (unknownError) {
      if (isSettingsCommandMissing(unknownError, "update_append_copy_timeout")) return;
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function updateShortcut(value: string) {
    const settings = await ipasteApi.updateShortcut(value);
    applySettings(settings);
  }

  async function updatePanelOpenBehavior(behavior: PanelOpenBehavior) {
    const settings = await ipasteApi.updatePanelOpenBehavior(behavior);
    applySettings(settings);
  }

  async function updatePanelLayout(layout: PanelLayout) {
    const nextLayout = cleanPanelLayout(layout);
    panelLayout.value = nextLayout;

    try {
      const settings = await ipasteApi.updatePanelLayout(nextLayout);
      applySettings(settings);
    } catch (unknownError) {
      if (isSettingsCommandMissing(unknownError, "update_panel_layout")) return;
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function updateOcrMode(mode: OcrMode) {
    const nextMode = cleanOcrMode(mode);
    ocrMode.value = nextMode;

    try {
      const settings = await ipasteApi.updateOcrMode(nextMode);
      applySettings(settings);
    } catch (unknownError) {
      if (isSettingsCommandMissing(unknownError, "update_ocr_mode")) return;
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function updateLanguage(value: Language) {
    const nextLanguage = cleanLanguage(value);
    language.value = nextLanguage;
    setLanguage(nextLanguage);

    try {
      const settings = await ipasteApi.updateLanguage(nextLanguage);
      applySettings(settings);
    } catch (unknownError) {
      if (isSettingsCommandMissing(unknownError, "update_language")) return;
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  async function saveCloudSettings(apiAddress: string, apiKey: string) {
    const settings = await ipasteApi.updateCloudSettings(apiAddress, apiKey);
    applySettings(settings);
    await syncCloudNow();
  }

  async function disableCloudSync() {
    const settings = await ipasteApi.disableCloudSync();
    applySettings(settings);
  }

  async function testCloudSettings(apiAddress: string, apiKey: string) {
    return ipasteApi.testCloudSettings(apiAddress, apiKey);
  }

  async function syncCloudNow() {
    if (!cloud.value.enabled) return;

    try {
      clearBackgroundSyncTimer();
      await applyCloudSnapshot();
    } catch (unknownError) {
      error.value = errorMessage(unknownError);
      throw unknownError;
    }
  }

  function syncCloudInBackground() {
    if (!cloud.value.enabled) return;
    clearBackgroundSyncTimer();
    backgroundSyncTimer = window.setTimeout(() => {
      backgroundSyncTimer = null;
      void ipasteApi.syncCloudInBackground().catch((unknownError) => {
        error.value = errorMessage(unknownError);
      });
    }, 600);
  }

  async function applyCloudSnapshot() {
    const snapshot = await ipasteApi.syncCloudNow();
    clips.value = snapshot.clips;
    hasMoreClips.value = snapshot.hasMoreClips;
    clipTotalCount.value = snapshot.clipTotalCount;
    visibleHistoryTotalCount.value = snapshot.clipTotalCount;
    categories.value = snapshot.categories;
    categoryItems.value = snapshot.categoryItems;
    shortcut.value = snapshot.shortcut;
    isListening.value = snapshot.isListening;
    isAppendCopyEnabled.value = snapshot.isAppendCopyEnabled;
    retentionDays.value = snapshot.settings.retentionDays;
    appendCopyTimeoutMinutes.value = cleanAppendCopyTimeoutMinutes(snapshot.settings.appendCopyTimeoutMinutes);
    panelOpenBehavior.value = snapshot.settings.panelOpenBehavior;
    panelLayout.value = cleanPanelLayout(snapshot.settings.panelLayout);
    ocrMode.value = cleanOcrMode(snapshot.settings.ocrMode);
    language.value = cleanLanguage(snapshot.settings.language);
    setLanguage(language.value);
    cloud.value = snapshot.settings.cloud;
    clampSelection();
  }

  function clearBackgroundSyncTimer() {
    if (backgroundSyncTimer === null) return;
    window.clearTimeout(backgroundSyncTimer);
    backgroundSyncTimer = null;
  }

  function applySettings(settings: AppSettings) {
    shortcut.value = settings.shortcut;
    retentionDays.value = settings.retentionDays;
    appendCopyTimeoutMinutes.value = settings.appendCopyTimeoutMinutes;
    panelOpenBehavior.value = settings.panelOpenBehavior;
    panelLayout.value = settings.panelLayout;
    ocrMode.value = settings.ocrMode;
    language.value = settings.language;
    setLanguage(language.value);
    cloud.value = settings.cloud;
  }

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
    if (panelOpenBehavior.value === "history" || !categories.value.some((category) => category.id === selectedCategoryId.value)) {
      selectCategory("history");
      return;
    }

    selectedIndex.value = 0;
  }

  function moveSelection(delta: number) {
    if (!visibleItems.value.length) return;
    const next = selectedIndex.value + delta;
    selectedIndex.value = Math.min(Math.max(next, 0), visibleItems.value.length - 1);
  }

  function setSelectedIndex(index: number) {
    selectedIndex.value = index;
  }

  function clampSelection() {
    if (!visibleItems.value.length) {
      selectedIndex.value = 0;
      return;
    }

    selectedIndex.value = Math.min(selectedIndex.value, visibleItems.value.length - 1);
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

    categoryItems.value = categoryItems.value.map((categoryItem) =>
      categoryItem.id === item.id ? (item as CategoryItem) : categoryItem,
    );
  }

  function originalClipId(item: ClipViewItem) {
    return item.collection === "history" ? item.id : item.clipSnapshotId;
  }

  function restoreCategorySelection(itemId: string | null) {
    if (!itemId) {
      clampSelection();
      return;
    }

    const index = visibleItems.value.findIndex((item) => item.collection === "category" && item.id === itemId);
    if (index >= 0) {
      selectedIndex.value = index;
      return;
    }

    clampSelection();
  }

  const visibleActions = computed(() => filterAutomations(automations.value, actionsQuery.value));

  async function loadAutomations() {
    automations.value = await ipasteApi.listAutomations();
  }

  async function createAutomation(input: AutomationInput) {
    await ipasteApi.createAutomation(input);
    await loadAutomations();
  }

  async function updateAutomation(id: string, input: AutomationInput) {
    await ipasteApi.updateAutomation(id, input);
    await loadAutomations();
  }

  async function deleteAutomation(id: string) {
    await ipasteApi.deleteAutomation(id);
    await loadAutomations();
  }

  async function runAutomation(id: string) {
    return await ipasteApi.runAutomation(id);
  }

  if (isTauri) {
    void listen<AutomationRunStartedEvent>(IPASTE_EVENTS.automationRunStarted, (event) => {
      const { automationId, runId, startedAt } = event.payload;
      const action = automations.value.find((entry) => entry.id === automationId);
      if (action) {
        action.lastRun = { id: runId, status: "running", startedAt, finishedAt: null, exitCode: null, durationMs: null };
      }
    });
    void listen<AutomationRunOutputEvent>(IPASTE_EVENTS.automationRunOutput, (event) => {
      const { runId, stream, chunk } = event.payload;
      const logs = runningAutomationLogs.value[runId] ?? { stdout: "", stderr: "" };
      const limit = 200 * 1024;
      if (stream === "stderr") logs.stderr = (logs.stderr + chunk).slice(-limit);
      else logs.stdout = (logs.stdout + chunk).slice(-limit);
      runningAutomationLogs.value = { ...runningAutomationLogs.value, [runId]: logs };
    });
    void listen<AutomationRunFinishedEvent>(IPASTE_EVENTS.automationRunFinished, (event) => {
      const { automationId, status, exitCode, finishedAt } = event.payload;
      const action = automations.value.find((entry) => entry.id === automationId);
      if (action?.lastRun) {
        action.lastRun = { ...action.lastRun, status, exitCode: exitCode ?? null, finishedAt };
      }
      if (action?.closePanelOnSuccess && status === "success") {
        closePanelRequested.value = true;
      }
    });
  }

  return {
    clips,
    categories,
    categoryItems,
    selectedCategoryId,
    selectedIndex,
    search,
    shortcut,
    isListening,
    isAppendCopyEnabled,
    isLoading,
    isLoadingMoreClips,
    hasMoreClips,
    fallbackGroups,
    clipTotalCount,
    visibleHistoryTotalCount,
    error,
    retentionDays,
    appendCopyTimeoutMinutes,
    panelOpenBehavior,
    panelLayout,
    ocrMode,
    language,
    cloud,
    activeCategory,
    visibleItems,
    selectedItem,
    load,
    reloadClips,
    loadMoreClips,
    bindEvents,
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
    updateRetentionDays,
    updateAppendCopyTimeout,
    updateShortcut,
    updatePanelOpenBehavior,
    updatePanelLayout,
    updateOcrMode,
    updateLanguage,
    saveCloudSettings,
    disableCloudSync,
    testCloudSettings,
    syncCloudNow,
    syncCloudInBackground,
    selectCategory,
    clearSearch,
    activatePanelDefault,
    moveSelection,
    setSelectedIndex,
    clampSelection,
    automations,
    selectedActionIndex,
    actionsQuery,
    runningAutomationLogs,
    closePanelRequested,
    visibleActions,
    loadAutomations,
    createAutomation,
    updateAutomation,
    deleteAutomation,
    runAutomation,
  };
});
