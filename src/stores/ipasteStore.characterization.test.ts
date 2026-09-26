// ipasteStore 特征测试：逐条记录现状行为，作为 Wave 2（Task 10-17 store 拆分）的等价性判据。
// 这里断言的是"现状"而非"期望"——例如 updateRetentionDays 会触发完整 load()、
// 设置写入存在静默/抛出两条不等价路径，这些是设计正文 §4.1 F2 记录的怪癖，
// 由 Task 39 统一。修改任何断言必须伴随对应任务，并说明旧行为 → 新行为。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { AppSnapshot, AppSettings, Category, CategoryItem, ClipItem, ClipPage, OcrEngine } from "../types";
import { useUiStore } from "./uiStore";
import { useSettingsStore } from "./settingsStore";
import { useCategoryStore } from "./categoryStore";

vi.mock("../lib/ipasteApi", () => ({
  ipasteApi: {
    snapshot: vi.fn(),
    listClips: vi.fn(),
    searchWithFallback: vi.fn(),
    deleteClip: vi.fn(),
    clearClips: vi.fn(),
    reorderCategories: vi.fn(),
    reorderCategoryItems: vi.fn(),
    updateSettings: vi.fn(),
    updateShortcut: vi.fn(),
    updateAppendCopyTimeout: vi.fn(),
    updateOcrEngine: vi.fn(),
  },
}));

import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "./ipasteStore";

const snapshotMock = vi.mocked(ipasteApi.snapshot);
const listClipsMock = vi.mocked(ipasteApi.listClips);
const deleteClipMock = vi.mocked(ipasteApi.deleteClip);
const reorderCategoryItemsMock = vi.mocked(ipasteApi.reorderCategoryItems);
const updateSettingsMock = vi.mocked(ipasteApi.updateSettings);
const updateShortcutMock = vi.mocked(ipasteApi.updateShortcut);
const updateAppendCopyTimeoutMock = vi.mocked(ipasteApi.updateAppendCopyTimeout);
const updateOcrEngineMock = vi.mocked(ipasteApi.updateOcrEngine);

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

function makeCategory(id: string, sortOrder: number): Category {
  return {
    id,
    name: `cat ${id}`,
    color: "#0D9488",
    sortOrder,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
  };
}

function makeCategoryItem(id: string, categoryId: string, order: number, overrides: Partial<CategoryItem> = {}): CategoryItem {
  const text = `item ${order}`;
  return {
    id,
    categoryId,
    clipSnapshotId: `${id}-snap`,
    clipType: "text",
    contentHash: id,
    displayName: null,
    previewText: text,
    text,
    sortOrder: order,
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    syncState: "local",
    isPinned: false,
    ...overrides,
  };
}

function makeSettings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    shortcut: "CommandOrControl+Shift+V",
    ocrShortcut: "CommandOrControl+Shift+O",
    retentionDays: 30,
    appendCopyTimeoutMinutes: 10,
    panelOpenBehavior: "history",
    panelLayout: "top",
    ocrMode: "fast",
    ocrEngine: "local",
    language: "en",
    cloud: { apiAddress: "", apiKey: "", enabled: false, lastConnectedAt: null },
    cloudOcr: { openaiBaseUrl: "", openaiModel: "", openaiApiKey: "", openaiPrompts: [] },
    ...overrides,
  };
}

function makeSnapshot(overrides: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    clips: [],
    hasMoreClips: false,
    clipTotalCount: 0,
    categories: [],
    categoryItems: [],
    shortcut: "CommandOrControl+Shift+V",
    isListening: true,
    isAppendCopyEnabled: false,
    settings: makeSettings(),
    ...overrides,
  };
}

/** 灌入基线的便捷入口：每个用例从明确的初始状态出发。 */
async function hydrate(snapshot: AppSnapshot) {
  snapshotMock.mockResolvedValueOnce(snapshot);
  const store = useIpasteStore();
  await store.load();
  return store;
}

beforeEach(() => {
  vi.resetAllMocks();
  setActivePinia(createPinia());
});

