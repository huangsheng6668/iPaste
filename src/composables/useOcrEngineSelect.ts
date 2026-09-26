import { computed, onUnmounted } from "vue";
import { subscribe } from "../platform/events";
import { t } from "../i18n";
import { isTauri } from "../lib/env";
import { useIpasteStore } from "../stores/ipasteStore";
import { useSettingsStore } from "../stores/settingsStore";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { OcrEngine, SettingsChangedEvent } from "../types";

export type OcrEngineOption = {
  value: OcrEngine;
  label: string;
  ready: boolean;
};

/** OCR 界面（截图结果窗 / 图片查看器）共用的引擎切换下拉逻辑：
 * 选项复用设置页文案，未配置的云引擎禁用；切换写入全局设置并返回新值，
 * 调用方据此用新引擎重新识别。独立窗口需自行 store.load() 后使用。
 *
 * 这些窗口不经过 App.vue 的事件接线：这里自行订阅 settingsChanged，
 * 保证「先开着 OCR 窗口、再去设置页保存 Key」时选项能即时解锁，
 * 而不是停留在窗口打开时的旧配置快照。 */
export function useOcrEngineSelect() {
  const store = useIpasteStore(); // 仅剩 load()（防陈旧兜底重载）
  const settings = useSettingsStore();

  const ocrEngineOptions = computed<OcrEngineOption[]>(() => [
    { value: "local", label: t("settings.bigmodel.engineLocal"), ready: true },
    {
      value: "openai",
      label: t("settings.openai.engineOpenai"),
      ready: Boolean(
        settings.cloudOcr.openaiBaseUrl && settings.cloudOcr.openaiModel && settings.cloudOcr.openaiApiKey,
      ),
    },
  ]);

  let unlistenSettingsChanged: (() => void) | null = null;
  if (isTauri) {
    // onUnmounted 必须在 setup 同步阶段注册；subscribe 异步完成后若组件已卸载则立即注销
    let disposed = false;
    void subscribe<SettingsChangedEvent>(IPASTE_EVENTS.settingsChanged, (changed) => {
      settings.applySettings(changed.settings);
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unlistenSettingsChanged = unlisten;
      }
    });
    onUnmounted(() => {
      disposed = true;
      unlistenSettingsChanged?.();
    });
  }

  async function switchOcrEngine(value: string): Promise<OcrEngine | null> {
    let option = ocrEngineOptions.value.find((option) => option.value === value);
    // 防陈旧兜底：事件覆盖不到的场景（如窗口加载时 keyring 瞬时读失败），
    // 重新拉一次设置再判定，避免把已配置的引擎误判为未配置。
    if (!option?.ready) {
      await store.load().catch(() => undefined);
      option = ocrEngineOptions.value.find((option) => option.value === value);
    }
    if (!option || !option.ready) return null;
    try {
      await settings.updateOcrEngine(option.value);
      return option.value;
    } catch {
      return null;
    }
  }

  return { ocrEngineOptions, switchOcrEngine };
}
