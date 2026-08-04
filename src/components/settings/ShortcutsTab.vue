<script setup lang="ts">
import { AlertCircle, CheckCircle2, Keyboard, RotateCcw, Save } from "lucide-vue-next";
import { t } from "../../i18n";
import { useShortcutRecorder } from "../../composables/useShortcutRecorder";

const {
  shortcutRecording,
  shortcutMessage,
  shortcutError,
  isSavingShortcut,
  formattedShortcutDraft,
  canSaveShortcut,
  fixedShortcuts,
  startRecordingShortcut,
  saveShortcut,
  restoreDefaultShortcut,
} = useShortcutRecorder();
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-teal">
          <Keyboard class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-slate-950">
            {{ t("settings.shortcuts.global.title") }}
          </h2>
          <p class="mt-1 text-sm text-slate-500">
            {{ t("settings.shortcuts.global.description") }}
          </p>
        </div>
      </div>

      <div class="settings-shortcut-recorder">
        <button
          type="button"
          class="shortcut-capture-button"
          :class="{ 'shortcut-capture-button-recording': shortcutRecording }"
          :aria-pressed="shortcutRecording"
          @click="startRecordingShortcut"
        >
          <Keyboard class="size-4" />
          <span>{{ shortcutRecording ? t("settings.shortcuts.recording") : formattedShortcutDraft }}</span>
        </button>

        <button
          type="button"
          class="settings-action-button"
          :disabled="isSavingShortcut"
          @click="restoreDefaultShortcut"
        >
          <RotateCcw class="size-4" />
          <span>{{ t("settings.shortcuts.restoreDefault") }}</span>
        </button>

        <button
          type="button"
          class="settings-action-button settings-action-button-primary"
          :disabled="!canSaveShortcut"
          @click="saveShortcut"
        >
          <Save class="size-4" />
          <span>{{ isSavingShortcut ? t("common.saving") : t("common.save") }}</span>
        </button>
      </div>

      <p
        v-if="shortcutError || shortcutMessage"
        class="settings-message"
        :class="{ 'settings-message-error': shortcutError }"
      >
        <CheckCircle2
          v-if="shortcutMessage && !shortcutError"
          class="size-4"
        />
        <AlertCircle
          v-else
          class="size-4"
        />
        <span>{{ shortcutError || shortcutMessage }}</span>
      </p>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-blue">
          <Keyboard class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-slate-950">
            {{ t("settings.shortcuts.panel.title") }}
          </h2>
          <p class="mt-1 text-sm text-slate-500">
            {{ t("settings.shortcuts.panel.description") }}
          </p>
        </div>
      </div>

      <div class="settings-shortcut-list">
        <div
          v-for="shortcut in fixedShortcuts"
          :key="shortcut.action"
          class="settings-shortcut-row"
        >
          <div
            class="shortcut-kbd-group"
            aria-hidden="true"
          >
            <kbd
              v-for="key in shortcut.keys"
              :key="key"
              class="shortcut-kbd"
            >{{ key }}</kbd>
          </div>
          <span>{{ shortcut.action }}</span>
        </div>
      </div>
    </section>
  </div>
</template>
