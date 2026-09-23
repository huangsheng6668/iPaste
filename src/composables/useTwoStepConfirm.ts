import { getCurrentInstance, onUnmounted, ref, type Ref } from "vue";

/**
 * 「两击确认 + N 秒重置」状态机：第一次 request(key) 进入确认态，
 * 窗口期内对同一 key 的再次确认由调用方执行动作；超时自动复位。
 * timeoutMs 传 null 关闭自动复位（显式 cancel / confirm 后复位）。
 */
export function useTwoStepConfirm(timeoutMs: number | null = 3000) {
  const confirmingKey = ref<string | null>(null);
  let resetTimer: number | null = null;

  function clearTimer() {
    if (resetTimer !== null) {
      window.clearTimeout(resetTimer);
      resetTimer = null;
    }
  }

  function request(key: string) {
    clearTimer();
    confirmingKey.value = key;
    if (timeoutMs === null) return;
    resetTimer = window.setTimeout(() => {
      confirmingKey.value = null;
      resetTimer = null;
    }, timeoutMs);
  }

  function cancel() {
    clearTimer();
    confirmingKey.value = null;
  }

  const isConfirming = (key: string) => confirmingKey.value === key;

  if (getCurrentInstance()) {
    onUnmounted(clearTimer);
  }

  return { confirmingKey, request, cancel, isConfirming } as {
    confirmingKey: Ref<string | null>;
    request: (key: string) => void;
    cancel: () => void;
    isConfirming: (key: string) => boolean;
  };
}
