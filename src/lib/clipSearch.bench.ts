import { bench, describe } from "vitest";
import { clipMatchesSearch } from "./clipSearch";
import type { ClipItem } from "../types";

function mockClips(n: number): ClipItem[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `clip-${i}`,
    clipType: "text" as const,
    contentHash: `hash-${i}`,
    previewText: `item ${i} hello world`,
    text: `item ${i} hello world`,
    lastCapturedAt: new Date().toISOString(),
    favoriteCount: 0,
    isPinned: false,
  }));
}

describe("clipSearch", () => {
  bench("filter 1k clips, matching query", () => {
    const clips = mockClips(1000);
    clips.filter((c) => clipMatchesSearch(c, "hello"));
  });
  bench("filter 5k clips, no match", () => {
    const clips = mockClips(5000);
    clips.filter((c) => clipMatchesSearch(c, "zzznomatch"));
  });
});
