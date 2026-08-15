import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useUiStore } from "./uiStore";

describe("uiStore toasts", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.useRealTimers());

  it("pushes auto-dismissed toasts and dedupes identical messages", () => {
    vi.useFakeTimers();
    const ui = useUiStore();
    ui.pushToast("boom");
    ui.pushToast("boom");
    expect(ui.toasts).toHaveLength(1);
    vi.advanceTimersByTime(3200);
    expect(ui.toasts).toHaveLength(0);
  });

  it("caps visible toasts at 3 by dropping the oldest", () => {
    vi.useFakeTimers();
    const ui = useUiStore();
    ui.pushToast("one");
    ui.pushToast("two");
    ui.pushToast("three");
    ui.pushToast("four");
    expect(ui.toasts.map((toast) => toast.message)).toEqual(["two", "three", "four"]);
    vi.advanceTimersByTime(3200);
    expect(ui.toasts).toHaveLength(0);
  });

  it("dismissToast removes a single toast before its timer fires", () => {
    vi.useFakeTimers();
    const ui = useUiStore();
    ui.pushToast("boom");
    ui.pushToast("stay");
    ui.dismissToast(ui.toasts[0]!.id);
    expect(ui.toasts.map((toast) => toast.message)).toEqual(["stay"]);
    vi.advanceTimersByTime(3200);
    expect(ui.toasts).toHaveLength(0);
  });
});
