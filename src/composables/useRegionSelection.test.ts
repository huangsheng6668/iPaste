import { describe, expect, it } from "vitest";
import { normalizeSelection, useRegionSelection } from "./useRegionSelection";

describe("normalizeSelection", () => {
  it("keeps left-to-right rects unchanged", () => {
    expect(normalizeSelection(10, 20, 40, 60)).toEqual({ left: 10, top: 20, width: 30, height: 40 });
  });

  it("normalizes right-to-left drags", () => {
    expect(normalizeSelection(40, 60, 10, 20)).toEqual({ left: 10, top: 20, width: 30, height: 40 });
  });

  it("normalizes mixed directions", () => {
    expect(normalizeSelection(40, 20, 10, 60)).toEqual({ left: 10, top: 20, width: 30, height: 40 });
  });

  it("returns zero-size rect for a click", () => {
    expect(normalizeSelection(5, 5, 5, 5)).toEqual({ left: 5, top: 5, width: 0, height: 0 });
  });
});

describe("useRegionSelection", () => {
  it("tracks a drag and clears state on end", () => {
    const { rect, isSelecting, beginSelection, updateSelection, endSelection } = useRegionSelection();

    expect(isSelecting.value).toBe(false);
    expect(rect.value).toEqual({ left: 0, top: 0, width: 0, height: 0 });

    beginSelection(100, 50);
    expect(isSelecting.value).toBe(true);

    updateSelection(40, 130);
    expect(rect.value).toEqual({ left: 40, top: 50, width: 60, height: 80 });

    const result = endSelection();
    expect(result).toEqual({ left: 40, top: 50, width: 60, height: 80 });
    expect(isSelecting.value).toBe(false);
  });

  it("endSelection without a drag returns null", () => {
    const { endSelection } = useRegionSelection();
    expect(endSelection()).toBeNull();
  });
});
