import { onMounted, ref } from "vue";
import { errorMessage } from "../lib/appError";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";

/** LAN 同步传输设置：自定义中继 URL 与自动推送开关的读写（round-trip 回填、失败回滚）。 */
export function useSyncTransportSettings() {
  const relayInput = ref("");
  const relaySaved = ref(false);
  const relayError = ref<string | null>(null);
  const savingRelay = ref(false);
  const autoPushMaster = ref(true);
  const autoPushNotify = ref(false);
  const autoPushError = ref<string | null>(null);

  onMounted(async () => {
    if (!isTauri) return;
    try {
      const settings = await ipasteApi.syncTransportSettingsGet();
      relayInput.value = settings.relayUrl ?? "";
    } catch {
      // 读取失败保持空输入（= n0 默认中继），不打断面板。
    }
    try {
      const autoPush = await ipasteApi.syncAutoPushSettingsGet();
      autoPushMaster.value = autoPush.master;
      autoPushNotify.value = autoPush.notify;
    } catch {
      // 读取失败保持缺省开关，不打断面板。
    }
  });

  async function saveRelay() {
    relayError.value = null;
    relaySaved.value = false;
    savingRelay.value = true;
    try {
      const normalized = relayInput.value.trim();
      const result = await ipasteApi.syncTransportSettingsSet(normalized ? normalized : null);
      relayInput.value = result.relayUrl ?? "";
      relaySaved.value = true;
    } catch (unknownError) {
      relayError.value = errorMessage(unknownError);
    } finally {
      savingRelay.value = false;
    }
  }

  // 开关即改即存：以落库返回值回填（round-trip）；失败时回滚到改前状态并提示。
  async function saveAutoPush() {
    autoPushError.value = null;
    const previous = { master: autoPushMaster.value, notify: autoPushNotify.value };
    try {
      const saved = await ipasteApi.syncAutoPushSettingsSet(autoPushMaster.value, autoPushNotify.value);
      autoPushMaster.value = saved.master;
      autoPushNotify.value = saved.notify;
    } catch (unknownError) {
      autoPushMaster.value = previous.master;
      autoPushNotify.value = previous.notify;
      autoPushError.value = errorMessage(unknownError);
    }
  }

  return {
    relayInput, relaySaved, relayError, savingRelay,
    autoPushMaster, autoPushNotify, autoPushError,
    saveRelay, saveAutoPush,
  };
}
