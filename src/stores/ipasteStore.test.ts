import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { ClipItem, ClipPage, ClipTypeFilter } from "../types";
import { clipMatchesTypeFilter } from "../lib/clipSearch";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    listClips: vi.fn(),
    deleteClip: vi.fn(),
  },
}));

// jsdom 环境提供真实的 localStorage/document；i18n 的模块顶层读取不再需要手动桩。
const { ipasteApi } = await import("../lib/ipasteApi");
const { useIpasteStore } = await import("./ipasteStore");

const listClipsMock = vi.mocked(ipasteApi.listClips);
const deleteClipMock = vi.mocked(ipasteApi.deleteClip);

// c01 最新、c30 最旧，模拟 last_captured_at DESC 的服务端排序。
function makeClip(id: string, order: number, overrides: Partial<ClipItem> = {}): ClipItem {
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
    ...overrides,
  };
}

let allClips: ClipItem[];

/** 与后端 list_clips_page_with_conn 一致的分页实现（OFFSET/LIMIT + has_more + 类型筛选与徽章计数）。 */
function pageOf(offset: number, limit: number, search: string, typeFilter: ClipTypeFilter = "all"): ClipPage {
  const query = search.trim().toLowerCase();
  const searchSource = query ? allClips.filter((clip) => clip.text.toLowerCase().includes(query)) : allClips;
  const source = searchSource.filter((clip) => clipMatchesTypeFilter(clip, typeFilter));
  return {
    clips: source.slice(offset, offset + limit),
    hasMore: offset + limit < source.length,
    totalCount: source.length,
    allCount: allClips.length,
    textCount: searchSource.filter((clip) => clip.clipType !== "image").length,
    imageCount: searchSource.filter((clip) => clip.clipType === "image").length,
  };
}

beforeEach(() => {
  setActivePinia(createPinia());
  vi.clearAllMocks();
  allClips = Array.from({ length: 30 }, (_, index) => makeClip(`c${String(index + 1).padStart(2, "0")}`, index));
  listClipsMock.mockImplementation((offset = 0, limit = 20, search = "", typeFilter: ClipTypeFilter = "all") =>
    Promise.resolve(pageOf(offset, limit, search, typeFilter)),
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
    expect(listClipsMock).toHaveBeenCalledWith(19, 1, "", "all");
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

    expect(listClipsMock).toHaveBeenCalledWith(19, 1, "clip", "all");
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
    resolveFirst({ clips: [makeClip("stale", 99)], hasMore: false, totalCount: 1, allCount: 1, textCount: 1, imageCount: 0 });
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

describe("类型筛选（全部/文本/图片）", () => {
  function hydrateMixedWindow() {
    const store = useIpasteStore();
    store.selectedCategoryId = "history";
    allClips = [
      makeClip("t1", 0),
      makeClip("i1", 1, { clipType: "image", previewText: "图片" }),
      makeClip("t2", 2),
      makeClip("i2", 3, { clipType: "image", previewText: "图片" }),
    ];
    store.clips = pageOf(0, 20, "").clips;
    store.hasMoreClips = false;
    store.clipTotalCount = 4;
    store.visibleHistoryTotalCount = 4;
    store.clipTextCount = 2;
    store.clipImageCount = 2;
    return store;
  }

  it("selectTypeFilter 触发重载并携带筛选值，窗口只保留目标类型，徽章计数不受筛选影响", async () => {
    const store = hydrateMixedWindow();

    store.selectTypeFilter("image");
    await vi.waitFor(() => expect(store.clips.map((clip) => clip.id)).toEqual(["i1", "i2"]));

    expect(listClipsMock).toHaveBeenCalledWith(0, 20, "", "image");
    expect(store.visibleHistoryTotalCount).toBe(2);
    expect(store.clipTextCount).toBe(2);
    expect(store.clipImageCount).toBe(2);
  });

  it("cycleTypeFilter 按 all → text → image → all 循环，delta -1 反向", async () => {
    const store = hydrateMixedWindow();
    expect(store.typeFilter).toBe("all");

    store.cycleTypeFilter(1);
    expect(store.typeFilter).toBe("text");
    await vi.waitFor(() => expect(store.clips.every((clip) => clip.clipType !== "image")).toBe(true));

    store.cycleTypeFilter(1);
    expect(store.typeFilter).toBe("image");

    store.cycleTypeFilter(1);
    expect(store.typeFilter).toBe("all");

    store.cycleTypeFilter(-1);
    expect(store.typeFilter).toBe("image");
  });

  it("相同筛选重复选择不触发重载", () => {
    const store = hydrateMixedWindow();
    listClipsMock.mockClear();

    store.selectTypeFilter("all");

    expect(listClipsMock).not.toHaveBeenCalled();
  });

  it("图片筛选下新捕获的文本条目只记账不进窗口，图片条目正常置顶", () => {
    const store = hydrateMixedWindow();
    store.typeFilter = "image";

    store.upsertClip(makeClip("t3", -1), 5, true);
    expect(store.clips.some((clip) => clip.id === "t3")).toBe(false);
    expect(store.clipTextCount).toBe(3);

    store.upsertClip(makeClip("i3", -2, { clipType: "image", previewText: "图片" }), 6, true);
    expect(store.clips[0]?.id).toBe("i3");
    expect(store.clipImageCount).toBe(3);
  });

  it("筛选态下删除图片条目递减图片计数", async () => {
    const store = hydrateMixedWindow();
    store.typeFilter = "image";
    store.clips = store.clips.filter((clip) => clip.clipType === "image");

    await store.deleteClip("i1");

    expect(store.clipImageCount).toBe(1);
    expect(store.clipTextCount).toBe(2);
    expect(store.clipTotalCount).toBe(3);
  });

  it("resetTypeFilter 归位 all 且不触发重载（面板隐藏路径）", () => {
    const store = hydrateMixedWindow();
    store.typeFilter = "image";
    listClipsMock.mockClear();

    store.resetTypeFilter();

    expect(store.typeFilter).toBe("all");
    expect(listClipsMock).not.toHaveBeenCalled();
  });
});
