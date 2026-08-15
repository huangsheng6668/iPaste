import { describe, expect, it } from "vitest";
import { reorderedIdsAfter } from "./useDragSort";

describe("reorderedIdsAfter", () => {
  it("inserts before the target", () => {
    expect(reorderedIdsAfter(["a", "b", "c"], "c", "a", "before")).toEqual(["c", "a", "b"]);
  });
  it("inserts after the target", () => {
    expect(reorderedIdsAfter(["a", "b", "c"], "a", "c", "after")).toEqual(["b", "c", "a"]);
  });
  it("returns null for unknown target", () => {
    expect(reorderedIdsAfter(["a"], "a", "zzz", "before")).toBeNull();
    expect(reorderedIdsAfter(["a"], "zzz", "a", "before")).toBeNull();
  });
  it("returns same reference when nothing changes", () => {
    const ids = ["a", "b"];
    expect(reorderedIdsAfter(ids, "a", "b", "before")).toBe(ids);
    expect(reorderedIdsAfter(ids, "b", "a", "after")).toBe(ids);
  });
  it("swaps adjacent items", () => {
    expect(reorderedIdsAfter(["a", "b"], "a", "b", "after")).toEqual(["b", "a"]);
  });
});