describe("特征：快照装载与选择复位", () => {
  it("load() 灌入 clips/categories/categoryItems 与三个计数字段", async () => {
    const clips = [makeClip("c1", 1), makeClip("c2", 2)];
    const categories = [makeCategory("k1", 0)];
    const categoryItems = [makeCategoryItem("i1", "k1", 0)];
    const store = await hydrate(
      makeSnapshot({ clips, hasMoreClips: true, clipTotalCount: 9, categories, categoryItems }),
    );
    const category = useCategoryStore();

    expect(store.clips).toEqual(clips);
    expect(category.categories).toEqual(categories);
    expect(category.categoryItems).toEqual(categoryItems);
    expect(store.hasMoreClips).toBe(true);
    expect(store.clipTotalCount).toBe(9);
    expect(store.visibleHistoryTotalCount).toBe(9);
    expect(store.isLoading).toBe(false);
    expect(store.error).toBeNull();
  });

  it("load() 把已失效的 selectedCategoryId 复位为 history", async () => {
    // 持续供给快照：两次 load 各取一次。
    snapshotMock.mockResolvedValue(makeSnapshot({ categories: [makeCategory("k1", 0)] }));
    const store = useIpasteStore();
    await store.load();
    store.selectedCategoryId = "ghost";
    await store.load();
    expect(store.selectedCategoryId).toBe("history");
  });

  it("load() 失败写入持久 error 横幅字段而不抛出", async () => {
    snapshotMock.mockRejectedValueOnce({ code: "store_failed", message: "db locked" });
    const store = useIpasteStore();
    await expect(store.load()).resolves.toBeUndefined();
    expect(store.error).toBe("db locked");
  });
});

describe("特征：upsertClip（捕获事件入口）", () => {
  it("新条目插入头部并使两个计数 +1（无更多页时）", async () => {
    const store = await hydrate(makeSnapshot({ clips: [makeClip("a", 1)], clipTotalCount: 1 }));
    const fresh = makeClip("fresh", 0);

    store.upsertClip(fresh);

    expect(store.clips.map((clip) => clip.id)).toEqual(["fresh", "a"]);
    expect(store.clipTotalCount).toBe(2);
    expect(store.visibleHistoryTotalCount).toBe(2);
    expect(store.selectedIndex).toBe(0);
  });

  it("已知条目重复捕获：更新内容、移到头部，计数取事件回传值", async () => {
    const store = await hydrate(
      makeSnapshot({ clips: [makeClip("a", 1), makeClip("b", 2)], clipTotalCount: 2 }),
    );
    const updatedB = makeClip("b", 0, { previewText: "clip updated", text: "clip updated" });

    store.upsertClip(updatedB, 2, false);

    expect(store.clips.map((clip) => clip.id)).toEqual(["b", "a"]);
    expect(store.clips[0].text).toBe("clip updated");
    expect(store.clipTotalCount).toBe(2);
    expect(store.visibleHistoryTotalCount).toBe(2);
  });

  it("已加载窗口截断为 120 条（新条目挤掉最旧一条）", async () => {
    const windowed = Array.from({ length: 120 }, (_, index) => makeClip(`c${index}`, index));
    const store = await hydrate(makeSnapshot({ clips: windowed, hasMoreClips: true, clipTotalCount: 150 }));

    store.upsertClip(makeClip("fresh", 0));

    expect(store.clips).toHaveLength(120);
    expect(store.clips[0].id).toBe("fresh");
    expect(store.clips[119].id).toBe("c118");
    // hasMoreClips 为 true 时不做推断计数，保持服务端值。
    expect(store.clipTotalCount).toBe(150);
  });
});

