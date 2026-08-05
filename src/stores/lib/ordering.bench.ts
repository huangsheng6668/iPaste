import { bench, describe } from "vitest";
import { compareSortOrder, orderCategoryItemsByIds } from "./ordering";

describe("ordering", () => {
  bench("compareSortOrder × 100 items", () => {
    const items = Array.from({ length: 100 }, (_, i) => ({ sortOrder: i }));
    items.sort(compareSortOrder);
  });
  bench("orderCategoryItemsByIds 500 items", () => {
    const items = Array.from({ length: 500 }, (_, i) => ({
      id: `item-${i}`,
      sortOrder: 0,
    }));
    const ids = Array.from({ length: 500 }, (_, i) => `item-${499 - i}`);
    orderCategoryItemsByIds(items, ids);
  });
});
