import { computed, onUnmounted } from "vue";
import { subscribe } from "../platform/events";
import { t } from "../i18n";
import { isTauri } from "../lib/env";
import { isOpenaiProfileReady } from "../lib/openaiEndpoint";
import { useIpasteStore } from "../stores/ipasteStore";
import { cleanCloudOcrSettings } from "../stores/lib/settings";
import { useSettingsStore } from "../stores/settingsStore";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { OcrEngine, OpenaiOcrProfile, SettingsChangedEvent } from "../types";

export type OcrEngineOption = {
  value: string;
  label: string;
  ready: boolean;
};

/** OCR 界面（截图结果窗 / 图片查看器）共用的引擎切换下拉逻辑：
 * 选项复用设置页文案；未配齐或远端缺 API Key 的接口禁用（本地无鉴权端点
 * 可留空 Key）；切换写入全局设置并返回新值，调用方据此用新引擎重新识别。
 * 独立窗口需自行 store.load() 后使用。
 *
 * 这些窗口不经过 App.vue 的事件接线：这里自行订阅 settingsChanged，
 * 保证「先开着 OCR 窗口、再去设置页保存 Key」时选项能即时解锁，
 * 而不是停留在窗口打开时的旧配置快照。 */
export function useOcrEngineSelect(currentEngine?: () => OcrEngine) {
  const store = useIpasteStore(); // 仅剩 load()（防陈旧兜底重载）
  const settings = useSettingsStore();

  const configuredProfiles = computed<OpenaiOcrProfile[]>(() =>
    cleanCloudOcrSettings(settings.cloudOcr).openaiProfiles.filter(
      (profile) => Boolean(profile.baseUrl.trim() && profile.model.trim()),
    ),
  );

  // 下拉的选中项必须落在「实际激活的配置」上：缺 Key 的激活接口要显示成不可用，
  // 不能悄悄替换成另一个就绪接口，否则用户看到的和实际请求的不是同一个。
  const activeConfiguredProfile = computed<OpenaiOcrProfile | null>(() => {
    const list = configuredProfiles.value;
    if (list.length === 0) return null;
    const activeId = settings.cloudOcr.openaiActiveProfileId;
    return list.find((profile) => profile.id === activeId) ?? list[0];
  });

  const ocrEngineOptions = computed<OcrEngineOption[]>(() => {
    const baseLabel = t("settings.openai.engineOpenai");
    const profiles = configuredProfiles.value;
    if (profiles.length >= 2) {
      return [
        { value: "local", label: t("settings.bigmodel.engineLocal"), ready: true },
        ...profiles.map((profile) => ({
          value: `openai:${profile.id}`,
          label: `OpenAI · ${profile.name.trim() || profile.model}`,
          // 缺 Key 的远端接口仍列出（让用户看清是哪个没配好），但不可选中。
          ready: isOpenaiProfileReady(profile),
        })),
      ];
    }
    const active = activeConfiguredProfile.value;
    return [
      { value: "local", label: t("settings.bigmodel.engineLocal"), ready: true },
      {
        value: "openai",
        label: baseLabel,
        ready: Boolean(active && isOpenaiProfileReady(active)),
      },
    ];
  });

  const selectedEngineValue = computed<string>(() => {
    const engine = currentEngine ? currentEngine() : settings.ocrEngine;
    if (engine === "local") return "local";
    if (configuredProfiles.value.length >= 2 && activeConfiguredProfile.value) {
      return `openai:${activeConfiguredProfile.value.id}`;
    }
    return "openai";
  });

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
    let option = ocrEngineOptions.value.find((item) => item.value === value);
    // 防陈旧兜底：事件覆盖不到的场景（如窗口加载时 keyring 瞬时读失败），
    // 重新拉一次设置再判定，避免把已配置的引擎误判为未配置。
    if (!option?.ready) {
      await store.load().catch(() => undefined);
      option = ocrEngineOptions.value.find((item) => item.value === value);
    }
    if (!option || !option.ready) return null;

    try {
      if (option.value === "local") {
        await settings.updateOcrEngine("local");
        return "local";
      }
      if (option.value.startsWith("openai:")) {
        const profileId = option.value.slice("openai:".length);
        if (profileId && profileId !== settings.cloudOcr.openaiActiveProfileId) {
          await settings.setOpenaiOcrActiveProfile(profileId);
        }
        if (settings.ocrEngine !== "openai") {
          await settings.updateOcrEngine("openai");
        }
        return "openai";
      }
      await settings.updateOcrEngine("openai");
      return "openai";
    } catch {
      return null;
    }
  }

  return { ocrEngineOptions, selectedEngineValue, switchOcrEngine };
}