describe("特征：历史分页", () => {
  it("loadMoreClips 拼接下一页、按 id 去重并翻转 hasMoreClips", async () => {
    const store = await hydrate(
      makeSnapshot({ clips: [makeClip("a", 1), makeClip("b", 2)], hasMoreClips: true, clipTotalCount: 4 }),
    );
    listClipsMock.mockResolvedValueOnce({
      clips: [makeClip("c", 3), makeClip("a", 1)],
      hasMore: false,
      totalCount: 3,
      allCount: 4,
    } satisfies ClipPage);

    await store.loadMoreClips();

    expect(listClipsMock).toHaveBeenCalledWith(2, 20, "");
    expect(store.clips.map((clip) => clip.id)).toEqual(["a", "b", "c"]);
    expect(store.hasMoreClips).toBe(false);
    expect(store.visibleHistoryTotalCount).toBe(3);
    expect(store.clipTotalCount).toBe(4);
  });

  it("reloadClips 竞态守卫：先发的旧请求返回后不覆盖新结果", async () => {
    const store = await hydrate(makeSnapshot());
    const stalePage: ClipPage = { clips: [makeClip("stale", 1)], hasMore: false, totalCount: 1, allCount: 1 };
    const freshPage: ClipPage = { clips: [makeClip("fresh", 1)], hasMore: false, totalCount: 1, allCount: 1 };

    let releaseStale!: (page: ClipPage) => void;
    const stalePromise = new Promise<ClipPage>((resolve) => {
      releaseStale = resolve;
    });
    let call = 0;
    listClipsMock.mockImplementation(() => {
      call += 1;
      return call === 1 ? stalePromise : Promise.resolve(freshPage);
    });

    const first = store.reloadClips();
    const second = store.reloadClips();
    await second;
    expect(store.clips.map((clip) => clip.id)).toEqual(["fresh"]);

    releaseStale(stalePage);
    await first;
    expect(store.clips.map((clip) => clip.id)).toEqual(["fresh"]);
  });

  it("deleteClip 本地移除、计数递减并按缺口补位（backfill）", async () => {
    const store = await hydrate(
      makeSnapshot({ clips: [makeClip("a", 1), makeClip("b", 2)], hasMoreClips: true, clipTotalCount: 2 }),
    );
    deleteClipMock.mockResolvedValueOnce(undefined);
    listClipsMock.mockResolvedValueOnce({
      clips: [makeClip("c", 3)],
      hasMore: true,
      totalCount: 2,
      allCount: 2,
    } satisfies ClipPage);

    await store.deleteClip("a");

    expect(deleteClipMock).toHaveBeenCalledWith("a");
    expect(store.clips.map((clip) => clip.id)).toEqual(["b", "c"]);
    expect(store.clipTotalCount).toBe(2);
    expect(store.visibleHistoryTotalCount).toBe(2);
  });
});

describe("特征：设置写入的两条不等价路径（F2 锚点）", () => {
  it("updateRetentionDays 成功后触发一次完整 load()", async () => {
    await hydrate(makeSnapshot());
    const settings = useSettingsStore();
    updateSettingsMock.mockResolvedValueOnce(makeSettings({ retentionDays: 14 }));

    // hydrate 已消费一次 snapshot，这里只统计本操作触发的完整重载。
    snapshotMock.mockClear();

    await settings.updateRetentionDays(14);

    expect(updateSettingsMock).toHaveBeenCalledWith(14);
    expect(settings.retentionDays).toBe(14);
    expect(snapshotMock).toHaveBeenCalledTimes(1);
  });

  it("updateAppendCopyTimeout 命令缺失时静默容忍且保留乐观值", async () => {
    await hydrate(makeSnapshot());
    const settings = useSettingsStore();
    updateAppendCopyTimeoutMock.mockRejectedValueOnce({
      code: "command_missing",
      message: "Command update_append_copy_timeout not found",
    });

    await expect(settings.updateAppendCopyTimeout(3)).resolves.toBeUndefined();

    expect(updateAppendCopyTimeoutMock).toHaveBeenCalledWith(3);
    expect(settings.appendCopyTimeoutMinutes).toBe(3);
    expect(useUiStore().toasts).toHaveLength(0);
  });

  it("updateAppendCopyTimeout 真实失败时回滚 + toast + 抛出（Task 39 统一后）", async () => {
    await hydrate(makeSnapshot());
    const settings = useSettingsStore();
    updateAppendCopyTimeoutMock.mockRejectedValueOnce({ code: "io", message: "disk full" });

    await expect(settings.updateAppendCopyTimeout(3)).rejects.toMatchObject({ message: "disk full" });
    expect(useUiStore().toasts.map((toast) => toast.message)).toContain("disk full");
    // 旧行为（Task 39 前）：乐观值保留为 3、只 toast 不回滚；
    // 新行为：回滚到写之前的镜像值（快照默认 10），界面不谎报已保存。
    expect(settings.appendCopyTimeoutMinutes).toBe(10);
  });

  it("回显式设置真实失败同样 toast + 抛出（Task 39 统一后新增）", async () => {
    await hydrate(makeSnapshot());
    const settings = useSettingsStore();
    updateShortcutMock.mockRejectedValueOnce({ code: "io", message: "db locked" });

    // 旧行为（Task 39 前）：回显式 setter 裸抛、不 toast（调用方各自兜底，用户可能毫无感知）；
    // 新行为：统一走 toast 通道并继续抛出。
    await expect(settings.updateShortcut("CommandOrControl+Alt+P")).rejects.toMatchObject({
      message: "db locked",
    });
    expect(useUiStore().toasts.map((toast) => toast.message)).toContain("db locked");
  });

  it("updateOcrEngine 脏值先清洗再落库（非法值不会到达后端）", async () => {
    await hydrate(makeSnapshot());
    const settings = useSettingsStore();
    updateOcrEngineMock.mockResolvedValueOnce(makeSettings({ ocrEngine: "local" }));

    // 模拟边界外传来的脏值：参数类型是 OcrEngine，但 clean 层存在的意义就是兜底非法运行时值。
    await settings.updateOcrEngine("bogus" as OcrEngine);

    expect(updateOcrEngineMock).toHaveBeenCalledWith("local");
    expect(settings.ocrEngine).toBe("local");
  });
});

