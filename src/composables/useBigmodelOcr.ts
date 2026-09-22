import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";

export function useBigmodelOcr() {
  const store = useIpasteStore();
  const bigmodelApiKey = ref("");
  const bigmodelMessage = ref<string | null>(null);
  const bigmodelError = ref<string | null>(null);
  const isTestingBigmodel = ref(false);
  const isSavingBigmodel = ref(false);

  const bigmodelStatusText = computed(() =>
    store.cloudOcr.bigmodelApiKey ? t("settings.bigmodel.configured") : t("settings.bigmodel.notConfigured"),
  );

  function resetBigmodelForm() {
    bigmodelApiKey.value = store.cloudOcr.bigmodelApiKey;
    bigmodelMessage.value = null;
    bigmodelError.value = null;
  }

  // store.load() 在父组件 onMounted 完成；watch 让表单跟随已加载的 store.cloudOcr
  // （与 useCloudSync 同法，规避子父挂载时序）。
  watch(() => store.cloudOcr, () => resetBigmodelForm(), { deep: true });

  async function testBigmodel() {
    bigmodelMessage.value = null;
    bigmodelError.value = null;
    isTestingBigmodel.value = true;
    try {
      await store.testBigmodelOcr(bigmodelApiKey.value);
      bigmodelMessage.value = t("settings.bigmodel.connected");
    } catch (unknownError) {
      bigmodelError.value = errorMessage(unknownError);
    } finally {
      isTestingBigmodel.value = false;
    }
  }

  async function saveBigmodelKey() {
    bigmodelMessage.value = null;
    bigmodelError.value = null;
    isSavingBigmodel.value = true;
    try {
      await store.saveBigmodelApiKey(bigmodelApiKey.value);
      bigmodelMessage.value = t("settings.bigmodel.saved");
    } catch (unknownError) {
      bigmodelError.value = errorMessage(unknownError);
    } finally {
      isSavingBigmodel.value = false;
    }
  }

  async function clearBigmodelKey() {
    bigmodelMessage.value = null;
    bigmodelError.value = null;
    isSavingBigmodel.value = true;
    try {
      await store.clearBigmodelApiKey();
      resetBigmodelForm();
      bigmodelMessage.value = t("settings.bigmodel.cleared");
    } catch (unknownError) {
      bigmodelError.value = errorMessage(unknownError);
    } finally {
      isSavingBigmodel.value = false;
    }
  }

  return {
    bigmodelApiKey,
    bigmodelMessage,
    bigmodelError,
    isTestingBigmodel,
    isSavingBigmodel,
    bigmodelStatusText,
    testBigmodel,
    saveBigmodelKey,
    clearBigmodelKey,
  };
}
