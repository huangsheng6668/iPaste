import { onMounted, ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";
import { errorMessage } from "../lib/appError";
import { isTauri } from "../lib/env";

export function useAutostart() {
  const autostartEnabled = ref(false);
  const isTogglingAutostart = ref(false);
  const autostartError = ref<string | null>(null);

  async function loadAutostartStatus() {
    if (!isTauri) return;
    try {
      autostartEnabled.value = await ipasteApi.isAutostartEnabled();
    } catch (unknownError) {
      autostartError.value = errorMessage(unknownError);
    }
  }

  async function toggleAutostart() {
    if (isTogglingAutostart.value) return;
    autostartError.value = null;
    isTogglingAutostart.value = true;
    try {
      autostartEnabled.value = autostartEnabled.value
        ? await ipasteApi.disableAutostart()
        : await ipasteApi.enableAutostart();
    } catch (unknownError) {
      autostartError.value = errorMessage(unknownError);
    } finally {
      isTogglingAutostart.value = false;
    }
  }

  onMounted(() => {
    void loadAutostartStatus();
  });

  return {
    autostartEnabled,
    isTogglingAutostart,
    autostartError,
    toggleAutostart,
  };
}