describe("特征：applySettings 广播回填", () => {
  it("回填全部设置字段并触发 setLanguage 副作用", () => {
    const settings = useSettingsStore();
    const next = makeSettings({
      shortcut: "CommandOrControl+Alt+P",
      ocrShortcut: "",
      retentionDays: 7,
      appendCopyTimeoutMinutes: 5,
      panelOpenBehavior: "last_selected",
      panelLayout: "side",
      ocrMode: "best",
      ocrEngine: "openai",
      language: "zh-CN",
    });

    settings.applySettings(next);

    expect(settings.shortcut).toBe("CommandOrControl+Alt+P");
    // 空的 ocrShortcut 回退默认值。
    expect(settings.ocrShortcut).toBe("CommandOrControl+Shift+O");
    expect(settings.retentionDays).toBe(7);
    expect(settings.appendCopyTimeoutMinutes).toBe(5);
    expect(settings.panelOpenBehavior).toBe("last_selected");
    expect(settings.panelLayout).toBe("side");
    expect(settings.ocrMode).toBe("best");
    expect(settings.ocrEngine).toBe("openai");
    expect(settings.language).toBe("zh-CN");
    expect(settings.cloud).toEqual(next.cloud);
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(window.localStorage.getItem("ipaste.language")).toBe("zh-CN");
  });
});

describe("特征：分类重排的乐观更新与失败回滚", () => {
  it("reorderCategoryItems 乐观重排、按新序提交并恢复选中", async () => {
    const items = [
      makeCategoryItem("i1", "k1", 0),
      makeCategoryItem("i2", "k1", 1),
      makeCategoryItem("i3", "k1", 2),
      makeCategoryItem("j1", "k2", 0),
    ];
    const store = await hydrate(makeSnapshot({ categories: [makeCategory("k1", 0), makeCategory("k2", 1)], categoryItems: items }));
    const category = useCategoryStore();
    store.selectCategory("k1");
    store.setSelectedIndex(2); // 选中 i3

    const resolved = [
      makeCategoryItem("i3", "k1", 0),
      makeCategoryItem("i1", "k1", 1),
      makeCategoryItem("i2", "k1", 2),
      makeCategoryItem("j1", "k2", 0),
    ];
    reorderCategoryItemsMock.mockResolvedValueOnce(resolved);

    await store.reorderCategoryItems("k1", ["i3", "i1", "i2"]);

    expect(reorderCategoryItemsMock).toHaveBeenCalledWith("k1", ["i3", "i1", "i2"]);
    expect(category.categoryItems.map((item) => item.id)).toEqual(["i3", "i1", "i2", "j1"]);
    // 选中的条目（i3）在新顺序里仍被选中。
    expect(store.visibleItems[store.selectedIndex]?.id).toBe("i3");
  });

  it("reorderCategoryItems 失败时回滚本地顺序、toast 并抛出", async () => {
    const items = [
      makeCategoryItem("i1", "k1", 0),
      makeCategoryItem("i2", "k1", 1),
    ];
    const store = await hydrate(makeSnapshot({ categories: [makeCategory("k1", 0)], categoryItems: items }));
    const category = useCategoryStore();
    store.selectCategory("k1");
    reorderCategoryItemsMock.mockRejectedValueOnce({ code: "io", message: "sync failed" });

    await expect(store.reorderCategoryItems("k1", ["i2", "i1"])).rejects.toMatchObject({ message: "sync failed" });

    expect(category.categoryItems.map((item) => item.id)).toEqual(["i1", "i2"]);
    expect(useUiStore().toasts.map((toast) => toast.message)).toContain("sync failed");
  });
});

