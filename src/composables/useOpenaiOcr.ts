import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useSettingsStore } from "../stores/settingsStore";
import { DEFAULT_OPENAI_OCR_PROMPTS } from "../stores/lib/settings";
import type { CloudOcrPromptMessage } from "../types";

/** OpenAI 兼容接口（通用云 OCR）配置表单：Base URL + 模型 + API Key + 自定义 Prompt 列表。 */
export function useOpenaiOcr() {
  const settings = useSettingsStore();
  const openaiBaseUrl = ref(settings.cloudOcr.openaiBaseUrl);
  const openaiModel = ref(settings.cloudOcr.openaiModel);
  const openaiApiKey = ref(settings.cloudOcr.openaiApiKey);
  const openaiPrompts = ref<CloudOcrPromptMessage[]>(
    settings.cloudOcr.openaiPrompts && settings.cloudOcr.openaiPrompts.length > 0
      ? settings.cloudOcr.openaiPrompts.map((item) => ({ ...item }))
      : DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item })),
  );
  const isPromptsExpanded = ref(false);
  const openaiMessage = ref<string | null>(null);
  const openaiError = ref<string | null>(null);
  const isTestingOpenai = ref(false);
  const isSavingOpenai = ref(false);

  const openaiConfigured = computed(() =>
    Boolean(settings.cloudOcr.openaiBaseUrl && settings.cloudOcr.openaiModel && settings.cloudOcr.openaiApiKey),
  );

  const openaiStatusText = computed(() =>
    openaiConfigured.value ? t("settings.openai.configured") : t("settings.openai.notConfigured"),
  );

  function syncFormFromStore() {
    openaiBaseUrl.value = settings.cloudOcr.openaiBaseUrl;
    openaiModel.value = settings.cloudOcr.openaiModel;
    openaiApiKey.value = settings.cloudOcr.openaiApiKey;
    openaiPrompts.value =
      settings.cloudOcr.openaiPrompts && settings.cloudOcr.openaiPrompts.length > 0
        ? settings.cloudOcr.openaiPrompts.map((item) => ({ ...item }))
        : DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
  }

  function resetOpenaiForm() {
    syncFormFromStore();
    openaiMessage.value = null;
    openaiError.value = null;
  }

  // 快照在父组件 onMounted 装载；watch 让表单跟随已加载的 settings.cloudOcr，
  // 加上 immediate: true 确保在子组件（tab）晚于装载挂载时也能立即读入已保存配置。
  watch(() => settings.cloudOcr, () => syncFormFromStore(), { deep: true, immediate: true });

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
      await settings.testOpenaiOcr(
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
      await settings.saveOpenaiOcrConfig(
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
      await settings.clearOpenaiOcrConfig();
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
