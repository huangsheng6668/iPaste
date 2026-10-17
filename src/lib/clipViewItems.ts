import type { CategoryItem, ClipItem, ClipViewItem } from "../types";

/**
 * ClipViewItem 的稳定包装缓存。
 *
 * 背景：`visibleItems` 与 `toCategoryClipViewItem` 此前每次重算都对底层
 * 条目做 `{...clip, collection}` 展开，生成全新对象。对象引用一变，
 * ClipCard 的 `item` prop 即失效，连带卡片内全部 computed 重算——一次
 * 剪贴板捕获就会放大成 n 张卡的全量重渲染。
 *
 * 用 WeakMap 按底层对象缓存包装结果：底层条目不变 ⇒ 包装对象引用不变，
 * Vue 的 prop 比较直接命中，跳过整棵卡片子树更新。
 *
 * 语义前提：store 中条目一律整体替换、从不原地改字段（ipasteStore 的
 * 不可变更新风格），因此"同一底层对象 ⇒ 同一包装内容"成立。
 */
const historyWrappers = new WeakMap<ClipItem, ClipViewItem>();
const categoryWrappers = new WeakMap<CategoryItem, ClipViewItem>();

/** 历史条目 → `collection: "history"` 包装（同一条目多次调用返回同一引用）。 */
export function historyViewItem(clip: ClipItem): ClipViewItem {
  let wrapper = historyWrappers.get(clip);
  if (!wrapper) {
    wrapper = { ...clip, collection: "history" };
    historyWrappers.set(clip, wrapper);
  }
  return wrapper;
}

/** 分类条目 → `collection: "category"` 包装（同一条目多次调用返回同一引用）。 */
export function categoryViewItem(item: CategoryItem): ClipViewItem {
  let wrapper = categoryWrappers.get(item);
  if (!wrapper) {
    wrapper = { ...item, collection: "category" };
    categoryWrappers.set(item, wrapper);
  }
  return wrapper;
}