describe("特征：patchItem（clip-updated 事件的落库单元）", () => {
  it("history 已有条目：原位替换", async () => {
    const store = await hydrate(makeSnapshot({ clips: [makeClip("a", 1), makeClip("b", 2)] }));

    store.patchItem("history", makeClip("a", 1, { text: "clip updated", previewText: "clip updated" }));

    expect(store.clips[0].text).toBe("clip updated");
    expect(store.clips.map((clip) => clip.id)).toEqual(["a", "b"]);
  });

  it("history 缺失条目：命中当前搜索时插到头部，否则忽略", async () => {
    const store = await hydrate(makeSnapshot({ clips: [makeClip("a", 1)] }));
    store.search = "needle";

    store.patchItem("history", makeClip("hit", 0, { text: "needle text", previewText: "needle text" }));
    expect(store.clips.map((clip) => clip.id)).toEqual(["hit", "a"]);

    store.patchItem("history", makeClip("miss", 0, { text: "unrelated", previewText: "unrelated" }));
    expect(store.clips.map((clip) => clip.id)).toEqual(["hit", "a"]);
  });

  it("category 条目：按 id 替换", async () => {
    const items = [makeCategoryItem("i1", "k1", 0)];
    const store = await hydrate(makeSnapshot({ categories: [makeCategory("k1", 0)], categoryItems: items }));
    const category = useCategoryStore();

    store.patchItem("category", makeCategoryItem("i1", "k1", 0, { displayName: "renamed" }));

    expect(category.categoryItems[0].displayName).toBe("renamed");
  });
});

describe("特征：applyClipUpdate（clip-updated 事件入口）", () => {
  it("history 合并：移除旧条目、递减两个计数并落入新条目", async () => {
    const store = await hydrate(
      makeSnapshot({ clips: [makeClip("a", 1), makeClip("b", 2)], clipTotalCount: 2 }),
    );

    store.applyClipUpdate({
      collection: "history",
      item: makeClip("merged-new", 0, { text: "merged", previewText: "merged" }),
      mergedFromId: "a",
    });

    expect(store.clips.map((clip) => clip.id)).toEqual(["merged-new", "b"]);
    expect(store.clipTotalCount).toBe(1);
    expect(store.visibleHistoryTotalCount).toBe(1);
  });

  it("category 合并：移除旧条目并替换为新条目", async () => {
    const items = [makeCategoryItem("i1", "k1", 0), makeCategoryItem("i2", "k1", 1)];
    const store = await hydrate(makeSnapshot({ categories: [makeCategory("k1", 0)], categoryItems: items }));
    const category = useCategoryStore();

    store.applyClipUpdate({
      collection: "category",
      item: makeCategoryItem("i2", "k1", 1, { displayName: "merged-name" }),
      mergedFromId: "i1",
    });

    expect(category.categoryItems.map((item) => item.id)).toEqual(["i2"]);
    expect(category.categoryItems[0].displayName).toBe("merged-name");
  });

  it("非合并更新：只走 patch 路径", async () => {
    const store = await hydrate(makeSnapshot({ clips: [makeClip("a", 1)], clipTotalCount: 1 }));

    store.applyClipUpdate({
      collection: "history",
      item: makeClip("a", 1, { text: "edited", previewText: "edited" }),
    });

    expect(store.clips.map((clip) => clip.id)).toEqual(["a"]);
    expect(store.clips[0].text).toBe("edited");
    expect(store.clipTotalCount).toBe(1);
  });
});
