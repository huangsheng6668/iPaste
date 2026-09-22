import type { CloudOcrPromptMessage, CloudOcrSettings, Language, OcrEngine, OcrMode, PanelLayout } from "../../types";

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

export function cleanCloudOcrSettings(settings: unknown): CloudOcrSettings {
  const raw = settings && typeof settings === "object" ? (settings as Record<string, unknown>) : {};
  return {
    openaiBaseUrl: typeof raw.openaiBaseUrl === "string" ? raw.openaiBaseUrl : "",
    openaiModel: typeof raw.openaiModel === "string" ? raw.openaiModel : "",
    openaiApiKey: typeof raw.openaiApiKey === "string" ? raw.openaiApiKey : "",
    openaiPrompts: cleanOpenaiPrompts(raw.openaiPrompts),
  };
}

