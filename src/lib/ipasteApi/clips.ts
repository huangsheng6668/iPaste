import { invoke } from "@tauri-apps/api/core";
import type {
  AppSnapshot,
  CategoryHitGroup,
  CategoryItem,
  ClipItem,
  ClipPage,
  ClipViewItem,
  ClipViewerPayload,
  SearchResult,
} from "../../types";
import { clipMatchesSearch } from "../clipSearch";
import { isTauri } from "../env";
import { call } from "./call";
import { mockCategories, mockCategoryItems, mockClips, mockSnapshot } from "./mockBackend";

/** 剪贴板历史域：快照引导、分页/搜索、条目编辑与删除、复制与回贴、主面板与放大预览窗口。 */
export const clipsApi = {
  snapshot() {
    return call<AppSnapshot>("get_snapshot", undefined, mockSnapshot);
  },
  listClips(offset = 0, limit = 20, search = "") {
    const query = search.trim().toLowerCase();
    const source = query
      ? mockClips.filter((item) => clipMatchesSearch(item, search))
      : mockClips;
    return call<ClipPage>("list_clips", { offset, limit, search }, {
      clips: source.slice(offset, offset + limit),
      hasMore: offset + limit < source.length,
      totalCount: source.length,
      allCount: mockClips.length,
    });
  },
  searchWithFallback(offset = 0, limit = 20, search = "") {
    const query = search.trim().toLowerCase();
    const matchedClips = query
      ? mockClips.filter((item) => clipMatchesSearch(item, search))
      : mockClips;
    if (matchedClips.length > 0) {
      return call<SearchResult>("search_with_fallback", { offset, limit, search }, {
        kind: "history",
        page: {
          clips: matchedClips.slice(offset, offset + limit),
          hasMore: offset + limit < matchedClips.length,
          totalCount: matchedClips.length,
          allCount: mockClips.length,
        },
      });
    }
    const groups: CategoryHitGroup[] = [];
    for (const cat of mockCategories) {
      const items = mockCategoryItems.filter(
        (item) => item.categoryId === cat.id && clipMatchesSearch(item, search),
      );
      if (items.length > 0) groups.push({ category: cat, items });
    }
    return call<SearchResult>("search_with_fallback", { offset, limit, search }, { kind: "categoryHits", groups });
  },
  deleteClip(id: string) {
    if (!isTauri) {
      const index = mockClips.findIndex((item) => item.id === id);
      if (index >= 0) mockClips.splice(index, 1);
    }
    return call<void>("delete_clip", { id });
  },
  clearClips() {
    if (!isTauri) {
      const count = mockClips.length;
      mockClips.splice(0, mockClips.length);
      return Promise.resolve(count);
    }
    return invoke<number>("clear_clips");
  },
  renameClip(id: string, collection: "history" | "category", displayName: string | null) {
    const normalizedName = displayName?.trim() || null;
    const fallback =
      collection === "history"
        ? mockClips.find((item) => item.id === id)
        : mockCategoryItems.find((item) => item.id === id);
    return call<ClipItem | CategoryItem | undefined>(
      "rename_clip",
      { id, collection, displayName: normalizedName },
      fallback ? { ...fallback, displayName: normalizedName } : undefined,
    );
  },
  updateClipContent(id: string, collection: "history" | "category", text: string) {
    const fallback =
      collection === "history"
        ? mockClips.find((item) => item.id === id)
        : mockCategoryItems.find((item) => item.id === id);
    return call<ClipItem | CategoryItem | undefined>(
      "update_clip_content",
      { id, collection, text },
      fallback ? { ...fallback, text, previewText: previewText(text) } : undefined,
    );
  },
  copyClip(clipType: string, text: string) {
    if (!isTauri && navigator.clipboard && clipType !== "image") {
      return navigator.clipboard.writeText(text);
    }
    return call<void>("copy_clip", { clipType, text });
  },
  showPanel() {
    return call<void>("show_panel");
  },
  hidePanel() {
    return call<void>("hide_panel");
  },
  applyClip(id: string, clipType: string, text: string) {
    return call<void>("apply_clip", { id, clipType, text });
  },
  closeClipViewer(label: string) {
    return call<void>("close_clip_viewer", { label });
  },
  openClipViewer(item: ClipViewItem, originalClipId: string, autoRecognize = false) {
    const label = `clip-viewer-${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;
    const payload: ClipViewerPayload = { label, originalClipId, item };
    localStorage.setItem(clipViewerStorageKey(label), JSON.stringify(payload));

    const recognizeParam = autoRecognize ? "&auto-recognize=1" : "";
    if (!isTauri) {
      window.open(
        `${window.location.origin}${window.location.pathname}?window=clip-viewer&label=${encodeURIComponent(label)}${recognizeParam}`,
        label,
        "width=840,height=620",
      );
      return Promise.resolve();
    }

    return invoke<void>("open_clip_viewer", {
      label,
      title: item.displayName?.trim() || item.previewText || "iPaste",
      autoRecognize,
    });
  },
};

export function clipViewerStorageKey(label: string) {
  return `ipaste.clipViewer.${label}`;
}

function previewText(text: string) {
  return text.split(/\s+/).filter(Boolean).join(" ").slice(0, 180);
}
