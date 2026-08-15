import { errorMessage } from "../../lib/appError";
import type { Language, OcrMode, PanelLayout } from "../../types";

export const DEFAULT_RETENTION_DAYS = 30;
export const DEFAULT_APPEND_COPY_TIMEOUT_MINUTES = 1;
export const APPEND_COPY_TIMEOUT_OPTIONS = [1, 3, 5, 10];
export const DEFAULT_PANEL_LAYOUT: PanelLayout = "top";
export const DEFAULT_OCR_MODE: OcrMode = "fast";
export const DEFAULT_LANGUAGE: Language = "en";

export function cleanAppendCopyTimeoutMinutes(minutes: unknown): number {
  const normalized = Number(minutes);
  return APPEND_COPY_TIMEOUT_OPTIONS.includes(normalized)
    ? normalized
    : DEFAULT_APPEND_COPY_TIMEOUT_MINUTES;
}

export function cleanPanelLayout(layout: unknown): PanelLayout {
  return layout === "side" ? "side" : DEFAULT_PANEL_LAYOUT;
}

export function cleanOcrMode(mode: unknown): OcrMode {
  return mode === "best" ? "best" : DEFAULT_OCR_MODE;
}

export function isSettingsCommandMissing(err: unknown, command: string): boolean {
  const message = errorMessage(err);
  return message.includes(command) && message.includes("not found");
}
