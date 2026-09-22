import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";

/** OpenAI 兼容接口（通用云 OCR）配置表单：Base URL + 模型 + API Key。 */
export function useOpenaiOcr() {
  const store = useIpasteStore();
  const openaiBaseUrl = ref("");
  const openaiModel = ref("");
  const openaiApiKey = ref("");
  const openaiMessage = ref<string | null>(null);
  const openaiError = ref<string | null>(null);
  const isTestingOpenai = ref(false);
  const isSavingOpenai = ref(false);

  const openaiConfigured = computed(() =>
    Boolean(store.cloudOcr.openaiBaseUrl && store.cloudOcr.openaiModel && store.cloudOcr.openaiApiKey),
  );

  const openaiStatusText = computed(() =>
    openaiConfigured.value ? t("settings.openai.configured") : t("settings.openai.notConfigured"),
  );

  function resetOpenaiForm() {
    openaiBaseUrl.value = store.cloudOcr.openaiBaseUrl;
    openaiModel.value = store.cloudOcr.openaiModel;
    openaiApiKey.value = store.cloudOcr.openaiApiKey;
    openaiMessage.value = null;
    openaiError.value = null;
  }

  // store.load() 在父组件 onMounted 完成；watch 让表单跟随已加载的 store.cloudOcr
  // （与 useCloudSync 同法，规避子父挂载时序）。
  watch(() => store.cloudOcr, () => resetOpenaiForm(), { deep: true });

  const formComplete = computed(() =>
    Boolean(openaiBaseUrl.value.trim() && openaiModel.value.trim() && openaiApiKey.value.trim()),
  );

  async function testOpenai() {
    openaiMessage.value = null;
    openaiError.value = null;
    isTestingOpenai.value = true;
    try {
      await store.testOpenaiOcr(openaiBaseUrl.value, openaiModel.value, openaiApiKey.value);
      openaiMessage.value = t("settings.openai.connected");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isTestingOpenai.value = false;
    }
  }

  async function saveOpenaiConfig() {
    openaiMessage.value = null;
    openaiError.value = null;
    isSavingOpenai.value = true;
    try {
      await store.saveOpenaiOcrConfig(openaiBaseUrl.value, openaiModel.value, openaiApiKey.value);
      openaiMessage.value = t("settings.openai.saved");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  async function clearOpenaiConfig() {
    openaiMessage.value = null;
    openaiError.value = null;
    isSavingOpenai.value = true;
    try {
      await store.clearOpenaiOcrConfig();
      resetOpenaiForm();
      openaiMessage.value = t("settings.openai.cleared");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  return {
    openaiBaseUrl,
    openaiModel,
    openaiApiKey,
    openaiMessage,
    openaiError,
    isTestingOpenai,
    isSavingOpenai,
    openaiConfigured,
    openaiStatusText,
    formComplete,
    testOpenai,
    saveOpenaiConfig,
    clearOpenaiConfig,
  };
}
