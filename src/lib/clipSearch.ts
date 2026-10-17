import type { ClipType, ClipTypeFilter } from "../types";

export type SearchableClip = {
  displayName?: string | null;
  previewText: string;
  clipType: ClipType;
  text: string;
};

/** 查询词归一化：过滤循环外执行一次，避免每条目重复 trim + toLowerCase。 */
export function normalizeSearchQuery(query: string): string {
  return query.trim().toLowerCase();
}

/**
 * 归一化查询下的匹配（图片条目只搜字面量 "image"，不搜 data URL，与后端同口径）。
 *
 * 性能：此前每条目对 4 个字段分别 toLowerCase（含完整 text），每条目产生
 * 最多 4 份全文字符串临时分配；改为拼接后单次小写化，分配量约为 1/n。
 * 字段间以 \n 分隔，正常查询词不会跨字段误匹配。
 */
export function clipMatchesNormalized(item: SearchableClip, normalized: string): boolean {
  if (!normalized) return true;
  const haystack = `${item.displayName ?? ""}\n${item.previewText}\n${item.clipType}\n${
    item.clipType === "image" ? "image" : item.text
  }`;
  return haystack.toLowerCase().includes(normalized);
}

export function clipMatchesSearch(item: SearchableClip, query: string): boolean {
  return clipMatchesNormalized(item, normalizeSearchQuery(query));
}

/**
 * 类型筛选匹配：与 Rust SQL 同一口径（store/clips.rs 的 clip_type 条件），
 * "text" = 一切非图片（可搜索的文本类），"image" = 仅图片，"all" = 不过滤。
 * 图片稀疏且分页加载，历史列表实际过滤在 SQL 层完成；本函数服务前端增量记账
 * （新捕获条目是否进入当前窗口）与分类视图过滤。
 */
export function clipMatchesTypeFilter(item: { clipType: string }, filter: ClipTypeFilter): boolean {
  if (filter === "image") return item.clipType === "image";
  if (filter === "text") return item.clipType !== "image";
  return true;
}
