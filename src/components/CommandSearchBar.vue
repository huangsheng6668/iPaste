<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ClipboardPlus,
  Clock,
  Download,
  Plus,
  ScanText,
  Search,
  Settings,
  Wifi,
  X,
  Zap,
} from "lucide-vue-next";
import { t } from "../i18n";
import { categoryDisplayName } from "../lib/format";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";
import type { Category } from "../types";

const logoUrl = new URL("../../src-tauri/icons/32x32.png", import.meta.url).href;

const props = defineProps<{
  searchQuery: string;
  shortcut: string;
  categories: Category[];
  selectedCategoryId: string;
  editingCategoryId: string | null;
  historyCount: number;
  categoryCounts: Record<string, number>;
  settingsOpen: boolean;
  appendCopyEnabled: boolean;
  appendCopyTimeoutMinutes: number;
  hasUpdate?: boolean;
  checkingUpdate?: boolean;
}>();

const emit = defineEmits<{
  "update:searchQuery": [value: string];
  selectCategory: [id: string];
  createCategory: [];
  editCategory: [id: string];
  renameCategory: [category: Category, name: string];
  recolorCategory: [category: Category, color: string];
  deleteCategory: [id: string];
  toggleSettings: [];
  toggleAppendCopy: [];
  openUpdate: [];
  close: [];
}>();

const searchInputRef = ref<HTMLInputElement | null>(null);
const tabsContainerRef = ref<HTMLElement | null>(null);
const isSearchFocused = ref(false);
const hasScrollLeft = ref(false);
const hasScrollRight = ref(false);
const editingName = ref("");
let dragReleaseTimer: number | null = null;

const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
const searchShortcutHint = computed(() => (isMacOs ? "⌘F" : "Ctrl+F"));

function updateScrollState() {
  const el = tabsContainerRef.value;
  if (!el) return;
  hasScrollLeft.value = el.scrollLeft > 2;
  hasScrollRight.value = el.scrollLeft + el.clientWidth < el.scrollWidth - 2;
}

watch(
  () => props.editingCategoryId,
  async (id) => {
    if (!id) return;
    const category = props.categories.find((item) => item.id === id);
    editingName.value = category ? categoryDisplayName(category.name) : "";
    await nextTick();
  },
);

watch(
  () => props.categories,
  () => {
    void nextTick(updateScrollState);
  },
  { deep: true },
);

onMounted(() => {
  updateScrollState();
  window.addEventListener("resize", updateScrollState);
});

onUnmounted(() => {
  window.removeEventListener("resize", updateScrollState);
});

async function startWindowDrag(event: MouseEvent) {
  if (!isTauri || event.button !== 0) return;
  event.preventDefault();
  if (dragReleaseTimer !== null) {
    window.clearTimeout(dragReleaseTimer);
    dragReleaseTimer = null;
  }
  void setMainWindowDragging(true);
  try {
    const nativeDragStarted = await startMainWindowDrag();
    if (!nativeDragStarted) {
      await getCurrentWindow().startDragging();
    }
  } finally {
    dragReleaseTimer = window.setTimeout(() => {
      void setMainWindowDragging(false);
      dragReleaseTimer = null;
    }, 900);
  }
}

function setMainWindowDragging(dragging: boolean) {
  return invoke("set_main_window_dragging", { dragging }).catch(() => {});
}

function startMainWindowDrag() {
  return invoke<boolean>("start_main_window_drag").catch(() => false);
}

function onLanSync() {
  void ipasteApi.openLanSync();
}

function onScreenshotOcr() {
  void ipasteApi.startScreenshotOcr();
}

function clearSearch() {
  emit("update:searchQuery", "");
  searchInputRef.value?.focus();
}

defineExpose({
  focusSearch: () => searchInputRef.value?.focus(),
});
</script>

