import { onMounted, ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";

const isTauri = "__TAURI_INTERNALS__" in window;

export function useAutostart() {
  const autostartEnabled = ref(false);
  const isTogglingAutostart = ref(false);
  const autostartError = ref<string | null>(null);

  async function loadAutostartStatus() {
    if (!isTauri) return;
    try {
      autostartEnabled.value = await ipasteApi.isAutostartEnabled();
    } catch (unknownError) {
      autostartError.value = String(unknownError);
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
      autostartError.value = String(unknownError);
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
