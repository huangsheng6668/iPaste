import { computed, onUnmounted, ref, watch } from "vue";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { formatShortcut } from "../lib/format";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";

const DEFAULT_SHORTCUT = "CommandOrControl+Shift+V";
const isMacOs = /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);

export function useShortcutRecorder() {
  const store = useIpasteStore();
  const shortcutDraft = ref(store.shortcut || DEFAULT_SHORTCUT);
  const shortcutRecording = ref(false);
  const shortcutMessage = ref<string | null>(null);
  const shortcutError = ref<string | null>(null);
  const isSavingShortcut = ref(false);
  let shouldRestoreAppShortcutAfterRecording = false;

  // store.load() 在父组件 onMounted 完成；用 watch 让 draft 跟随已加载的 store.shortcut，
  // 替代原先在 onMounted 里手动调用的 resetShortcutForm()，规避子父挂载时序。
  watch(
    () => store.shortcut,
    (value) => {
      shortcutDraft.value = value || DEFAULT_SHORTCUT;
    },
  );

  const formattedShortcutDraft = computed(() => formatShortcut(shortcutDraft.value || store.shortcut));
  const canSaveShortcut = computed(() =>
    Boolean(shortcutDraft.value && shortcutDraft.value !== store.shortcut && !isSavingShortcut.value),
  );
  const fixedShortcuts = computed(() => [
    { keys: [formatShortcut("CommandOrControl+F")], action: t("settings.shortcuts.focusSearch") },
    { keys: ["↑", "↓", "←", "→"], action: t("settings.shortcuts.moveCards") },
    { keys: [isMacOs ? "Cmd" : "Ctrl"], action: t("settings.shortcuts.quickPreview") },
    { keys: ["Enter"], action: t("settings.shortcuts.pasteSelected") },
    { keys: ["Esc"], action: t("settings.shortcuts.closePanelOrMenu") },
    { keys: [formatShortcut("CommandOrControl+1")], action: t("settings.shortcuts.switchHistory") },
    { keys: [formatShortcut("CommandOrControl+2")], action: t("settings.shortcuts.switchFirstCategory") },
    {
      keys: [`${formatShortcut("CommandOrControl+3")} ... ${formatShortcut("CommandOrControl+9")}`],
      action: t("settings.shortcuts.switchMoreCategories"),
    },
  ]);

  function resetShortcutForm() {
    shortcutDraft.value = store.shortcut || DEFAULT_SHORTCUT;
    shortcutMessage.value = null;
    shortcutError.value = null;
  }

  async function startRecordingShortcut() {
    if (shortcutRecording.value) return;
    if (!(await pauseAppShortcutWhileRecording())) return;
    shortcutRecording.value = true;
    shortcutMessage.value = null;
    shortcutError.value = null;
    window.addEventListener("keydown", handleShortcutRecording, { capture: true });
  }

  async function stopRecordingShortcut(options: { restoreAppShortcut?: boolean } = {}) {
    if (shortcutRecording.value) {
      shortcutRecording.value = false;
      window.removeEventListener("keydown", handleShortcutRecording, { capture: true });
    }
    if (options.restoreAppShortcut) {
      await restoreAppShortcutAfterRecording();
    }
  }

  function handleShortcutRecording(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();

    if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
      void stopRecordingShortcut({ restoreAppShortcut: true });
      return;
    }

    const shortcut = shortcutFromKeyboardEvent(event);
    if (!shortcut) {
      shortcutError.value = t("settings.shortcuts.invalid");
      return;
    }

    shortcutDraft.value = shortcut;
    shortcutError.value = null;
    void stopRecordingShortcut({ restoreAppShortcut: true });
  }

  async function pauseAppShortcutWhileRecording() {
    if (shouldRestoreAppShortcutAfterRecording) return true;

    try {
      await ipasteApi.setAppShortcutEnabled(false);
      shouldRestoreAppShortcutAfterRecording = true;
      return true;
    } catch (unknownError) {
      shortcutError.value = errorMessage(unknownError);
      return false;
    }
  }

  async function restoreAppShortcutAfterRecording() {
    if (!shouldRestoreAppShortcutAfterRecording) return;
    shouldRestoreAppShortcutAfterRecording = false;

    try {
      await ipasteApi.setAppShortcutEnabled(true);
    } catch (unknownError) {
      shouldRestoreAppShortcutAfterRecording = true;
      shortcutError.value = errorMessage(unknownError);
    }
  }

  function shortcutFromKeyboardEvent(event: KeyboardEvent) {
    const key = shortcutKeyFromEvent(event);
    if (!key) return "";

    const modifiers: string[] = [];
    if (event.metaKey) modifiers.push("Command");
    if (event.ctrlKey) modifiers.push("Control");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");

    if (!modifiers.length) return "";
    return [...modifiers, key].join("+");
  }

  function shortcutKeyFromEvent(event: KeyboardEvent) {
    const modifierKeys = new Set(["Shift", "Control", "Alt", "Meta", "Command"]);
    if (modifierKeys.has(event.key)) return "";

    if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
    if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(event.code)) return event.code;

    const specialKeys: Record<string, string> = {
      ArrowDown: "ArrowDown",
      ArrowLeft: "ArrowLeft",
      ArrowRight: "ArrowRight",
      ArrowUp: "ArrowUp",
      Backspace: "Backspace",
      Delete: "Delete",
      Enter: "Enter",
      Escape: "Escape",
      Home: "Home",
      End: "End",
      Insert: "Insert",
      PageUp: "PageUp",
      PageDown: "PageDown",
      Space: "Space",
      Tab: "Tab",
    };
    return specialKeys[event.code] ?? "";
  }

  async function saveShortcut() {
    await stopRecordingShortcut({ restoreAppShortcut: true });
    shortcutMessage.value = null;
    shortcutError.value = null;
    isSavingShortcut.value = true;
    try {
      await store.updateShortcut(shortcutDraft.value);
      shortcutDraft.value = store.shortcut;
      shortcutMessage.value = t("settings.shortcuts.saved");
    } catch (unknownError) {
      shortcutError.value = errorMessage(unknownError);
    } finally {
      isSavingShortcut.value = false;
    }
  }

  function restoreDefaultShortcut() {
    void stopRecordingShortcut({ restoreAppShortcut: true });
    shortcutDraft.value = DEFAULT_SHORTCUT;
    shortcutMessage.value = null;
    shortcutError.value = null;
  }

  onUnmounted(() => {
    void stopRecordingShortcut({ restoreAppShortcut: true });
  });

  return {
    shortcutDraft,
    shortcutRecording,
    shortcutMessage,
    shortcutError,
    isSavingShortcut,
    formattedShortcutDraft,
    canSaveShortcut,
    fixedShortcuts,
    resetShortcutForm,
    startRecordingShortcut,
    stopRecordingShortcut,
    handleShortcutRecording,
    saveShortcut,
    restoreDefaultShortcut,
  };
}
