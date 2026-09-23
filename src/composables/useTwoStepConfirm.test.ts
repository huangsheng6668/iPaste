import { afterEach, describe, expect, it, vi } from "vitest";
import { useTwoStepConfirm } from "./useTwoStepConfirm";

// 透传包装：调用时才解析全局 setTimeout/clearTimeout，
// 保证 vi.useFakeTimers() 替换后 composable 拿到的是假定时器。
vi.stubGlobal("window", {
  setTimeout: (handler: TimerHandler, timeout?: number) => setTimeout(handler, timeout),
  clearTimeout: (id: unknown) => clearTimeout(id as ReturnType<typeof setTimeout>),
});

describe("useTwoStepConfirm", () => {
  afterEach(() => vi.useRealTimers());

  it("request 进入确认态并在超时后自动复位", () => {
    vi.useFakeTimers();
    const { request, isConfirming } = useTwoStepConfirm(3000);
    request("dev-1");
    expect(isConfirming("dev-1")).toBe(true);
    vi.advanceTimersByTime(3001);
    expect(isConfirming("dev-1")).toBe(false);
  });

  it("timeoutMs 为 null 时不自动复位，cancel 手动复位", () => {
    vi.useFakeTimers();
    const { request, cancel, isConfirming } = useTwoStepConfirm(null);
    request("dev-1");
    vi.advanceTimersByTime(10_000);
    expect(isConfirming("dev-1")).toBe(true);
    cancel();
    expect(isConfirming("dev-1")).toBe(false);
  });

  it("重复 request 重置计时窗口", () => {
    vi.useFakeTimers();
    const { request, isConfirming } = useTwoStepConfirm(3000);
    request("a");
    vi.advanceTimersByTime(2000);
    request("a");
    vi.advanceTimersByTime(2000);
    expect(isConfirming("a")).toBe(true);
    vi.advanceTimersByTime(1001);
    expect(isConfirming("a")).toBe(false);
  });
});
