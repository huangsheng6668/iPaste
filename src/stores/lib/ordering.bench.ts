import { bench, describe } from "vitest";
import { compareSortOrder, orderCategoryItemsByIds } from "./ordering";
import type { Category, CategoryItem } from "../../types";

function mockCategories(n: number): Category[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `cat-${i}`,
    name: `category ${i}`,
    color: "#2563EB",
    sortOrder: i,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  }));
}

function mockCategoryItems(n: number): CategoryItem[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `item-${i}`,
    categoryId: `cat-${i % 5}`,
    clipSnapshotId: `snap-${i}`,
    clipType: "text" as const,
    contentHash: `hash-${i}`,
    displayName: null,
    previewText: `item ${i}`,
    text: `item ${i}`,
    sortOrder: 0,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    syncState: "local" as const,
    isPinned: false,
  }));
}

describe("ordering", () => {
  bench("compareSortOrder × 100 categories", () => {
    const items = mockCategories(100);
    items.sort(compareSortOrder);
  });
  bench("orderCategoryItemsByIds 500 items", () => {
    const items = mockCategoryItems(500);
    const ids = Array.from({ length: 100 }, (_, i) => `item-${i}`);
    orderCategoryItemsByIds(items, "cat-0", ids);
  });
});
