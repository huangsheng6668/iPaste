import type {
  AppInfo,
  AppSettings,
  AppSnapshot,
  CloudOcrPromptMessage,
  CloudOcrSettings,
  Language,
  OcrMode,
  OpenaiOcrProfile,
} from "../../types";
import { call } from "./call";
import { DEFAULT_OPENAI_OCR_PROMPTS, fallbackAppInfo, mockSettings, mockSnapshot } from "./mockBackend";

let mockCloudOcr: CloudOcrSettings = {
  openaiProfiles: [],
  openaiActiveProfileId: "",
  openaiBaseUrl: "",
  openaiModel: "",
  openaiApiKey: "",
  openaiPrompts: DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item })),
};

function applyMockCloudOcr(
  profiles: OpenaiOcrProfile[],
  activeProfileId: string,
  prompts = mockCloudOcr.openaiPrompts,
): AppSettings {
  // call() 在浏览器模式会用 structuredClone 复制 fallback，Vue reactive 代理不可克隆，
  // 因此 mock 状态始终存普通对象副本。
  const safeProfiles = profiles.map((profile) => ({ ...profile }));
  const safePrompts = prompts.map((prompt) => ({ ...prompt }));
  const active = safeProfiles.find((profile) => profile.id === activeProfileId) ?? safeProfiles[0];
  mockCloudOcr = {
    openaiProfiles: safeProfiles,
    openaiActiveProfileId: active?.id ?? "",
    openaiBaseUrl: active?.baseUrl ?? "",
    openaiModel: active?.model ?? "",
    openaiApiKey: active?.apiKey ?? "",
    openaiPrompts: safePrompts,
  };
  return mockSettings({ cloudOcr: mockCloudOcr });
}

