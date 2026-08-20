<script setup lang="ts">
import { AlertCircle, CheckCircle2, Keyboard, RotateCcw, Save, ScanText } from "lucide-vue-next";
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
} = useShortcutRecorder("panel");

const {
  shortcutRecording: ocrRecording,
  shortcutMessage: ocrMessage,
  shortcutError: ocrError,
  isSavingShortcut: isSavingOcrShortcut,
  formattedShortcutDraft: formattedOcrDraft,
  canSaveShortcut: canSaveOcrShortcut,
  startRecordingShortcut: startRecordingOcrShortcut,
  saveShortcut: saveOcrShortcut,
  restoreDefaultShortcut: restoreDefaultOcrShortcut,
} = useShortcutRecorder("ocr");
</script>

<template>
  <div class="settings-section">
    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-teal">
          <Keyboard class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.shortcuts.global.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.shortcuts.global.description") }}
          </p>
        </div>
      </div>

      <div class="settings-shortcut-recorder">
        <button
          type="button"
          class="shortcut-capture-button"
          :class="{
            'shortcut-capture-button-recording': shortcutRecording,
            'shortcut-recording-glow': shortcutRecording,
          }"
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
        <div class="settings-icon settings-icon-teal">
          <ScanText class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.shortcuts.ocr.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
            {{ t("settings.shortcuts.ocr.description") }}
          </p>
        </div>
      </div>

      <div class="settings-shortcut-recorder">
        <button
          type="button"
          class="shortcut-capture-button"
          :class="{
            'shortcut-capture-button-recording': ocrRecording,
            'shortcut-recording-glow': ocrRecording,
          }"
          :aria-pressed="ocrRecording"
          @click="startRecordingOcrShortcut"
        >
          <Keyboard class="size-4" />
          <span>{{ ocrRecording ? t("settings.shortcuts.recording") : formattedOcrDraft }}</span>
        </button>

        <button
          type="button"
          class="settings-action-button"
          :disabled="isSavingOcrShortcut"
          @click="restoreDefaultOcrShortcut"
        >
          <RotateCcw class="size-4" />
          <span>{{ t("settings.shortcuts.restoreDefault") }}</span>
        </button>

        <button
          type="button"
          class="settings-action-button settings-action-button-primary"
          :disabled="!canSaveOcrShortcut"
          @click="saveOcrShortcut"
        >
          <Save class="size-4" />
          <span>{{ isSavingOcrShortcut ? t("common.saving") : t("common.save") }}</span>
        </button>
      </div>

      <p
        v-if="ocrError || ocrMessage"
        class="settings-message"
        :class="{ 'settings-message-error': ocrError }"
      >
        <CheckCircle2
          v-if="ocrMessage && !ocrError"
          class="size-4"
        />
        <AlertCircle
          v-else
          class="size-4"
        />
        <span>{{ ocrError || ocrMessage }}</span>
      </p>
    </section>

    <section class="settings-panel settings-column-panel">
      <div class="settings-panel-heading">
        <div class="settings-icon settings-icon-blue">
          <Keyboard class="size-5" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="text-sm font-semibold text-[var(--text-1)]">
            {{ t("settings.shortcuts.panel.title") }}
          </h2>
          <p class="mt-1 text-sm text-[var(--text-2)]">
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
