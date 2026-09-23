import type { AppSettings, ImageOcrResult, OcrInstallStatus, OcrResultPayload, ScreenshotSelection } from "../../types";
import { call } from "./index";
import {
  buildInstalledMocrStatus,
  buildInstalledPaddleStatus,
  buildMockImageOcrResult,
  fallbackMocrInstallStatus,
  fallbackOcrInstallStatus,
  mockSettings,
} from "./mockBackend";

/** OCR 域：Paddle / Manga / 云端引擎安装状态与识别、截图选区流程、系统权限查询。 */
export const ocrApi = {
  ocrInstallStatus() {
    return call<OcrInstallStatus>("get_ocr_install_status", undefined, fallbackOcrInstallStatus);
  },
  installOcrAssets() {
    return call<OcrInstallStatus>("install_ocr_assets", undefined, buildInstalledPaddleStatus());
  },
  removeOcrAssets() {
    return call<OcrInstallStatus>("remove_ocr_assets", undefined, fallbackOcrInstallStatus);
  },
  mocrInstallStatus() {
    return call<OcrInstallStatus>("get_mocr_install_status", undefined, fallbackMocrInstallStatus);
  },
  installMocrAssets() {
    return call<OcrInstallStatus>("install_mocr_assets", undefined, buildInstalledMocrStatus());
  },
  removeMocrAssets() {
    return call<OcrInstallStatus>("remove_mocr_assets", undefined, fallbackMocrInstallStatus);
  },
  recognizeImageText(imagePath: string, profile?: string, language?: string) {
    return call<ImageOcrResult>("recognize_image_text", { imagePath, profile, language }, buildMockImageOcrResult());
  },
  startScreenshotOcr() {
    return call<void>("start_screenshot_ocr");
  },
  submitScreenshotSelection(selection: ScreenshotSelection) {
    return call<void>("submit_screenshot_selection", { selection });
  },
  cancelScreenshotOcr() {
    return call<void>("cancel_screenshot_ocr");
  },
  getOcrResultPayload(token: string) {
    return call<OcrResultPayload | null>("get_ocr_result_payload", { token }, null);
  },
  updateOcrShortcut(shortcut: string) {
    return call<AppSettings>("update_ocr_shortcut", { shortcut }, mockSettings({ ocrShortcut: shortcut }));
  },
  openScreenRecordingSettings() {
    return call<void>("open_screen_recording_settings");
  },
  screenCapturePermissionStatus() {
    return call<boolean>("screen_capture_permission_status", undefined, true);
  },
  accessibilityPermissionStatus() {
    return call<boolean>("accessibility_permission_status", undefined, true);
  },
  requestScreenCapturePermission() {
    return call<boolean>("request_screen_capture_permission", undefined, false);
  },
  openAccessibilitySettings() {
    return call<void>("open_accessibility_settings");
  },
};
