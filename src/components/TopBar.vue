<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ClipboardPlus, Download, Search, Settings, Wifi, X } from "lucide-vue-next";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";

const logoUrl = new URL("../../src-tauri/icons/32x32.png", import.meta.url).href;

defineProps<{
  modelValue: string;
  shortcut: string;
  settingsOpen: boolean;
  appendCopyEnabled: boolean;
  appendCopyTimeoutMinutes: number;
  hasUpdate?: boolean;
  checkingUpdate?: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
  toggleSettings: [];
  toggleAppendCopy: [];
  openUpdate: [];
  close: [];
}>();

let dragReleaseTimer: number | null = null;

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
</script>

<template>
  <header class="top-bar">
    <div
      class="top-bar-drag-zone"
      @mousedown="startWindowDrag"
    >
      <img
        class="top-bar-logo"
        :src="logoUrl"
        alt=""
      >
      <span class="top-bar-brand">iPaste</span>
    </div>

    <div
      class="top-bar-spacer"
      aria-hidden="true"
      @mousedown="startWindowDrag"
    />

    <label class="search-box">
      <Search class="search-box-icon size-4 shrink-0" />
      <input
        class="min-w-0 flex-1 bg-transparent text-sm outline-none"
        :value="modelValue"
        tabindex="-1"
        :placeholder="t('topBar.searchPlaceholder')"
        spellcheck="false"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      >
      <span
        class="search-kbd"
        aria-hidden="true"
      >
        {{ shortcut }}
      </span>
    </label>

    <button
      v-if="hasUpdate"
      type="button"
      class="icon-button update-icon-button"
      :class="{ 'update-icon-button-checking': checkingUpdate }"
      tabindex="-1"
      :aria-label="t('topBar.openUpdate')"
      :data-tooltip="t('topBar.openUpdate')"
      @click.stop="emit('openUpdate')"
    >
      <Download class="size-4" />
    </button>

    <button
      type="button"
      class="icon-button append-copy-button"
      :class="{ 'append-copy-button-active': appendCopyEnabled }"
      tabindex="-1"
      :aria-pressed="appendCopyEnabled"
      :aria-label="appendCopyEnabled ? t('appendCopy.disable') : t('appendCopy.enable')"
      :data-tooltip="appendCopyEnabled ? t('appendCopy.disable') : t('appendCopy.enableTooltip', { minutes: appendCopyTimeoutMinutes })"
      @click.stop="emit('toggleAppendCopy')"
    >
      <ClipboardPlus class="size-4" />
    </button>

    <button
      type="button"
      class="icon-button"
      tabindex="-1"
      :aria-label="t('lan.title')"
      :data-tooltip="t('lan.title')"
      @click.stop="onLanSync"
    >
      <Wifi class="size-4" />
    </button>

    <button
      type="button"
      class="icon-button"
      :class="{ 'icon-button-active': settingsOpen }"
      tabindex="-1"
      :aria-label="t('topBar.openSettings')"
      :data-tooltip="t('topBar.openSettings')"
      @click.stop="emit('toggleSettings')"
    >
      <Settings class="size-4" />
    </button>

    <button
      type="button"
      class="icon-button"
      tabindex="-1"
      :aria-label="t('topBar.closePanel')"
      :data-tooltip="t('topBar.closePanel')"
      @click="emit('close')"
    >
      <X class="size-4" />
    </button>
  </header>
</template>
