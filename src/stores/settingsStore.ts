import { defineStore } from "pinia";
import { ref } from "vue";
import { cleanLanguage, setLanguage } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { isCommandMissing } from "../lib/appError";
import { showError } from "./uiStore";
import {
  DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
  DEFAULT_LANGUAGE,
  DEFAULT_OPENAI_OCR_PROMPTS,
  DEFAULT_OCR_ENGINE,
  DEFAULT_OCR_MODE,
  DEFAULT_PANEL_LAYOUT,
  DEFAULT_RETENTION_DAYS,
  cleanAppendCopyTimeoutMinutes,
  cleanCloudOcrSettings,
  SETTINGS_SCHEMA,
  cleanOcrEngine,
  cleanOcrMode,
  cleanPanelLayout,
} from "./lib/settings";
import type {
  AppSettings,
  AppSnapshot,
  CloudOcrPromptMessage,
  CloudOcrSettings,
  CloudSettings,
  Language,
  OcrEngine,
  OcrMode,
  PanelLayout,
  PanelOpenBehavior,
} from "../types";

/**
 * 设置单一镜像 store（Task 10 从 ipasteStore 原样拆出，行为零改动）。
 * 写路径保持现状两条：回显式（applySettings(await api...)）与乐观式
 * （persistSetting + tolerateMissing）——语义统一属计划 Task 39，不在本任务。
 */
