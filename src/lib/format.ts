import type { ClipType } from "../types";
import { currentLocale, t, type I18nKey } from "../i18n";

// Intl 构造函数代价不低（相对时间/日期格式化内部要加载 locale 数据），
// 而这些格式化器在列表渲染中对每个条目调用；按 locale 缓存实例复用。
// locale 集合有限（设置页枚举），Map 不会无限增长。
const relativeFormatterCache = new Map<string, Intl.RelativeTimeFormat>();
const monthDayFormatterCache = new Map<string, Intl.DateTimeFormat>();
const pluralRulesCache = new Map<string, Intl.PluralRules>();

function relativeFormatterFor(locale: string) {
  let formatter = relativeFormatterCache.get(locale);
  if (!formatter) {
    formatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
    relativeFormatterCache.set(locale, formatter);
  }
  return formatter;
}

function monthDayFormatterFor(locale: string) {
  let formatter = monthDayFormatterCache.get(locale);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, { month: "short", day: "numeric" });
    monthDayFormatterCache.set(locale, formatter);
  }
  return formatter;
}

function pluralRulesFor(locale: string) {
  let rules = pluralRulesCache.get(locale);
  if (!rules) {
    rules = new Intl.PluralRules(locale);
    pluralRulesCache.set(locale, rules);
  }
  return rules;
}

export function formatShortcut(shortcut: string) {
  return shortcut
    .replace("CommandOrControl", platformModifier())
    .split("+")
    .join(" + ");
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

  return monthDayFormatterFor(locale).format(new Date(value));
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

type GraphemeSegmenter = { segment(input: string): Iterable<unknown> };
const segmenterCache = new Map<string, GraphemeSegmenter | null>();

/** 按字素分词统计字符数（中日韩组合字符按视觉单元计）。Segmenter 构造
 * 成本高，实例按 locale 缓存；运行环境不支持时回退码点计数。 */
function graphemeSegmenterFor(locale: string): GraphemeSegmenter | null {
  if (!segmenterCache.has(locale)) {
    const ctor = (Intl as typeof Intl & {
      Segmenter?: new (
        locale?: string,
        options?: { granularity?: "grapheme" | "word" | "sentence" },
      ) => GraphemeSegmenter;
    }).Segmenter;
    segmenterCache.set(locale, ctor ? new ctor(locale, { granularity: "grapheme" }) : null);
  }
  return segmenterCache.get(locale) ?? null;
}

function countCharacters(text: string) {
  const segmenter = graphemeSegmenterFor(currentLocale.value);
  return segmenter ? [...segmenter.segment(text)].length : Array.from(text).length;
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
