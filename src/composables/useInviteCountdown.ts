import { computed, onUnmounted, ref, watch, type ComputedRef, type Ref } from "vue";

/** 邀请有效期内每秒驱动 nowMs 的 mm:ss 倒计时；清空/卸载时清理定时器。 */
export function useInviteCountdown(expiresAt: Ref<number | null>) {
  const nowMs = ref(Date.now());
  let timer: ReturnType<typeof setInterval> | null = null;

  const countdown = computed(() => {
    if (!expiresAt.value) return "00:00";
    const remaining = Math.max(0, Math.floor((expiresAt.value - nowMs.value) / 1000));
    const minutes = String(Math.floor(remaining / 60)).padStart(2, "0");
    const seconds = String(remaining % 60).padStart(2, "0");
    return `${minutes}:${seconds}`;
  });

  const expired = computed(() => {
    if (!expiresAt.value) return false;
    return expiresAt.value - nowMs.value <= 0;
  });

  watch(
    expiresAt,
    (value) => {
      if (value && timer === null) {
        nowMs.value = Date.now();
        timer = setInterval(() => {
          nowMs.value = Date.now();
        }, 1000);
      } else if (!value && timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    },
    { immediate: true },
  );

  onUnmounted(() => {
    if (timer !== null) clearInterval(timer);
  });

  return { countdown, expired } as { countdown: ComputedRef<string>; expired: ComputedRef<boolean> };
}