<template>
  <div class="raycast-top-section">
    <!-- Top Search & Drag Bar -->
    <div
      class="raycast-search-row"
      @mousedown="startWindowDrag"
    >
      <div class="flex items-center gap-2">
        <img
          class="size-6 shrink-0 rounded-md shadow-sm select-none"
          :src="logoUrl"
          alt=""
        >
      </div>

      <!-- Search Box Input -->
      <div
        class="raycast-search-input-wrap"
        @mousedown.stop
      >
        <Search class="size-4 shrink-0 text-[var(--text-3)] transition-colors" />
        <input
          ref="searchInputRef"
          class="raycast-search-input"
          :value="searchQuery"
          :placeholder="t('topBar.searchPlaceholder')"
          spellcheck="false"
          @focus="isSearchFocused = true"
          @blur="isSearchFocused = false"
          @input="emit('update:searchQuery', ($event.target as HTMLInputElement).value)"
        >
        <button
          v-if="searchQuery"
          type="button"
          class="inline-flex size-4 items-center justify-center rounded-full text-[var(--text-3)] hover:text-[var(--text-1)] transition-colors"
          @click="clearSearch"
        >
          <X class="size-3" />
        </button>
        <kbd
          v-else-if="!isSearchFocused"
          class="keyboard-kbd text-[0.625rem] opacity-60 pointer-events-none select-none"
        >{{ searchShortcutHint }}</kbd>
      </div>

      <!-- Quick Action Buttons -->
      <div
        class="flex items-center gap-1"
        @mousedown.stop
      >
        <button
          v-if="hasUpdate"
          type="button"
          class="icon-button update-icon-button"
          :class="{ 'update-icon-button-checking': checkingUpdate }"
          :aria-label="t('topBar.openUpdate')"
          :data-tooltip="t('topBar.openUpdate')"
          @click="emit('openUpdate')"
        >
          <Download class="size-3.5" />
        </button>

        <button
          type="button"
          class="icon-button append-copy-button"
          :class="{ 'append-copy-button-active': appendCopyEnabled }"
          :aria-label="appendCopyEnabled ? t('appendCopy.disable') : t('appendCopy.enable')"
          :data-tooltip="appendCopyEnabled ? t('appendCopy.disable') : t('appendCopy.enableTooltip', { minutes: appendCopyTimeoutMinutes })"
          @click="emit('toggleAppendCopy')"
        >
          <ClipboardPlus class="size-3.5" />
        </button>

        <button
          type="button"
          class="icon-button"
          :aria-label="t('topBar.screenshotOcr')"
          :data-tooltip="t('topBar.screenshotOcr')"
          @click="onScreenshotOcr"
        >
          <ScanText class="size-3.5" />
        </button>

        <button
          type="button"
          class="icon-button"
          :aria-label="t('deviceSync.title')"
          :data-tooltip="t('deviceSync.title')"
          @click="onLanSync"
        >
          <Wifi class="size-3.5" />
        </button>

        <button
          type="button"
          class="icon-button"
          :class="{ 'icon-button-active': settingsOpen }"
          :aria-label="t('topBar.openSettings')"
          :data-tooltip="t('topBar.openSettings')"
          @click="emit('toggleSettings')"
        >
          <Settings class="size-3.5" />
        </button>

        <button
          type="button"
          class="icon-button"
          :aria-label="t('topBar.closePanel')"
          :data-tooltip="t('topBar.closePanel')"
          @click="emit('close')"
        >
          <X class="size-3.5" />
        </button>
      </div>
    </div>

    <!-- Category Pill Filter Tabs Wrapper with Fade Mask -->
    <div
      class="raycast-filter-tabs-wrapper"
      :class="{
        'has-scroll-left': hasScrollLeft,
        'has-scroll-right': hasScrollRight,
      }"
      @mousedown.stop
    >
      <div
        ref="tabsContainerRef"
        class="raycast-filter-tabs"
        @scroll="updateScrollState"
      >
        <!-- All History Tab -->
        <button
          type="button"
          class="raycast-pill-tab"
          :class="{ 'raycast-pill-tab-active': selectedCategoryId === 'history' }"
          @click="emit('selectCategory', 'history')"
        >
          <Clock class="size-3" />
          <span>{{ t("category.history") }}</span>
          <span
            v-if="historyCount > 0"
            class="text-[0.625rem] opacity-75 tabular-nums font-mono"
          >{{ historyCount }}</span>
        </button>

        <!-- Custom Categories -->
        <button
          v-for="cat in categories"
          :key="cat.id"
          type="button"
          class="raycast-pill-tab"
          :class="{ 'raycast-pill-tab-active': selectedCategoryId === cat.id }"
          @click="emit('selectCategory', cat.id)"
        >
          <span
            class="size-2 rounded-full shrink-0"
            :style="{
              backgroundColor: cat.color,
              boxShadow: `0 0 6px ${cat.color}90`,
            }"
          />
          <span>{{ categoryDisplayName(cat.name) }}</span>
          <span
            v-if="categoryCounts[cat.id]"
            class="text-[0.625rem] opacity-75 tabular-nums font-mono"
          >{{ categoryCounts[cat.id] }}</span>
        </button>

        <!-- Automation Tab -->
        <button
          type="button"
          class="raycast-pill-tab"
          :class="{ 'raycast-pill-tab-active': selectedCategoryId === 'automation' }"
          @click="emit('selectCategory', 'automation')"
        >
          <Zap class="size-3" />
          <span>{{ t("automation.entry") }}</span>
        </button>

        <!-- Add Category Button -->
        <button
          type="button"
          class="raycast-pill-tab hover:text-[var(--accent)]"
          :aria-label="t('category.newCategory')"
          :data-tooltip="t('category.newCategory')"
          @click="emit('createCategory')"
        >
          <Plus class="size-3" />
        </button>
      </div>
    </div>
  </div>
</template>
