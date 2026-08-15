import { computed, ref, watch } from "vue";
import { t } from "../i18n";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";

export function useCloudSync() {
  const store = useIpasteStore();
  const cloudApiAddress = ref("");
  const cloudApiKey = ref("");
  const cloudMessage = ref<string | null>(null);
  const cloudError = ref<string | null>(null);
  const isTestingCloud = ref(false);
  const isSavingCloud = ref(false);

  const cloudStatusText = computed(() => {
    return store.cloud.enabled ? t("settings.cloud.enabled") : t("settings.cloud.disabled");
  });

  function resetCloudForm() {
    cloudApiAddress.value = store.cloud.apiAddress;
    cloudApiKey.value = store.cloud.apiKey;
    cloudMessage.value = null;
    cloudError.value = null;
  }

  // store.load() 在父组件 onMounted 完成；watch 让表单跟随已加载的 store.cloud，
  // 替代原先在 onMounted 里手动调用的 resetCloudForm()，规避子父挂载时序。
  watch(() => store.cloud, () => resetCloudForm(), { deep: true });

  async function testCloud() {
    cloudMessage.value = null;
    cloudError.value = null;
    isTestingCloud.value = true;
    try {
      await store.testCloudSettings(cloudApiAddress.value, cloudApiKey.value);
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
      await store.saveCloudSettings(cloudApiAddress.value, cloudApiKey.value);
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
      await store.disableCloudSync();
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
