import type { ClipViewItem } from "../types";

/** 条目在 UI 中的稳定键（collection + id）：拖拽、预览、编辑共用的定位方式。 */
export function contextItemKey(item: ClipViewItem): string {
  return `${item.collection}-${item.id}`;
}

/** 应用/复制时后端需要的原始 clips 表 id（分类条目存的是快照外键）。 */
export function originalClipId(item: ClipViewItem): string {
  return item.collection === "history" ? item.id : item.clipSnapshotId;
}
