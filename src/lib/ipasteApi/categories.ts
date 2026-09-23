import { invoke } from "@tauri-apps/api/core";
import type { Category, CategoryItem, CategoryWithItem } from "../../types";
import { isCommandMissing } from "../appError";
import { isTauri } from "../env";
import { call } from "./index";
import { buildMockCategory, buildMockCategoryItem, mockCategories, mockCategoryItems, mockClips } from "./mockBackend";

/** 分类域：分类与分类条目的 CRUD、重排，以及从历史片段保存快照。 */
export const categoriesApi = {
  listCategories() {
    return call<Category[]>("list_categories", undefined, mockCategories);
  },
  listCategoryItems() {
    return call<CategoryItem[]>("list_category_items", undefined, mockCategoryItems);
  },
  reorderCategories(categoryIds: string[]) {
    if (!isTauri) {
      const ordered = categoryIds
        .map((id, index) => {
          const category = mockCategories.find((item) => item.id === id);
          return category ? { ...category, sortOrder: index, updatedAt: new Date().toISOString() } : null;
        })
        .filter((item): item is Category => Boolean(item));
      mockCategories.splice(0, mockCategories.length, ...ordered);
      return Promise.resolve(structuredClone(mockCategories));
    }
    return invoke<Category[]>("reorder_categories", { categoryIds });
  },
  reorderCategoryItems(categoryId: string, itemIds: string[]) {
    if (!isTauri) {
      const timestamp = new Date().toISOString();
      const ordered = itemIds
        .map((id, index) => {
          const item = mockCategoryItems.find((entry) => entry.id === id && entry.categoryId === categoryId);
          return item ? { ...item, sortOrder: index, updatedAt: timestamp } : null;
        })
        .filter((item): item is CategoryItem => Boolean(item));
      const otherItems = mockCategoryItems.filter((item) => item.categoryId !== categoryId);
      mockCategoryItems.splice(0, mockCategoryItems.length, ...otherItems, ...ordered);
      return Promise.resolve(structuredClone(mockCategoryItems));
    }
    return invoke<CategoryItem[]>("reorder_category_items", { categoryId, itemIds });
  },
  createCategory(name: string, color: string) {
    return call<Category>("create_category", { name, color }, buildMockCategory(name, color, mockCategories.length));
  },
  async createCategoryWithClip(name: string, color: string, clipId: string) {
    const clip = mockClips.find((item) => item.id === clipId) ?? mockClips[0];
    const category = buildMockCategory(name, color, mockCategories.length);
    const fallback: CategoryWithItem = {
      category,
      item: buildMockCategoryItem(clip, category.id),
    };

    if (!isTauri) return structuredClone(fallback);

    try {
      return await invoke<CategoryWithItem>("create_category_with_clip", { name, color, clipId });
    } catch (unknownError) {
      if (!isCommandMissing(unknownError, "create_category_with_clip")) throw unknownError;

      const created = await invoke<Category>("create_category", { name, color });
      const item = await invoke<CategoryItem>("add_clip_to_category", { clipId, categoryId: created.id });
      return { category: created, item };
    }
  },
  updateCategory(id: string, name: string, color: string) {
    return call<Category>("update_category", { id, name, color }, buildMockCategory(name, color, 0, id));
  },
  deleteCategory(id: string) {
    return call<void>("delete_category", { id });
  },
  addClipToCategory(clipId: string, categoryId: string) {
    const clip = mockClips.find((item) => item.id === clipId) ?? mockClips[0];
    return call<CategoryItem>("add_clip_to_category", { clipId, categoryId }, buildMockCategoryItem(clip, categoryId));
  },
  removeCategoryItem(id: string) {
    return call<void>("remove_category_item", { id });
  },
};
