import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useSettingsStore } from "../stores/settingsStore";
import {
  DEFAULT_OPENAI_OCR_PROMPTS,
  activeOpenaiOcrProfile,
  cleanCloudOcrSettings,
  isOpenaiOcrConfigured,
} from "../stores/lib/settings";
import type { AppSettings, CloudOcrPromptMessage, OpenaiOcrProfile } from "../types";

/** OpenAI 兼容接口（通用云 OCR）配置：多接口列表 + 当前编辑表单 + 共用 Prompt 列表。 */
export function useOpenaiOcr() {
  const settings = useSettingsStore();
  const normalizedCloudOcr = computed(() => cleanCloudOcrSettings(settings.cloudOcr));
  const profiles = computed(() => normalizedCloudOcr.value.openaiProfiles);
  const activeProfile = computed(() => activeOpenaiOcrProfile(normalizedCloudOcr.value));

  const selectedProfileId = ref<string | null>(null);
  const isCreatingNew = ref(false);
  const profileName = ref("");
  const openaiBaseUrl = ref("");
  const openaiModel = ref("");
  const openaiApiKey = ref("");
  const openaiPrompts = ref<CloudOcrPromptMessage[]>(
    normalizedCloudOcr.value.openaiPrompts.map((item) => ({ ...item })),
  );
  const isPromptsExpanded = ref(false);
  const openaiMessage = ref<string | null>(null);
  const openaiError = ref<string | null>(null);
  const isTestingOpenai = ref(false);
  const isSavingOpenai = ref(false);

  const openaiConfigured = computed(() => isOpenaiOcrConfigured(normalizedCloudOcr.value));
  const profileCount = computed(() => profiles.value.length);
  const hasProfiles = computed(() => profileCount.value > 0);
  const openaiStatusText = computed(() => {
    if (!hasProfiles.value) return t("settings.openai.notConfigured");
    return t("settings.openai.profileSummary", {
      count: profileCount.value,
      name: activeProfile.value?.name ?? activeProfile.value?.model ?? "",
    });
  });

  function applyProfile(profile: OpenaiOcrProfile | null) {
    profileName.value = profile?.name ?? "";
    openaiBaseUrl.value = profile?.baseUrl ?? "";
    openaiModel.value = profile?.model ?? "";
    openaiApiKey.value = profile?.apiKey ?? "";
  }

  function clearDraft() {
    applyProfile(null);
  }

  // 父组件装载快照后同步列表；保存/切换/删除返回的新设置也会经这里回填。
  watch(
    normalizedCloudOcr,
    (cloud) => {
      openaiPrompts.value = cloud.openaiPrompts.map((item) => ({ ...item }));
      if (isCreatingNew.value) return;
      const selected = cloud.openaiProfiles.find((profile) => profile.id === selectedProfileId.value);
      if (selected) {
        applyProfile(selected);
        return;
      }
      const preferred = activeOpenaiOcrProfile(cloud) ?? cloud.openaiProfiles[0] ?? null;
      selectedProfileId.value = preferred?.id ?? null;
      applyProfile(preferred);
    },
    { deep: true, immediate: true },
  );

  // Key 可留空：本地无鉴权端点（vLLM/Ollama 等）不需要。
  const formComplete = computed(() =>
    Boolean(openaiBaseUrl.value.trim() && openaiModel.value.trim()),
  );
  const isEditingExisting = computed(() =>
    Boolean(selectedProfileId.value && profiles.value.some((profile) => profile.id === selectedProfileId.value)),
  );
  const isEditingActive = computed(() =>
    Boolean(selectedProfileId.value && selectedProfileId.value === activeProfile.value?.id),
  );

  function selectProfile(profileId: string) {
    const profile = profiles.value.find((item) => item.id === profileId);
    if (!profile) return;
    selectedProfileId.value = profile.id;
    isCreatingNew.value = false;
    applyProfile(profile);
    openaiMessage.value = null;
    openaiError.value = null;
  }

  function startNewProfile() {
    selectedProfileId.value = null;
    isCreatingNew.value = true;
    clearDraft();
    openaiMessage.value = null;
    openaiError.value = null;
  }

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

  function resetMessages() {
    openaiMessage.value = null;
    openaiError.value = null;
  }

  async function testOpenai() {
    resetMessages();
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

  function selectAfterMutation(result: AppSettings | undefined, previousIds: Set<string>) {
    const cloud = cleanCloudOcrSettings(result?.cloudOcr);
    openaiPrompts.value = cloud.openaiPrompts.map((item) => ({ ...item }));
    if (selectedProfileId.value && cloud.openaiProfiles.some((item) => item.id === selectedProfileId.value)) {
      const current = cloud.openaiProfiles.find((item) => item.id === selectedProfileId.value);
      applyProfile(current ?? null);
      isCreatingNew.value = false;
      return;
    }
    const created = cloud.openaiProfiles.find((item) => !previousIds.has(item.id));
    const preferred = created ?? activeOpenaiOcrProfile(cloud) ?? cloud.openaiProfiles[0] ?? null;
    selectedProfileId.value = preferred?.id ?? null;
    isCreatingNew.value = !preferred;
    applyProfile(preferred);
  }

  async function saveOpenaiConfig() {
    resetMessages();
    isSavingOpenai.value = true;
    const previousIds = new Set(profiles.value.map((profile) => profile.id));
    try {
      const result = await settings.saveOpenaiOcrProfile(
        {
          id: selectedProfileId.value ?? "",
          name: profileName.value.trim(),
          baseUrl: openaiBaseUrl.value,
          model: openaiModel.value,
          apiKey: openaiApiKey.value,
        },
        openaiPrompts.value,
      );
      selectAfterMutation(result, previousIds);
      openaiMessage.value = t("settings.openai.saved");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  async function setActiveProfile(profileId: string = selectedProfileId.value ?? "") {
    if (!profileId) return;
    resetMessages();
    isSavingOpenai.value = true;
    try {
      const result = await settings.setOpenaiOcrActiveProfile(profileId);
      selectAfterMutation(result, new Set(profiles.value.map((profile) => profile.id)));
      openaiMessage.value = t("settings.openai.activeProfileUpdated");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  async function deleteProfile(profileId: string = selectedProfileId.value ?? "") {
    if (!profileId) return;
    resetMessages();
    isSavingOpenai.value = true;
    const wasSelected = profileId === selectedProfileId.value;
    try {
      const result = await settings.deleteOpenaiOcrProfile(profileId);
      const cloud = cleanCloudOcrSettings(result?.cloudOcr);
      openaiPrompts.value = cloud.openaiPrompts.map((item) => ({ ...item }));
      if (wasSelected) {
        const preferred = activeOpenaiOcrProfile(cloud) ?? cloud.openaiProfiles[0] ?? null;
        selectedProfileId.value = preferred?.id ?? null;
        isCreatingNew.value = !preferred;
        applyProfile(preferred);
      }
      openaiMessage.value = t("settings.openai.profileDeleted");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  async function clearOpenaiConfig() {
    resetMessages();
    isSavingOpenai.value = true;
    try {
      await settings.clearOpenaiOcrConfig();
      selectedProfileId.value = null;
      isCreatingNew.value = false;
      clearDraft();
      openaiPrompts.value = DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
      openaiMessage.value = t("settings.openai.cleared");
    } catch (unknownError) {
      openaiError.value = errorMessage(unknownError);
    } finally {
      isSavingOpenai.value = false;
    }
  }

  return {
    profiles,
    activeProfile,
    activeProfileId: computed(() => activeProfile.value?.id ?? ""),
    selectedProfileId,
    profileName,
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
    profileCount,
    hasProfiles,
    formComplete,
    isEditingExisting,
    isEditingActive,
    isCreatingNew,
    selectProfile,
    startNewProfile,
    addPrompt,
    removePrompt,
    restoreDefaultPrompts,
    testOpenai,
    saveOpenaiConfig,
    setActiveProfile,
    deleteProfile,
    clearOpenaiConfig,
  };
}