export const useSettingsStore = defineStore("settings", () => {
  const shortcut = ref("CommandOrControl+Shift+V");
  const ocrShortcut = ref("CommandOrControl+Shift+O");
  const retentionDays = ref(DEFAULT_RETENTION_DAYS);
  const appendCopyTimeoutMinutes = ref(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
  const panelOpenBehavior = ref<PanelOpenBehavior>("history");
  const panelLayout = ref<PanelLayout>(DEFAULT_PANEL_LAYOUT);
  const ocrMode = ref<OcrMode>(DEFAULT_OCR_MODE);
  const ocrEngine = ref<OcrEngine>(DEFAULT_OCR_ENGINE);
  const language = ref<Language>(DEFAULT_LANGUAGE);
  const cloud = ref<CloudSettings>({
    apiAddress: "",
    apiKey: "",
    enabled: false,
    lastConnectedAt: null,
  });
  const cloudOcr = ref<CloudOcrSettings>({
    openaiBaseUrl: "",
    openaiModel: "",
    openaiApiKey: "",
    openaiPrompts: DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item })),
  });

  // 保留天变更后的全量重载由宿主 store 注入（原实现直接调用 ipasteStore.load，
  // 见特征测试「updateRetentionDays 成功后触发一次完整 load()」）。
  let retentionReloader: (() => Promise<void> | void) | null = null;

  function registerRetentionReloader(reload: () => Promise<void> | void) {
    retentionReloader = reload;
  }

  async function updateRetentionDays(days: number) {
    const settings = await ipasteApi.updateSettings(days);
    applySettings(settings);
    await retentionReloader?.();
  }

  async function updateAppendCopyTimeout(minutes: number) {
    const spec = SETTINGS_SCHEMA.appendCopyTimeoutMinutes;
    const nextMinutes = spec.clean(minutes);
    appendCopyTimeoutMinutes.value = nextMinutes;
    await persistSetting(spec.command, () => ipasteApi.updateAppendCopyTimeout(nextMinutes), { tolerateMissing: true });
  }

  async function updateShortcut(value: string) {
    const settings = await ipasteApi.updateShortcut(SETTINGS_SCHEMA.shortcut.clean(value));
    applySettings(settings);
  }

  async function updateOcrShortcut(value: string) {
    const settings = await ipasteApi.updateOcrShortcut(SETTINGS_SCHEMA.ocrShortcut.clean(value));
    applySettings(settings);
  }

  async function updatePanelOpenBehavior(behavior: PanelOpenBehavior) {
    const settings = await ipasteApi.updatePanelOpenBehavior(SETTINGS_SCHEMA.panelOpenBehavior.clean(behavior));
    applySettings(settings);
  }

  async function updatePanelLayout(layout: PanelLayout) {
    const spec = SETTINGS_SCHEMA.panelLayout;
    const nextLayout = spec.clean(layout);
    panelLayout.value = nextLayout;
    await persistSetting(spec.command, () => ipasteApi.updatePanelLayout(nextLayout), { tolerateMissing: true });
  }

  async function updateOcrMode(mode: OcrMode) {
    const spec = SETTINGS_SCHEMA.ocrMode;
    const nextMode = spec.clean(mode);
    ocrMode.value = nextMode;
    await persistSetting(spec.command, () => ipasteApi.updateOcrMode(nextMode), { tolerateMissing: true });
  }

  async function updateOcrEngine(engine: OcrEngine) {
    const spec = SETTINGS_SCHEMA.ocrEngine;
    const nextEngine = spec.clean(engine);
    ocrEngine.value = nextEngine;
    await persistSetting(spec.command, () => ipasteApi.updateOcrEngine(nextEngine), { tolerateMissing: true });
  }

  async function saveOpenaiOcrConfig(
    baseUrl: string,
    model: string,
    apiKey: string,
    prompts?: CloudOcrPromptMessage[],
  ) {
    const settings = await ipasteApi.updateOpenaiOcrConfig(baseUrl, model, apiKey, prompts);
    applySettings(settings);
  }

  async function clearOpenaiOcrConfig() {
    const settings = await ipasteApi.clearOpenaiOcrConfig();
    applySettings(settings);
  }

  async function testOpenaiOcr(
    baseUrl: string,
    model: string,
    apiKey: string,
    prompts?: CloudOcrPromptMessage[],
  ) {
    return ipasteApi.testOpenaiOcr(baseUrl, model, apiKey, prompts);
  }

  async function updateLanguage(value: Language) {
    const spec = SETTINGS_SCHEMA.language;
    const nextLanguage = spec.clean(value);
    language.value = nextLanguage;
    setLanguage(nextLanguage);
    await persistSetting(spec.command, () => ipasteApi.updateLanguage(nextLanguage), { tolerateMissing: true });
  }

  /** settings 落库统一编排：成功回填广播；老二进制命令缺失时按需静默容忍。 */
  async function persistSetting(
    command: string,
    save: () => Promise<AppSettings>,
    options: { tolerateMissing?: boolean } = {},
  ): Promise<void> {
    try {
      applySettings(await save());
    } catch (unknownError) {
      if (options.tolerateMissing && isCommandMissing(unknownError, command)) return;
      showError(unknownError);
      throw unknownError;
    }
  }

  function applySettings(settings: AppSettings) {
    shortcut.value = settings.shortcut;
    ocrShortcut.value = settings.ocrShortcut || "CommandOrControl+Shift+O";
    retentionDays.value = settings.retentionDays;
    appendCopyTimeoutMinutes.value = settings.appendCopyTimeoutMinutes;
    panelOpenBehavior.value = settings.panelOpenBehavior;
    panelLayout.value = settings.panelLayout;
    ocrMode.value = settings.ocrMode;
    ocrEngine.value = cleanOcrEngine(settings.ocrEngine);
    language.value = settings.language;
    setLanguage(language.value);
    cloud.value = settings.cloud;
    cloudOcr.value = cleanCloudOcrSettings(settings.cloudOcr);
  }

  /** 快照中的设置字段回填（原 ipasteStore.hydrateFromSnapshot 的设置段，逐字段照抄）。 */
  function applySnapshotSettings(snapshot: AppSnapshot) {
    shortcut.value = snapshot.shortcut;
    retentionDays.value = snapshot.settings.retentionDays;
    appendCopyTimeoutMinutes.value = cleanAppendCopyTimeoutMinutes(snapshot.settings.appendCopyTimeoutMinutes);
    panelOpenBehavior.value = snapshot.settings.panelOpenBehavior;
    panelLayout.value = cleanPanelLayout(snapshot.settings.panelLayout);
    ocrMode.value = cleanOcrMode(snapshot.settings.ocrMode);
    ocrEngine.value = cleanOcrEngine(snapshot.settings.ocrEngine);
    language.value = cleanLanguage(snapshot.settings.language);
    setLanguage(language.value);
    cloud.value = snapshot.settings.cloud;
    cloudOcr.value = cleanCloudOcrSettings(snapshot.settings.cloudOcr);
  }

  return {
    shortcut,
    ocrShortcut,
    retentionDays,
    appendCopyTimeoutMinutes,
    panelOpenBehavior,
    panelLayout,
    ocrMode,
    ocrEngine,
    language,
    cloud,
    cloudOcr,
    registerRetentionReloader,
    applySnapshotSettings,
    applySettings,
    updateRetentionDays,
    updateAppendCopyTimeout,
    updateShortcut,
    updateOcrShortcut,
    updatePanelOpenBehavior,
    updatePanelLayout,
    updateOcrMode,
    updateOcrEngine,
    updateLanguage,
    saveOpenaiOcrConfig,
    clearOpenaiOcrConfig,
    testOpenaiOcr,
  };
});
