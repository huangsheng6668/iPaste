import { cleanLanguage } from "../../i18n";
import type {
  CloudOcrPromptMessage,
  CloudOcrSettings,
  Language,
  OcrEngine,
  OcrMode,
  PanelLayout,
  PanelOpenBehavior,
} from "../../types";

export const DEFAULT_RETENTION_DAYS = 30;
export const DEFAULT_APPEND_COPY_TIMEOUT_MINUTES = 1;
export const APPEND_COPY_TIMEOUT_OPTIONS = [1, 3, 5, 10];
export const DEFAULT_PANEL_LAYOUT: PanelLayout = "top";
export const DEFAULT_OCR_MODE: OcrMode = "fast";
export const DEFAULT_OCR_ENGINE: OcrEngine = "local";
export const DEFAULT_LANGUAGE: Language = "en";

export const DEFAULT_OPENAI_OCR_PROMPTS: CloudOcrPromptMessage[] = [
  {
    role: "system",
    content:
      "You are a precise multilingual OCR transcription engine. Transcribe all readable text from the provided image accurately, preserving its natural reading order, line breaks, structural indentation, and original language. Do not summarize, explain, translate, describe visual elements, or wrap the transcription in commentary.",
  },
  {
    role: "user",
    content:
      "Transcribe all visible text from the attached image in natural reading order. Output only the plain transcription, without conversational filler or explanations.\nExpected primary recognition language: {recognition_language}.",
  },
];

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

export function cleanOcrEngine(engine: unknown): OcrEngine {
  return engine === "openai" ? "openai" : DEFAULT_OCR_ENGINE;
}

export function cleanOpenaiPrompts(prompts: unknown): CloudOcrPromptMessage[] {
  if (!Array.isArray(prompts) || prompts.length === 0) {
    return DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
  }
  const cleaned: CloudOcrPromptMessage[] = [];
  for (const item of prompts) {
    if (item && typeof item === "object") {
      const role = (item as { role?: unknown }).role === "system" ? "system" : "user";
      const content = typeof (item as { content?: unknown }).content === "string" ? (item as { content: string }).content : "";
      cleaned.push({ role, content });
    }
  }
  return cleaned.length > 0 ? cleaned : DEFAULT_OPENAI_OCR_PROMPTS.map((item) => ({ ...item }));
}

/** 云 OCR（OpenAI 兼容）是否已配置齐 base/model/key（useOpenaiOcr 与引擎选择区共用）。 */
/**
 * 设置项登记表（Task 37）：键名 → 后端命令 / 本地清洗器 / 写法语义的唯一来源。
 *
 * - `command`：Tauri 命令名，用在 persistSetting 的 tolerateMissing 判定与调试；
 * - `clean`：本地清洗（乐观写与边界防御共用，非法值回落默认）；
 * - `optimistic`：true = 先写本地再落库（失败静默容忍老二进制），
 *   false = 等后端返回值整体回填（applySettings）。
 *
 * 后端对应的清单在 `src-tauri/src/store/settings/registry.rs`；两侧只共享键名与语义，
 * 不做代码生成（字段类型本就不同：Rust 侧是 String，前端是字面量联合）。
 */
export const SETTINGS_SCHEMA = {
  shortcut: {
    command: "update_shortcut",
    optimistic: false,
    clean: (value: string) => value,
  },
  ocrShortcut: {
    command: "update_ocr_shortcut",
    optimistic: false,
    clean: (value: string) => value,
  },
  panelOpenBehavior: {
    command: "update_panel_open_behavior",
    optimistic: false,
    clean: (value: PanelOpenBehavior) => value,
  },
  appendCopyTimeoutMinutes: {
    command: "update_append_copy_timeout",
    optimistic: true,
    clean: cleanAppendCopyTimeoutMinutes,
  },
  panelLayout: {
    command: "update_panel_layout",
    optimistic: true,
    clean: cleanPanelLayout,
  },
  ocrMode: {
    command: "update_ocr_mode",
    optimistic: true,
    clean: cleanOcrMode,
  },
  ocrEngine: {
    command: "update_ocr_engine",
    optimistic: true,
    clean: cleanOcrEngine,
  },
  language: {
    command: "update_language",
    optimistic: true,
    clean: cleanLanguage,
  },
} as const;

export type SettingKey = keyof typeof SETTINGS_SCHEMA;

/** 全部设置键（读取镜像与测试遍历用）。 */
export const SETTING_KEYS = Object.keys(SETTINGS_SCHEMA) as SettingKey[];

export function isOpenaiOcrConfigured(cloudOcr: CloudOcrSettings): boolean {
  return Boolean(cloudOcr.openaiBaseUrl && cloudOcr.openaiModel && cloudOcr.openaiApiKey);
}

export function cleanCloudOcrSettings(settings: unknown): CloudOcrSettings {
  const raw = settings && typeof settings === "object" ? (settings as Record<string, unknown>) : {};
  return {
    openaiBaseUrl: typeof raw.openaiBaseUrl === "string" ? raw.openaiBaseUrl : "",
    openaiModel: typeof raw.openaiModel === "string" ? raw.openaiModel : "",
    openaiApiKey: typeof raw.openaiApiKey === "string" ? raw.openaiApiKey : "",
    openaiPrompts: cleanOpenaiPrompts(raw.openaiPrompts),
  };
}

