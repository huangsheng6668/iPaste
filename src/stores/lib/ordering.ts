import type { Category, CategoryItem } from "../../types";

export function compareSortOrder(left: Category, right: Category): number {
  return left.sortOrder - right.sortOrder || left.createdAt.localeCompare(right.createdAt);
}

export function compareCategoryItemOrder(left: CategoryItem, right: CategoryItem): number {
  if (left.categoryId !== right.categoryId) return left.categoryId.localeCompare(right.categoryId);
  if (left.isPinned !== right.isPinned) return left.isPinned ? -1 : 1;
  return left.sortOrder - right.sortOrder || right.createdAt.localeCompare(left.createdAt);
}

export function orderCategoriesByIds(items: Category[], ids: string[]): Category[] {
  const byId = new Map(items.map((item) => [item.id, item]));
  return ids
    .map((id, index) => {
      const item = byId.get(id);
      return item ? { ...item, sortOrder: index } : null;
    })
    .filter((item): item is Category => Boolean(item));
}

export function orderCategoryItemsByIds(items: CategoryItem[], categoryId: string, ids: string[]): CategoryItem[] {
  const byId = new Map(items.filter((item) => item.categoryId === categoryId).map((item) => [item.id, item]));
  const reordered = ids
    .map((id, index) => {
      const item = byId.get(id);
      return item ? { ...item, sortOrder: index } : null;
    })
    .filter((item): item is CategoryItem => Boolean(item));
  return [
    ...items.filter((item) => item.categoryId !== categoryId),
    ...reordered,
  ].sort(compareCategoryItemOrder);
}
