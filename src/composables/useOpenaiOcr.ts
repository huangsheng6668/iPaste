import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";
import { DEFAULT_OPENAI_OCR_PROMPTS } from "../stores/lib/settings";
import type { CloudOcrPromptMessage } from "../types";

/** OpenAI 兼容接口（通用云 OCR）配置表单：Base URL + 模型 + API Key + 自定义 Prompt 列表。 */
export function useOpenaiOcr() {
  const store = useIpasteStore();
  const openaiBaseUrl = ref("");
  const openaiModel = ref("");
  const openaiApiKey = ref("");
  const openaiPrompts = ref<CloudOcrPromptMessage[]>([]);
  const isPromptsExpanded = ref(false);
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
    openaiPrompts.value =
      store.cloudOcr.openaiPrompts && store.cloudOcr.openaiPrompts.length > 0
        ? store.cloudOcr.openaiPrompts.map((item) => ({ ...item }))
        : DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
    openaiMessage.value = null;
    openaiError.value = null;
  }

  // store.load() 在父组件 onMounted 完成；watch 让表单跟随已加载的 store.cloudOcr
  // （与 useCloudSync 同法，规避子父挂载时序）。
  watch(() => store.cloudOcr, () => resetOpenaiForm(), { deep: true });

  const formComplete = computed(() =>
    Boolean(openaiBaseUrl.value.trim() && openaiModel.value.trim() && openaiApiKey.value.trim()),
  );

  function addPrompt(role: "system" | "user" = "user") {
    openaiPrompts.value.push({ role, content: "" });
  }

  function removePrompt(index: number) {
    if (openaiPrompts.value.length <= 1) return;
    openaiPrompts.value.splice(index, 1);
  }

  function restoreDefaultPrompts() {
    openaiPrompts.value = DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
  }

  async function testOpenai() {
    openaiMessage.value = null;
    openaiError.value = null;
    isTestingOpenai.value = true;
    try {
      await store.testOpenaiOcr(
        openaiBaseUrl.value,
        openaiModel.value,
        openaiApiKey.value,
        openaiPrompts.value,
      );
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
      await store.saveOpenaiOcrConfig(
        openaiBaseUrl.value,
        openaiModel.value,
        openaiApiKey.value,
        openaiPrompts.value,
      );
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
    openaiPrompts,
    isPromptsExpanded,
    openaiMessage,
    openaiError,
    isTestingOpenai,
    isSavingOpenai,
    openaiConfigured,
    openaiStatusText,
    formComplete,
    addPrompt,
    removePrompt,
    restoreDefaultPrompts,
    testOpenai,
    saveOpenaiConfig,
    clearOpenaiConfig,
  };
}
