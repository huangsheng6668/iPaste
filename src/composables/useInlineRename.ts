import { nextTick, ref } from "vue";
import { contextItemKey } from "../lib/clipKeys";
import { typeLabel } from "../lib/format";
import type { ClipViewItem } from "../types";

type InlineRenameOptions = {
  /** begin 起始钩子：App 用它先把目标条目同步为选中态（原 startEditingClipName 的索引定位段）。 */
  onBegin?: (item: ClipViewItem) => void;
  /** commit 落库回调：名字已 trim、空串归 null，由 App 注入既有 store.renameClip 路径。 */
  onCommit: (item: ClipViewItem, name: string | null) => void | Promise<void>;
};

/**
 * 列表行内重命名状态机（原 App.vue editingClipKey/editingClipName 内联流）：
 * begin 负责选中同步、初始值（空名回退类型标签）与聚焦行内输入框；
 * commit 匹配当前编辑键才落库并复位；cancel 仅复位不落库。
 * 落库路径由 App 注入，composable 不感知 store。
 */
export function useInlineRename(options: InlineRenameOptions) {
  const renamingKey = ref<string | null>(null);
  const renameValue = ref("");

  async function begin(item: ClipViewItem) {
    options.onBegin?.(item);
    renamingKey.value = contextItemKey(item);
    renameValue.value = item.displayName?.trim() || typeLabel(item.clipType);
    await focusInput();
  }

  async function commit(item: ClipViewItem) {
    if (renamingKey.value !== contextItemKey(item)) return;

    const name = renameValue.value.trim();
    renamingKey.value = null;
    renameValue.value = "";
    await options.onCommit(item, name || null);
  }

  function cancel() {
    renamingKey.value = null;
    renameValue.value = "";
  }

  async function focusInput() {
    await nextTick();
    window.setTimeout(() => {
      const input = document.querySelector<HTMLInputElement>(".clip-title-input");
      input?.focus();
      input?.select();
    }, 40);
  }

  return { renamingKey, renameValue, begin, commit, cancel };
}