/** 设置域：应用设置项更新、云同步配置、快捷键、开机自启与设置窗口。 */
export const settingsApi = {
  setListening(enabled: boolean) {
    return call<boolean>("set_listening", { enabled }, enabled);
  },
  setAppendCopyEnabled(enabled: boolean) {
    return call<boolean>("set_append_copy_enabled", { enabled }, enabled);
  },
  updateSettings(retentionDays: number) {
    return call<AppSettings>("update_settings", { retentionDays }, mockSettings({ retentionDays }));
  },
  updateAppendCopyTimeout(minutes: number) {
    return call<AppSettings>("update_append_copy_timeout", { minutes }, mockSettings({ appendCopyTimeoutMinutes: minutes }));
  },
  updateShortcut(shortcut: string) {
    return call<AppSettings>("update_shortcut", { shortcut }, mockSettings({ shortcut }));
  },
  setAppShortcutEnabled(enabled: boolean) {
    return call<boolean>("set_app_shortcut_enabled", { enabled }, enabled);
  },
  updatePanelOpenBehavior(behavior: AppSettings["panelOpenBehavior"]) {
    return call<AppSettings>("update_panel_open_behavior", { behavior }, mockSettings({ panelOpenBehavior: behavior }));
  },
  updatePanelLayout(layout: AppSettings["panelLayout"]) {
    return call<AppSettings>("update_panel_layout", { layout }, mockSettings({ panelLayout: layout }));
  },
  updateOcrMode(mode: OcrMode) {
    return call<AppSettings>("update_ocr_mode", { mode }, mockSettings({ ocrMode: mode }));
  },
  updateOcrEngine(engine: AppSettings["ocrEngine"]) {
    return call<AppSettings>("update_ocr_engine", { engine }, mockSettings({ ocrEngine: engine }));
  },
  updateOpenaiOcrConfig(baseUrl: string, model: string, apiKey: string, prompts?: CloudOcrPromptMessage[]) {
    const effectivePrompts = prompts ?? DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
    const profile: OpenaiOcrProfile = {
      id: mockCloudOcr.openaiActiveProfileId || "legacy",
      name: model,
      baseUrl,
      model,
      apiKey,
    };
    const profiles = mockCloudOcr.openaiProfiles.length
      ? mockCloudOcr.openaiProfiles.map((item) => item.id === profile.id ? profile : item)
      : [profile];
    return call<AppSettings>("update_openai_ocr_config", { baseUrl, model, apiKey, prompts: effectivePrompts }, applyMockCloudOcr(profiles, profile.id, effectivePrompts));
  },
  saveOpenaiOcrProfile(profile: OpenaiOcrProfile, prompts?: CloudOcrPromptMessage[]) {
    const id = profile.id || crypto.randomUUID();
    const saved = { ...profile, id, name: profile.name || profile.model };
    const profiles = mockCloudOcr.openaiProfiles.some((item) => item.id === id)
      ? mockCloudOcr.openaiProfiles.map((item) => item.id === id ? saved : item)
      : [...mockCloudOcr.openaiProfiles, saved];
    const activeId = mockCloudOcr.openaiActiveProfileId || id;
    const effectivePrompts = prompts ?? mockCloudOcr.openaiPrompts;
    return call<AppSettings>("save_openai_ocr_profile", { profile: saved, prompts: effectivePrompts }, applyMockCloudOcr(profiles, activeId, effectivePrompts));
  },
  setOpenaiOcrActiveProfile(profileId: string) {
    return call<AppSettings>("set_openai_ocr_active_profile", { profileId }, applyMockCloudOcr(mockCloudOcr.openaiProfiles, profileId));
  },
  deleteOpenaiOcrProfile(profileId: string) {
    const profiles = mockCloudOcr.openaiProfiles.filter((profile) => profile.id !== profileId);
    return call<AppSettings>("delete_openai_ocr_profile", { profileId }, applyMockCloudOcr(profiles, mockCloudOcr.openaiActiveProfileId === profileId ? profiles[0]?.id ?? "" : mockCloudOcr.openaiActiveProfileId));
  },
  clearOpenaiOcrConfig() {
    return call<AppSettings>("clear_openai_ocr_config", undefined, applyMockCloudOcr([], "", DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }))));
  },
  testOpenaiOcr(baseUrl: string, model: string, apiKey: string, prompts?: CloudOcrPromptMessage[]) {
    return call<boolean>("test_openai_ocr", { baseUrl, model, apiKey, prompts }, true);
  },
  updateLanguage(language: Language) {
    return call<AppSettings>("update_language", { language }, mockSettings({ language }));
  },
  updateCloudSettings(apiAddress: string, apiKey: string) {
    return call<AppSettings>("update_cloud_settings", { apiAddress, apiKey }, mockSettings({
      cloud: { apiAddress, apiKey, enabled: Boolean(apiAddress && apiKey), lastConnectedAt: new Date().toISOString() },
    }));
  },
  disableCloudSync() {
    return call<AppSettings>("disable_cloud_sync", undefined, mockSettings({
      cloud: { apiAddress: "", apiKey: "", enabled: false, lastConnectedAt: null },
    }));
  },
  syncCloudNow() {
    return call<AppSnapshot>("sync_cloud_now", undefined, mockSnapshot);
  },
  syncCloudInBackground() {
    return call<void>("sync_cloud_in_background");
  },
  testCloudSettings(apiAddress: string, apiKey: string) {
    return call<boolean>("test_cloud_settings", { apiAddress, apiKey }, true);
  },
  appInfo() {
    return call<AppInfo>("get_app_info", undefined, fallbackAppInfo);
  },
  showSettings() {
    return call<void>("show_settings");
  },
  hideSettings() {
    return call<void>("hide_settings");
  },
  enableAutostart() {
    return call<boolean>("enable_autostart", undefined, true);
  },
  disableAutostart() {
    return call<boolean>("disable_autostart", undefined, false);
  },
  isAutostartEnabled() {
    return call<boolean>("is_autostart_enabled", undefined, false);
  },
};
