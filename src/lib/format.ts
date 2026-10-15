import type { ClipType } from "../types";
import { currentLocale, t, type I18nKey } from "../i18n";

export function formatShortcut(shortcut: string) {
  return shortcut
    .replace("CommandOrControl", platformModifier())
    .split("+")
    .join(" + ");
}

// Intl 对象构造昂贵（ICU 初始化），而 formatTime / textStats / pluralText
// 在列表每张卡片、查看器每次按键的路径上都会被调用——此前每次调用都新建
// RelativeTimeFormat / DateTimeFormat / Segmenter / PluralRules。提为模块级
// 单例，locale 变化时重建。
let relativeTimeFormatter: Intl.RelativeTimeFormat | null = null;
let relativeTimeLocale = "";
let dateTimeFormatter: Intl.DateTimeFormat | null = null;
let dateTimeLocale = "";
let pluralRules: Intl.PluralRules | null = null;
let pluralRulesLocale = "";
let graphemeSegmenter: GraphemeSegmenter | null = null;
let segmenterLocale = "";

type GraphemeSegmenter = { segment(input: string): Iterable<unknown> };

function relativeFormatterFor(locale: string): Intl.RelativeTimeFormat {
  if (relativeTimeLocale !== locale || !relativeTimeFormatter) {
    relativeTimeFormatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
    relativeTimeLocale = locale;
  }
  return relativeTimeFormatter;
}

function dateTimeFormatterFor(locale: string): Intl.DateTimeFormat {
  if (dateTimeLocale !== locale || !dateTimeFormatter) {
    dateTimeFormatter = new Intl.DateTimeFormat(locale, {
      month: "short",
      day: "numeric",
    });
    dateTimeLocale = locale;
  }
  return dateTimeFormatter;
}

function pluralRulesFor(locale: string): Intl.PluralRules {
  if (pluralRulesLocale !== locale || !pluralRules) {
    pluralRules = new Intl.PluralRules(locale);
    pluralRulesLocale = locale;
  }
  return pluralRules;
}

function graphemeSegmenterFor(locale: string): GraphemeSegmenter | null {
  if (!("Segmenter" in Intl)) return null;
  if (segmenterLocale !== locale || !graphemeSegmenter) {
    graphemeSegmenter = new (Intl as typeof Intl & {
      Segmenter: new (
        locale?: string,
        options?: { granularity?: "grapheme" | "word" | "sentence" },
      ) => GraphemeSegmenter;
    }).Segmenter(locale, { granularity: "grapheme" });
    segmenterLocale = locale;
  }
  return graphemeSegmenter;
}

export function formatTime(value: string) {
  const timestamp = new Date(value).getTime();
  const delta = timestamp - Date.now();
  const minutes = Math.round(delta / 60_000);
  const locale = currentLocale.value;
  const relativeFormatter = relativeFormatterFor(locale);

  if (Math.abs(minutes) < 1) return t("time.justNow");
  if (Math.abs(minutes) < 60) return relativeFormatter.format(minutes, "minute");

  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return relativeFormatter.format(hours, "hour");

  const days = Math.round(hours / 24);
  if (Math.abs(days) < 7) return relativeFormatter.format(days, "day");

  return dateTimeFormatterFor(locale).format(new Date(value));
}

export function typeLabel(type: ClipType) {
  const labels: Record<ClipType, ReturnType<typeof t>> = {
    text: t("type.text"),
    link: t("type.link"),
    color: t("type.color"),
    image: t("type.image"),
    file: t("type.file"),
    html: t("type.html"),
  };

  return labels[type] ?? t("type.text");
}

export function textStats(text: string) {
  const chars = countCharacters(text);

  if (chars > 999) return t("stats.kChars", { value: (chars / 1000).toFixed(1) });
  return t("stats.chars", { value: chars });
}

export function imageStats(previewText: string) {
  const trimmed = previewText.trim();
  if (!trimmed) return "";

  return trimmed.replace(/^(?:image|图片|画像|이미지|imagen|bild)\s*[:：-]?\s*/i, "").trim();
}

export function clipMetricText(type: ClipType, text: string, previewText: string) {
  return type === "image" ? imageStats(previewText) : textStats(text);
}

const CODE_HINT_PATTERN = /^(const|let|var|function|import|export|class|def|public|private|fn|impl|struct|enum|\{|<|SELECT|INSERT|UPDATE|DELETE)/m;

/** 文本是否呈现为代码（html 视为代码；否则按代码提示行判定）。 */
export function isCodeText(clipType: string, text: string): boolean {
  if (clipType === "html") return true;
  return CODE_HINT_PATTERN.test(text);
}

export function pluralText(keyOne: I18nKey, keyOther: I18nKey, value: number) {
  const category = pluralRulesFor(currentLocale.value).select(value);
  return t(category === "one" ? keyOne : keyOther, { value });
}

export function lineCountText(lines: number) {
  return pluralText("stats.lineOne", "stats.lineOther", lines);
}

function countCharacters(text: string) {
  const segmenter = graphemeSegmenterFor(currentLocale.value);
  if (!segmenter) {
    // 无 Segmenter：按 code point 迭代计数，不物化数组。
    let count = 0;
    for (const _ of text) count += 1;
    return count;
  }
  // 迭代计数：此前的 `[...segmenter.segment(text)].length` 会把整段文本
  // （剪贴板条目可达几十上百 KB）物化成 grapheme 数组，纯浪费。
  let count = 0;
  for (const _ of segmenter.segment(text)) count += 1;
  return count;
}

export function syncStateLabel(value: string) {
  const labels: Record<string, string> = {
    local: t("sync.local"),
    syncing: t("sync.syncing"),
    synced: t("sync.synced"),
    conflict: t("sync.conflict"),
  };

  return labels[value] ?? t("sync.local");
}

export function categoryDisplayName(name: string) {
  const labels: Record<string, string> = {
    "Dev Snippets": t("category.devSnippets"),
    "开发片段": t("category.devSnippets"),
  };

  return labels[name] ?? name;
}

function platformModifier() {
  const platform = navigator.platform.toLowerCase();
  if (platform.includes("mac")) return "Cmd";
  return "Ctrl";
}

export function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
