import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useSettingsStore } from "../stores/settingsStore";
import { useCloudSyncStore } from "../stores/cloudSyncStore";

export function useCloudSync() {
  const settings = useSettingsStore();
  const sync = useCloudSyncStore();
  const cloudApiAddress = ref(settings.cloud.apiAddress);
  const cloudApiKey = ref(settings.cloud.apiKey);
  const cloudMessage = ref<string | null>(null);
  const cloudError = ref<string | null>(null);
  const isTestingCloud = ref(false);
  const isSavingCloud = ref(false);

  const cloudStatusText = computed(() => {
    return settings.cloud.enabled ? t("settings.cloud.enabled") : t("settings.cloud.disabled");
  });

  function syncFormFromStore() {
    cloudApiAddress.value = settings.cloud.apiAddress;
    cloudApiKey.value = settings.cloud.apiKey;
  }

  function resetCloudForm() {
    syncFormFromStore();
    cloudMessage.value = null;
    cloudError.value = null;
  }

  // 快照在父组件 onMounted 装载；watch 让表单跟随已加载的 settings.cloud，
  // 加上 immediate: true 确保在子组件（tab）晚于装载挂载时也能立即读入已保存配置。
  watch(() => settings.cloud, () => syncFormFromStore(), { deep: true, immediate: true });

  async function testCloud() {
    cloudMessage.value = null;
    cloudError.value = null;
    isTestingCloud.value = true;
    try {
      await sync.testCloudSettings(cloudApiAddress.value, cloudApiKey.value);
      cloudMessage.value = t("settings.cloud.connected");
    } catch (unknownError) {
      cloudError.value = errorMessage(unknownError);
    } finally {
      isTestingCloud.value = false;
    }
  }

  async function saveCloud() {
    cloudMessage.value = null;
    cloudError.value = null;
    isSavingCloud.value = true;
    try {
      await sync.saveCloudSettings(cloudApiAddress.value, cloudApiKey.value);
      resetCloudForm();
      cloudMessage.value = t("settings.cloud.saved");
    } catch (unknownError) {
      cloudError.value = errorMessage(unknownError);
    } finally {
      isSavingCloud.value = false;
    }
  }

  async function disableCloud() {
    cloudMessage.value = null;
    cloudError.value = null;
    isSavingCloud.value = true;
    try {
      await sync.disableCloudSync();
      resetCloudForm();
      cloudMessage.value = t("settings.cloud.disabledMessage");
    } catch (unknownError) {
      cloudError.value = errorMessage(unknownError);
    } finally {
      isSavingCloud.value = false;
    }
  }

  return {
    cloudApiAddress,
    cloudApiKey,
    cloudMessage,
    cloudError,
    isTestingCloud,
    isSavingCloud,
    cloudStatusText,
    testCloud,
    saveCloud,
    disableCloud,
  };
}
