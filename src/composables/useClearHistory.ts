import { ref } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";

export function useClearHistory() {
  const store = useIpasteStore();
  const isClearingHistory = ref(false);
  const confirmingClearHistory = ref(false);
  const storageMessage = ref<string | null>(null);
  const storageError = ref<string | null>(null);

  function requestClearHistory() {
    storageMessage.value = null;
    storageError.value = null;
    confirmingClearHistory.value = true;
  }

  function cancelClearHistory() {
    confirmingClearHistory.value = false;
  }

  async function confirmClearHistory() {
    if (isClearingHistory.value) return;
    isClearingHistory.value = true;
    storageMessage.value = null;
    storageError.value = null;
    try {
      const deleted = await store.clearHistory();
      confirmingClearHistory.value = false;
      storageMessage.value = t("settings.storage.cleared", { count: deleted });
    } catch (unknownError) {
      storageError.value = errorMessage(unknownError);
    } finally {
      isClearingHistory.value = false;
    }
  }

  return {
    isClearingHistory,
    confirmingClearHistory,
    storageMessage,
    storageError,
    requestClearHistory,
    cancelClearHistory,
    confirmClearHistory,
  };
}
