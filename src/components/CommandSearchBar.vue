<script setup lang="ts">
import { computed, ref } from "vue";
import { ClipboardPlus, Download, ScanText, Search, Settings, Wifi, X } from "lucide-vue-next";
import { t } from "../i18n";
import { isMacOs } from "../lib/env";
import { ipasteApi } from "../lib/ipasteApi";
import { showError } from "../stores/uiStore";
import { useWindowDrag } from "../composables/useWindowDrag";

const logoUrl = new URL("../../src-tauri/icons/32x32.png", import.meta.url).href;

defineProps<{
  searchQuery: string;
  shortcut: string;
  settingsOpen: boolean;
  appendCopyEnabled: boolean;
  appendCopyTimeoutMinutes: number;
  hasUpdate?: boolean;
  checkingUpdate?: boolean;
}>();

const emit = defineEmits<{
  "update:searchQuery": [value: string];
  toggleSettings: [];
  toggleAppendCopy: [];
  openUpdate: [];
  close: [];
}>();

const searchInputRef = ref<HTMLInputElement | null>(null);
const isSearchFocused = ref(false);

const { startWindowDrag } = useWindowDrag({ mainWindow: true });

const searchShortcutHint = computed(() => (isMacOs ? "⌘F" : "Ctrl+F"));

// 这两条命令此前是裸的 void：失败时点按钮毫无反应。
// 设备同步尤其糟——面板一失焦就自动隐藏，用户只看到「面板没了、窗口也没出现」，
// 没有任何线索说明是命令失败了。与 ipasteStore 的动作失败一致走瞬态 toast。
function onLanSync() {
  void ipasteApi.openLanSync().catch(showError);
}

function onScreenshotOcr() {
  void ipasteApi.startScreenshotOcr().catch(showError);
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

    <!-- 分类的选中/新建/重命名/改色/删除/拖拽排序统一由 CategoryRail 承担：
         这里曾并存一份只读胶囊，既重复又没有管理操作。 -->
  </div>
</template>
