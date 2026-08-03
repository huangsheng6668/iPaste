import type { ClipType } from "../types";

export type SearchableClip = {
  displayName?: string | null;
  previewText: string;
  clipType: ClipType;
  text: string;
};

export function clipMatchesSearch(item: SearchableClip, query: string): boolean {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return true;
  return [
    item.displayName ?? "",
    item.previewText,
    item.clipType,
    item.clipType === "image" ? "image" : item.text,
  ].some((field) => field.toLowerCase().includes(normalized));
}
