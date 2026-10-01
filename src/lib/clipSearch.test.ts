import { describe, it, expect } from "vitest";
import { clipMatchesSearch, clipMatchesTypeFilter } from "./clipSearch";

const baseItem = {
  previewText: "Hello World",
  text: "Hello World full body",
  clipType: "text" as const,
  displayName: null,
};

describe("clipMatchesSearch", () => {
  it("returns true for empty query", () => {
    expect(clipMatchesSearch(baseItem, "")).toBe(true);
  });

  it("returns true for whitespace-only query", () => {
    expect(clipMatchesSearch(baseItem, "   ")).toBe(true);
  });

  it("matches case-insensitively in text", () => {
    expect(clipMatchesSearch(baseItem, "HELLO")).toBe(true);
    expect(clipMatchesSearch(baseItem, "world")).toBe(true);
  });

  it("matches previewText", () => {
    expect(clipMatchesSearch({ ...baseItem, previewText: "preview-only" }, "preview-only")).toBe(true);
  });

  it("matches displayName when present", () => {
    expect(clipMatchesSearch({ ...baseItem, displayName: "My Note" }, "note")).toBe(true);
  });

  it("matches clipType", () => {
    expect(clipMatchesSearch({ ...baseItem, clipType: "link" }, "link")).toBe(true);
  });

  it("matches the literal 'image' token for image clips instead of the data URL", () => {
    const imageItem = {
      previewText: "Image 240 x 160",
      text: "data:image/png;base64,iVBORw0KGgo=",
      clipType: "image" as const,
      displayName: null,
    };
    expect(clipMatchesSearch(imageItem, "image")).toBe(true);
    expect(clipMatchesSearch(imageItem, "iVBOR")).toBe(false);
  });

  it("returns false when no field matches", () => {
    expect(clipMatchesSearch(baseItem, "zzz-not-present")).toBe(false);
  });
});

describe("clipMatchesTypeFilter", () => {
  it("all 放行全部类型", () => {
    expect(clipMatchesTypeFilter({ clipType: "text" }, "all")).toBe(true);
    expect(clipMatchesTypeFilter({ clipType: "image" }, "all")).toBe(true);
  });

  it("text 涵盖一切非图片类型（与 Rust SQL 同口径）", () => {
    for (const clipType of ["text", "link", "color", "html", "file"]) {
      expect(clipMatchesTypeFilter({ clipType }, "text")).toBe(true);
    }
    expect(clipMatchesTypeFilter({ clipType: "image" }, "text")).toBe(false);
  });

  it("image 仅匹配图片", () => {
    expect(clipMatchesTypeFilter({ clipType: "image" }, "image")).toBe(true);
    expect(clipMatchesTypeFilter({ clipType: "text" }, "image")).toBe(false);
  });
});
