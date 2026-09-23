import { computed, ref } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";
import { useTwoStepConfirm } from "./useTwoStepConfirm";

export function useClearHistory() {
  const store = useIpasteStore();
  const isClearingHistory = ref(false);
  // 清空历史两步确认：设置页场景不自动复位（timeoutMs=null），由取消/确认显式复位。
  const clearConfirm = useTwoStepConfirm(null);
  const confirmingClearHistory = computed(() => clearConfirm.isConfirming("clear"));
  const storageMessage = ref<string | null>(null);
  const storageError = ref<string | null>(null);

  function requestClearHistory() {
    storageMessage.value = null;
    storageError.value = null;
    clearConfirm.request("clear");
  }

  function cancelClearHistory() {
    clearConfirm.cancel();
  }

  async function confirmClearHistory() {
    if (isClearingHistory.value) return;
    isClearingHistory.value = true;
    storageMessage.value = null;
    storageError.value = null;
    try {
      const deleted = await store.clearHistory();
      clearConfirm.cancel();
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
