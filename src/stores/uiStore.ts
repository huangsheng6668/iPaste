import { defineStore } from "pinia";
import { ref } from "vue";
import { errorMessage } from "../lib/appError";

/** 瞬态 toast 条目（ErrorToast 渲染单元）。 */
export interface UiToast {
  id: number;
  message: string;
}

const TOAST_AUTO_DISMISS_MS = 3200;
const MAX_VISIBLE_TOASTS = 3;

export const useUiStore = defineStore("ui", () => {
  /** 当前可见 toast（最多 3 条，纵向小栈）。 */
  const toasts = ref<ReadonlyArray<UiToast>>([]);
  let nextToastId = 0;
  const dismissTimers = new Map<number, ReturnType<typeof setTimeout>>();

  function clearDismissTimer(id: number) {
    const timer = dismissTimers.get(id);
    if (timer === undefined) return;
    clearTimeout(timer);
    dismissTimers.delete(id);
  }

  function dismissToast(id: number) {
    clearDismissTimer(id);
    toasts.value = toasts.value.filter((toast) => toast.id !== id);
  }

  function pushToast(message: string) {
    // 同文案去重：可见期间重复报错只保留一条。
    if (toasts.value.some((toast) => toast.message === message)) return;

    const next = [...toasts.value, { id: ++nextToastId, message }];
    while (next.length > MAX_VISIBLE_TOASTS) {
      const dropped = next.shift();
      if (dropped) clearDismissTimer(dropped.id);
    }
    toasts.value = next;

    const id = nextToastId;
    dismissTimers.set(
      id,
      setTimeout(() => dismissToast(id), TOAST_AUTO_DISMISS_MS),
    );
  }

  return { toasts, pushToast, dismissToast };
});

/** 瞬态错误通道便捷入口：pushToast(errorMessage(e))。 */
export function showError(error: unknown) {
  useUiStore().pushToast(errorMessage(error));
}
