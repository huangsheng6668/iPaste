import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { ClipItem, ClipPage } from "../types";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    listClips: vi.fn(),
    deleteClip: vi.fn(),
  },
}));

// i18n.ts（经 ipasteStore 传递引入）在模块顶层读取 localStorage/document；
// vitest 跑在 node 环境，先补浏览器全局再动态导入。
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
});
vi.stubGlobal("document", {
  documentElement: { lang: "en" },
  createElement: () => ({}),
});

const { ipasteApi } = await import("../lib/ipasteApi");
const { useIpasteStore } = await import("./ipasteStore");

const listClipsMock = vi.mocked(ipasteApi.listClips);
const deleteClipMock = vi.mocked(ipasteApi.deleteClip);

// c01 最新、c30 最旧，模拟 last_captured_at DESC 的服务端排序。
function makeClip(id: string, order: number): ClipItem {
  const text = `clip ${order}`;
  return {
    id,
    clipType: "text",
    contentHash: id,
    displayName: null,
    previewText: text,
    text,
    sourceApp: "Test",
    lastCapturedAt: new Date(Date.now() - order * 60_000).toISOString(),
    favoriteCount: 0,
    isPinned: false,
  };
}

let allClips: ClipItem[];

/** 与后端 list_clips_page_with_conn 一致的分页实现（OFFSET/LIMIT + has_more）。 */
function pageOf(offset: number, limit: number, search: string): ClipPage {
  const query = search.trim().toLowerCase();
  const source = query ? allClips.filter((clip) => clip.text.toLowerCase().includes(query)) : allClips;
  return {
    clips: source.slice(offset, offset + limit),
    hasMore: offset + limit < source.length,
    totalCount: source.length,
    allCount: allClips.length,
  };
}

beforeEach(() => {
  setActivePinia(createPinia());
  vi.clearAllMocks();
  allClips = Array.from({ length: 30 }, (_, index) => makeClip(`c${String(index + 1).padStart(2, "0")}`, index));
  listClipsMock.mockImplementation((offset = 0, limit = 20, search = "") =>
    Promise.resolve(pageOf(offset, limit, search)),
  );
  deleteClipMock.mockImplementation(async (id: string) => {
    allClips = allClips.filter((clip) => clip.id !== id);
  });
});

function hydrateLoadedWindow() {
  const store = useIpasteStore();
  store.selectedCategoryId = "history";
  store.clips = pageOf(0, 20, "").clips;
  store.hasMoreClips = true;
  store.clipTotalCount = 30;
  store.visibleHistoryTotalCount = 30;
  return store;
}

describe("deleteClip 删除补位", () => {
  it("已加载窗口删掉一条且还有下一页时，从下一页顶部补回一条，窗口保持 20 条", async () => {
    const store = hydrateLoadedWindow();
    const idsBefore = store.clips.map((clip) => clip.id);

    await store.deleteClip("c05");

    expect(store.clips.map((clip) => clip.id)).toEqual([
      ...idsBefore.filter((id) => id !== "c05"),
      "c21",
    ]);
    expect(store.clips).toHaveLength(20);
    expect(listClipsMock).toHaveBeenCalledWith(19, 1, "");
    expect(store.hasMoreClips).toBe(true);
  });

  it("没有下一页时不发起补位请求，窗口正常缩短", async () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    store.clips = allClips.slice(0, 3);
    store.hasMoreClips = false;
    store.clipTotalCount = 3;
    store.visibleHistoryTotalCount = 3;

    await store.deleteClip("c01");

    expect(store.clips.map((clip) => clip.id)).toEqual(["c02", "c03"]);
    expect(listClipsMock).not.toHaveBeenCalled();
  });

  it("hasMore 过期（服务端实际没有更多）时，按响应收口 hasMore 与总数", async () => {
    const store = hydrateLoadedWindow();
    allClips = allClips.slice(0, 19);

    await store.deleteClip("c05");

    expect(store.clips).toHaveLength(19);
    expect(store.hasMoreClips).toBe(false);
    // 服务端删后共 18 条，总数以补位响应的真值为准。
    expect(store.visibleHistoryTotalCount).toBe(18);
    expect(store.clipTotalCount).toBe(18);
  });

  it("搜索态下补位请求携带当前搜索词", async () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    store.search = "clip";
    store.clips = pageOf(0, 20, "clip").clips;
    store.hasMoreClips = pageOf(0, 20, "clip").hasMore;
    store.clipTotalCount = 30;
    store.visibleHistoryTotalCount = 30;

    await store.deleteClip("c05");

    expect(listClipsMock).toHaveBeenCalledWith(19, 1, "clip");
    expect(store.clips).toHaveLength(20);
  });
});

describe("upsertClip 窗口截断与计数", () => {
  it("新条目置顶并截断到 120 条", () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    store.clips = Array.from({ length: 120 }, (_, index) => makeClip(`c${String(index).padStart(3, "0")}`, index));
    store.hasMoreClips = true;

    store.upsertClip(makeClip("new-clip", -1), 121, true);

    expect(store.clips[0]?.id).toBe("new-clip");
    expect(store.clips).toHaveLength(120);
  });

  it("无 totalCount 且已到页尾时递增两个计数", () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    store.clips = [makeClip("c01", 0)];
    store.hasMoreClips = false;
    store.clipTotalCount = 1;
    store.visibleHistoryTotalCount = 1;

    store.upsertClip(makeClip("new-clip", -1));

    expect(store.clipTotalCount).toBe(2);
    expect(store.visibleHistoryTotalCount).toBe(2);
  });
});

describe("reloadClips 竞态守卫", () => {
  it("并发 reload 只采纳最新请求的结果", async () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    let resolveFirst!: (page: ClipPage) => void;
    listClipsMock.mockImplementationOnce(() => new Promise<ClipPage>((resolve) => { resolveFirst = resolve; }));
    listClipsMock.mockImplementationOnce(() => Promise.resolve(pageOf(0, 20, "")));

    const first = store.reloadClips();
    const second = store.reloadClips();
    resolveFirst({ clips: [makeClip("stale", 99)], hasMore: false, totalCount: 1, allCount: 1 });
    await Promise.all([first, second]);

    expect(store.clips.some((clip) => clip.id === "stale")).toBe(false);
    expect(store.clips).toHaveLength(20);
  });

  it("陈旧请求失败不写入 error", async () => {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    let rejectFirst!: (error: Error) => void;
    listClipsMock.mockImplementationOnce(() => new Promise<ClipPage>((_, reject) => { rejectFirst = reject; }));
    listClipsMock.mockImplementationOnce(() => Promise.resolve(pageOf(0, 20, "")));

    const first = store.reloadClips();
    const second = store.reloadClips();
    rejectFirst(new Error("stale request failed"));
    await Promise.all([first, second]);

    expect(store.error).toBeNull();
    expect(store.clips).toHaveLength(20);
  });
});
