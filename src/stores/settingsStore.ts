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
    // 保留天影响历史清理：成功后经宿主注入的钩子做一次全量重载（原为隐式 await load()，
    // Task 39 起显式化为 onApplied 钩子；调用次数与时机不变）。
    await writeSetting("update_settings", () => ipasteApi.updateSettings(days), {
      onApplied: () => retentionReloader?.(),
    });
  }

  async function updateShortcut(value: string) {
    await writeSetting(SETTINGS_SCHEMA.shortcut.command, () =>
      ipasteApi.updateShortcut(SETTINGS_SCHEMA.shortcut.clean(value)),
    );
  }

  async function updateOcrShortcut(value: string) {
    await writeSetting(SETTINGS_SCHEMA.ocrShortcut.command, () =>
      ipasteApi.updateOcrShortcut(SETTINGS_SCHEMA.ocrShortcut.clean(value)),
    );
  }

  async function updatePanelOpenBehavior(behavior: PanelOpenBehavior) {
    await writeSetting(SETTINGS_SCHEMA.panelOpenBehavior.command, () =>
      ipasteApi.updatePanelOpenBehavior(SETTINGS_SCHEMA.panelOpenBehavior.clean(behavior)),
    );
  }

  async function updateAppendCopyTimeout(minutes: number) {
    await writeMirrorSetting("appendCopyTimeoutMinutes", minutes, (next) =>
      ipasteApi.updateAppendCopyTimeout(next),
    );
  }

  async function updatePanelLayout(layout: PanelLayout) {
    await writeMirrorSetting("panelLayout", layout, (next) => ipasteApi.updatePanelLayout(next));
  }

  async function updateOcrMode(mode: OcrMode) {
    await writeMirrorSetting("ocrMode", mode, (next) => ipasteApi.updateOcrMode(next));
  }

  async function updateOcrEngine(engine: OcrEngine) {
    await writeMirrorSetting("ocrEngine", engine, (next) => ipasteApi.updateOcrEngine(next));
  }

  async function updateLanguage(value: Language) {
    await writeMirrorSetting("language", value, (next) => ipasteApi.updateLanguage(next));
  }

  async function saveOpenaiOcrConfig(
    baseUrl: string,
    model: string,
    apiKey: string,
    prompts?: CloudOcrPromptMessage[],
  ) {
    await writeSetting("update_openai_ocr_config", () =>
      ipasteApi.updateOpenaiOcrConfig(baseUrl, model, apiKey, prompts),
    );
  }

  async function clearOpenaiOcrConfig() {
    await writeSetting("clear_openai_ocr_config", () => ipasteApi.clearOpenaiOcrConfig());
  }

  async function testOpenaiOcr(
    baseUrl: string,
    model: string,
    apiKey: string,
    prompts?: CloudOcrPromptMessage[],
  ) {
    return ipasteApi.testOpenaiOcr(baseUrl, model, apiKey, prompts);
  }

  // —— 统一写路径（Task 39：全库唯一设置写入编排）——

  /** 设置域 ref 索引：乐观写/回滚按 key 定位（language 附带界面语言切换）。 */
  const settingRefs = { appendCopyTimeoutMinutes, panelLayout, ocrMode, ocrEngine, language } as const;
  type MirrorKey = keyof typeof settingRefs;

  function applyMirror(key: MirrorKey, value: (typeof settingRefs)[MirrorKey]["value"]) {
    settingRefs[key].value = value;
    if (key === "language") setLanguage(value as Language);
  }

  /**
   * 乐观镜像写入：先写本地镜像（language 顺带切换 i18n），成功后由后端返回值整体回填；
   * 真实失败回滚到之前的镜像值。老二进制命令缺失时保持乐观值静默返回（与旧版一致：
   * 没有可同步的后端，回滚只会让界面与用户刚做的操作打架）。
   */
  async function writeMirrorSetting<K extends MirrorKey>(
    key: K,
    value: (typeof settingRefs)[K]["value"],
    save: (next: (typeof settingRefs)[K]["value"]) => Promise<AppSettings>,
  ) {
    const spec = SETTINGS_SCHEMA[key];
    const previous = settingRefs[key].value;
    const next = spec.clean(value as never) as (typeof settingRefs)[K]["value"];
    await writeSetting(spec.command, () => save(next), {
      optimistic: () => applyMirror(key, next),
      rollback: () => applyMirror(key, previous),
      tolerateMissing: true,
    });
  }

  /**
   * 设置落库统一编排：乐观写（可选）→ 命令 → 成功回填（+ onApplied 钩子）；
   * 老二进制命令缺失（tolerateMissing）→ 保持乐观值静默；
   * 真实失败 → 回滚乐观值 + toast + 继续抛出。
   * （Task 39 前的三种失败形态——乐观式只 toast 不回滚、回显式裸抛不 toast、
   * 　命令缺失静默——统一为此一种。）
   */
  async function writeSetting(
    command: string,
    save: () => Promise<AppSettings>,
    options: {
      optimistic?: () => void;
      rollback?: () => void;
      onApplied?: () => void | Promise<void>;
      tolerateMissing?: boolean;
    } = {},
  ): Promise<void> {
    options.optimistic?.();
    try {
      applySettings(await save());
      await options.onApplied?.();
    } catch (unknownError) {
      if (options.tolerateMissing && isCommandMissing(unknownError, command)) return;
      options.rollback?.();
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
