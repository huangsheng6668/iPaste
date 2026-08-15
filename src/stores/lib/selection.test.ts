import { describe, expect, it } from "vitest";
import { clampIndex, indexForKey, moveIndex } from "./selection";

describe("clampIndex", () => {
  it("clamps into [0, length-1] and treats empty as 0", () => {
    expect(clampIndex(5, 3)).toBe(2);
    expect(clampIndex(-1, 3)).toBe(0);
    expect(clampIndex(1, 0)).toBe(0);
  });
});

describe("moveIndex", () => {
  it("moves by delta and stays in bounds", () => {
    expect(moveIndex(2, 2, 5)).toBe(4);
    expect(moveIndex(2, -5, 5)).toBe(0);
    expect(moveIndex(4, 2, 5)).toBe(4);
    expect(moveIndex(0, -1, 0)).toBe(0);
  });
});

describe("indexForKey", () => {
  it("finds the index of the item whose key matches", () => {
    const items = [{ id: "a" }, { id: "b" }];
    expect(indexForKey(items, (item) => item.id, "b")).toBe(1);
    expect(indexForKey(items, (item) => item.id, "zzz")).toBe(-1);
  });
});
