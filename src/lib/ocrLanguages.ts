import { t } from "../i18n";

export type OcrLanguageId = "auto" | "zh-Hans" | "zh-Hant" | "en" | "ja";

export type OcrLanguageOption = {
  id: OcrLanguageId;
  labelKey:
    | "ocr.language.auto"
    | "ocr.language.zhHans"
    | "ocr.language.zhHant"
    | "ocr.language.en"
    | "ocr.language.ja";
};

export const OCR_LANGUAGE_STORAGE_KEY = "ipaste.ocrLanguage";

export const OCR_LANGUAGE_OPTIONS: ReadonlyArray<OcrLanguageOption> = [
  { id: "auto", labelKey: "ocr.language.auto" },
  { id: "zh-Hans", labelKey: "ocr.language.zhHans" },
  { id: "zh-Hant", labelKey: "ocr.language.zhHant" },
  { id: "en", labelKey: "ocr.language.en" },
  { id: "ja", labelKey: "ocr.language.ja" },
];

const VALID_IDS = new Set<string>(OCR_LANGUAGE_OPTIONS.map((option) => option.id));

export function normalizeOcrLanguage(value: string | null | undefined): OcrLanguageId | null {
  return value && VALID_IDS.has(value) ? (value as OcrLanguageId) : null;
}

export function loadOcrLanguage(): OcrLanguageId {
  return normalizeOcrLanguage(localStorage.getItem(OCR_LANGUAGE_STORAGE_KEY)) ?? "auto";
}

export function saveOcrLanguage(language: OcrLanguageId) {
  localStorage.setItem(OCR_LANGUAGE_STORAGE_KEY, language);
}

/** 引擎返回的语言串 → 本地化显示名；组合串（Paddle 自动/manga）一并映射，未知串原样返回。 */
export function ocrLanguageLabel(value: string): string {
  const option = OCR_LANGUAGE_OPTIONS.find((option) => option.id === value);
  if (option) return t(option.labelKey);
  const compositeLabels: Record<string, string> = {
    "zh-Hans+en": t("ocr.language.mixedZhEn"),
    "ja+zh+en": t("ocr.language.mixedJaZhEn"),
  };
  return compositeLabels[value] ?? value;
}
