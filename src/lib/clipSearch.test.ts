import { describe, it, expect } from "vitest";
import { clipMatchesSearch } from "./clipSearch";

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
