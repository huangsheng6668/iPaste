import { defineStore } from "pinia";
import { ref } from "vue";
import { ipasteApi } from "../lib/ipasteApi";
import { showError } from "./uiStore";
import { useCloudSyncStore } from "./cloudSyncStore";
import { compareCategoryItemOrder, compareSortOrder, orderCategoriesByIds } from "./lib/ordering";
import type { Category, CategoryItem } from "../types";

const CATEGORY_COLORS = ["#0D9488", "#2563EB", "#7C3AED", "#D97706", "#DC2626", "#475569"];

/**
 * 分类与分类条目的数据域 store（Task 12 从 ipasteStore 拆出）。
 * 只承担数据 CRUD 与云同步触发；面板选择、历史计数等编排留在宿主 store
 * 的同名包装里（见 ipasteStore 注释），依赖方向保持单向。
 */
export const useCategoryStore = defineStore("category", () => {
  const sync = useCloudSyncStore();

  const categories = ref<Category[]>([]);
  const categoryItems = ref<CategoryItem[]>([]);

  async function createCategory(name: string): Promise<Category> {
    const color = CATEGORY_COLORS[categories.value.length % CATEGORY_COLORS.length];
    const category = await ipasteApi.createCategory(name, color);
    categories.value = [...categories.value, category].sort(compareSortOrder);
    sync.syncCloudInBackground();
    return category;
  }

  async function createCategoryWithClip(name: string, clipId: string): Promise<{ category: Category; item: CategoryItem }> {
    const color = CATEGORY_COLORS[categories.value.length % CATEGORY_COLORS.length];
    const { category, item } = await ipasteApi.createCategoryWithClip(name, color, clipId);
    categories.value = [...categories.value, category].sort(compareSortOrder);
    categoryItems.value = [...categoryItems.value, item].sort(compareCategoryItemOrder);
    sync.syncCloudInBackground();
    return { category, item };
  }

  async function renameCategory(category: Category, name: string) {
    const next = await ipasteApi.updateCategory(category.id, name, category.color);
    categories.value = categories.value.map((item) => (item.id === next.id ? next : item));
    sync.syncCloudInBackground();
  }

  async function updateCategoryColor(category: Category, color: string) {
    const next = await ipasteApi.updateCategory(category.id, category.name, color);
    categories.value = categories.value.map((item) => (item.id === next.id ? next : item));
    sync.syncCloudInBackground();
  }

  async function deleteCategory(id: string) {
    await ipasteApi.deleteCategory(id);
    categories.value = categories.value.filter((category) => category.id !== id);
    categoryItems.value = categoryItems.value.filter((item) => item.categoryId !== id);
    sync.syncCloudInBackground();
  }

  /** 返回 created 标记：宿主据此对历史条目做 favoriteCount 提升等联动。 */
  async function addToCategory(clipId: string, categoryId: string): Promise<{ item: CategoryItem; created: boolean }> {
    const item = await ipasteApi.addClipToCategory(clipId, categoryId);
    const existing = categoryItems.value.some((categoryItem) => categoryItem.id === item.id);
    if (!existing) {
      categoryItems.value = [...categoryItems.value, item].sort(compareCategoryItemOrder);
      sync.syncCloudInBackground();
    }
    return { item, created: !existing };
  }

  async function removeCategoryItem(id: string) {
    await ipasteApi.removeCategoryItem(id);
    categoryItems.value = categoryItems.value.filter((item) => item.id !== id);
    sync.syncCloudInBackground();
  }

  async function reorderCategories(categoryIds: string[]): Promise<void> {
    if (categoryIds.length !== categories.value.length) return;

    const previous = categories.value;
    categories.value = orderCategoriesByIds(previous, categoryIds);

    try {
      categories.value = await ipasteApi.reorderCategories(categoryIds);
      sync.syncCloudInBackground();
    } catch (unknownError) {
      categories.value = previous;
      showError(unknownError);
      throw unknownError;
    }
  }

  function patchCategoryItem(item: CategoryItem) {
    categoryItems.value = categoryItems.value.map((categoryItem) =>
      categoryItem.id === item.id ? item : categoryItem,
    );
  }

  return {
    categories,
    categoryItems,
    createCategory,
    createCategoryWithClip,
    renameCategory,
    updateCategoryColor,
    deleteCategory,
    addToCategory,
    removeCategoryItem,
    reorderCategories,
    patchCategoryItem,
  };
});
