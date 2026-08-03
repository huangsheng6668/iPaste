import { describe, it, expect } from "vitest";
import {
  compareSortOrder,
  compareCategoryItemOrder,
  orderCategoriesByIds,
  orderCategoryItemsByIds,
} from "./ordering";
import type { Category, CategoryItem } from "../../types";

const category = (id: string, sortOrder: number, createdAt: string): Category => ({
  id,
  name: id,
  color: "#000000",
  sortOrder,
  createdAt,
  updatedAt: createdAt,
});

const item = (id: string, categoryId: string, sortOrder: number, createdAt: string, isPinned = false): CategoryItem => ({
  id,
  categoryId,
  clipSnapshotId: `snap-${id}`,
  clipType: "text",
  contentHash: `hash-${id}`,
  displayName: null,
  previewText: "",
  text: "",
  sortOrder,
  createdAt,
  updatedAt: createdAt,
  syncState: "local",
  isPinned,
});

describe("compareSortOrder", () => {
  it("orders by sortOrder ascending", () => {
    expect(compareSortOrder(category("a", 1, "2026-01-01"), category("b", 2, "2026-01-01"))).toBeLessThan(0);
    expect(compareSortOrder(category("a", 2, "2026-01-01"), category("b", 1, "2026-01-01"))).toBeGreaterThan(0);
  });

  it("falls back to createdAt ascending when sortOrder ties", () => {
    expect(compareSortOrder(category("a", 1, "2026-01-01"), category("b", 1, "2026-01-02"))).toBeLessThan(0);
  });
});

describe("compareCategoryItemOrder", () => {
  it("groups by categoryId first", () => {
    expect(compareCategoryItemOrder(item("a", "cat-b", 0, "2026-01-01"), item("b", "cat-a", 0, "2026-01-01"))).toBeGreaterThan(0);
  });

  it("puts pinned items before unpinned within the same category", () => {
    expect(compareCategoryItemOrder(item("a", "cat", 5, "2026-01-01", true), item("b", "cat", 1, "2026-01-01", false))).toBeLessThan(0);
  });

  it("orders by sortOrder ascending then createdAt descending within same pinned/category", () => {
    expect(compareCategoryItemOrder(item("a", "cat", 1, "2026-01-01"), item("b", "cat", 2, "2026-01-01"))).toBeLessThan(0);
    expect(compareCategoryItemOrder(item("a", "cat", 1, "2026-01-02"), item("b", "cat", 1, "2026-01-01"))).toBeLessThan(0);
  });
});

describe("orderCategoriesByIds", () => {
  it("reorders by the given id sequence and rewrites sortOrder", () => {
    const result = orderCategoriesByIds(
      [category("a", 0, "t1"), category("b", 1, "t2"), category("c", 2, "t3")],
      ["c", "a", "b"],
    );
    expect(result.map((c) => c.id)).toEqual(["c", "a", "b"]);
    expect(result.map((c) => c.sortOrder)).toEqual([0, 1, 2]);
  });

  it("drops ids that are not present in the source", () => {
    const result = orderCategoriesByIds([category("a", 0, "t1")], ["a", "missing"]);
    expect(result.map((c) => c.id)).toEqual(["a"]);
  });
});

describe("orderCategoryItemsByIds", () => {
  it("reorders only the target category and preserves other categories", () => {
    const items = [
      item("a1", "cat-1", 0, "t1"),
      item("b1", "cat-2", 0, "t2"),
      item("a2", "cat-1", 1, "t3"),
    ];
    const result = orderCategoryItemsByIds(items, "cat-1", ["a2", "a1"]);
    const cat1 = result.filter((i) => i.categoryId === "cat-1");
    expect(cat1.map((i) => i.id)).toEqual(["a2", "a1"]);
    expect(cat1.map((i) => i.sortOrder)).toEqual([0, 1]);
    expect(result.filter((i) => i.categoryId === "cat-2").map((i) => i.id)).toEqual(["b1"]);
  });
});
