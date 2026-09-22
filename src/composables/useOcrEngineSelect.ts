import { computed } from "vue";
import { t } from "../i18n";
import { useIpasteStore } from "../stores/ipasteStore";
import type { OcrEngine } from "../types";

export type OcrEngineOption = {
  value: OcrEngine;
  label: string;
  ready: boolean;
};

/** OCR 界面（截图结果窗 / 图片查看器）共用的引擎切换下拉逻辑：
 * 选项复用设置页文案，未配置的云引擎禁用；切换写入全局设置并返回新值，
 * 调用方据此用新引擎重新识别。独立窗口需自行 store.load() 后使用。 */
export function useOcrEngineSelect() {
  const store = useIpasteStore();

  const ocrEngineOptions = computed<OcrEngineOption[]>(() => [
    { value: "local", label: t("settings.bigmodel.engineLocal"), ready: true },
    {
      value: "bigmodel",
      label: t("settings.bigmodel.engineCloud"),
      ready: Boolean(store.cloudOcr.bigmodelApiKey),
    },
    {
      value: "openai",
      label: t("settings.openai.engineOpenai"),
      ready: Boolean(
        store.cloudOcr.openaiBaseUrl && store.cloudOcr.openaiModel && store.cloudOcr.openaiApiKey,
      ),
    },
  ]);

  async function switchOcrEngine(value: string): Promise<OcrEngine | null> {
    const option = ocrEngineOptions.value.find((option) => option.value === value);
    if (!option || !option.ready) return null;
    try {
      await store.updateOcrEngine(option.value);
      return option.value;
    } catch {
      return null;
    }
  }

  return { ocrEngineOptions, switchOcrEngine };
}
