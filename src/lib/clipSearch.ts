import type { ClipType, ClipTypeFilter } from "../types";

export type SearchableClip = {
  displayName?: string | null;
  previewText: string;
  clipType: ClipType;
  text: string;
};

export function clipMatchesSearch(item: SearchableClip, query: string): boolean {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return true;
  return [
    item.displayName ?? "",
    item.previewText,
    item.clipType,
    item.clipType === "image" ? "image" : item.text,
  ].some((field) => field.toLowerCase().includes(normalized));
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
