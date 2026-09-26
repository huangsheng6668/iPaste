import { describe, expect, it } from "vitest";
import {
  buildOcrLineSeeds,
  compareOcrWordsInLine,
  joinOcrWords,
  shouldInsertOcrSpace,
  type OcrSourceWord,
} from "./textLayout";

// 词工厂：只给排版相关的字段赋有意义的值，其余取中性默认。
function word(overrides: Partial<OcrSourceWord> & { text: string; left: number; top: number }): OcrSourceWord {
  return {
    sourceIndex: 0,
    width: 10,
    height: 10,
    confidence: 0.9,
    blockIndex: 0,
    paragraphIndex: 0,
    lineIndex: 0,
    wordIndex: 0,
    ...overrides,
  };
}

/** 模拟引擎未提供结构化标注（lineIndex 等为 NaN/undefined）。 */
function geometryWord(overrides: Partial<OcrSourceWord> & { text: string; left: number; top: number }): OcrSourceWord {
  return {
    ...word(overrides),
    // Number.isFinite(NaN) === false，触发几何分组分支
    blockIndex: Number.NaN,
    paragraphIndex: Number.NaN,
    lineIndex: Number.NaN,
  };
}

describe("buildOcrLineSeeds", () => {
  it("空输入返回空数组", () => {
    expect(buildOcrLineSeeds([])).toEqual([]);
  });

  it("结构化标注：按 block/paragraph/line 分组并按三级序号排序", () => {
    const seeds = buildOcrLineSeeds([
      word({ text: "b", left: 20, top: 10, lineIndex: 1, wordIndex: 1, sourceIndex: 1 }),
      word({ text: "a", left: 10, top: 10, lineIndex: 1, wordIndex: 0, sourceIndex: 0 }),
      word({ text: "c", left: 5, top: 40, lineIndex: 2, wordIndex: 0, sourceIndex: 2 }),
    ]);

    expect(seeds.map((seed) => seed.key)).toEqual(["0:0:1", "0:0:2"]);
    expect(seeds[0].words.map((entry) => entry.text)).toEqual(["b", "a"]);
    // 行框取组内词的最小 top/left
    expect(seeds[0].top).toBe(10);
    expect(seeds[0].left).toBe(10);
    expect(seeds[1].words.map((entry) => entry.text)).toEqual(["c"]);
  });

  it("结构化标注：跨段落按 paragraphIndex 排序优先于 lineIndex", () => {
    const seeds = buildOcrLineSeeds([
      word({ text: "p1", left: 0, top: 100, paragraphIndex: 1, lineIndex: 0, sourceIndex: 0 }),
      word({ text: "p0", left: 0, top: 10, paragraphIndex: 0, lineIndex: 0, sourceIndex: 1 }),
    ]);

    expect(seeds.map((seed) => seed.words[0].text)).toEqual(["p0", "p1"]);
  });

  it("几何分组：同一视觉行的词合并为一行", () => {
    const seeds = buildOcrLineSeeds([
      geometryWord({ text: "one", left: 10, top: 10, sourceIndex: 0 }),
      geometryWord({ text: "two", left: 30, top: 12, sourceIndex: 1 }),
      geometryWord({ text: "next", left: 10, top: 60, sourceIndex: 2 }),
    ]);

    expect(seeds).toHaveLength(2);
    expect(seeds[0].words.map((entry) => entry.text)).toEqual(["one", "two"]);
    expect(seeds[1].words.map((entry) => entry.text)).toEqual(["next"]);
  });

  it("几何分组：垂直距离超过容差则另起一行（容差 = max(4, height*0.55)）", () => {
    // height=10 → 行带 [0,10]，容差 5.5；距离按中心到行带的间隔计（不是到首词中心）。
    // b.top=10 → 中心 15，间隔 5 ≤ 5.5：同一行。
    const within = buildOcrLineSeeds([
      geometryWord({ text: "a", left: 0, top: 0, sourceIndex: 0 }),
      geometryWord({ text: "b", left: 20, top: 10, sourceIndex: 1 }),
    ]);
    expect(within).toHaveLength(1);

    // b.top=12 → 中心 17，间隔 7 > 5.5：另起一行。
    const beyond = buildOcrLineSeeds([
      geometryWord({ text: "a", left: 0, top: 0, sourceIndex: 0 }),
      geometryWord({ text: "b", left: 20, top: 12, sourceIndex: 1 }),
    ]);
    expect(beyond).toHaveLength(2);
  });

  it("几何分组：行按 top/left 排序，key 为 geometry:序号", () => {
    const seeds = buildOcrLineSeeds([
      geometryWord({ text: "lower", left: 0, top: 100, sourceIndex: 0 }),
      geometryWord({ text: "upper", left: 0, top: 0, sourceIndex: 1 }),
    ]);

    expect(seeds.map((seed) => seed.words[0].text)).toEqual(["upper", "lower"]);
    expect(seeds.map((seed) => seed.key)).toEqual(["geometry:0", "geometry:1"]);
  });

  it("混合输入（部分词缺结构化标注）整体走几何分组", () => {
    const seeds = buildOcrLineSeeds([
      word({ text: "structured", left: 0, top: 0, lineIndex: 0, sourceIndex: 0 }),
      geometryWord({ text: "loose", left: 30, top: 1, sourceIndex: 1 }),
    ]);

    expect(seeds).toHaveLength(1);
    expect(seeds[0].key).toBe("geometry:0");
    expect(seeds[0].words).toHaveLength(2);
  });
});

describe("compareOcrWordsInLine", () => {
  it("都有效时按 wordIndex 排序", () => {
    const a = word({ text: "a", left: 100, top: 0, wordIndex: 1 });
    const b = word({ text: "b", left: 10, top: 0, wordIndex: 0 });
    expect(compareOcrWordsInLine(a, b)).toBeGreaterThan(0);
  });

  it("wordIndex 相同或缺失时按 left、再按 sourceIndex", () => {
    const left = word({ text: "l", left: 5, top: 0, wordIndex: Number.NaN, sourceIndex: 9 });
    const right = word({ text: "r", left: 50, top: 0, wordIndex: Number.NaN, sourceIndex: 1 });
    expect(compareOcrWordsInLine(left, right)).toBeLessThan(0);

    const sameLeftEarly = word({ text: "e", left: 5, top: 0, wordIndex: Number.NaN, sourceIndex: 1 });
    const sameLeftLate = word({ text: "l", left: 5, top: 0, wordIndex: Number.NaN, sourceIndex: 9 });
    expect(compareOcrWordsInLine(sameLeftEarly, sameLeftLate)).toBeLessThan(0);
  });
});

describe("joinOcrWords", () => {
  it("拉丁词之间补空格、CJK 之间不补", () => {
    expect(joinOcrWords([{ text: "hello" }, { text: "world" }])).toBe("hello world");
    expect(joinOcrWords([{ text: "你好" }, { text: "世界" }])).toBe("你好世界");
  });

  it("跳过空白词并去除词首尾空白", () => {
    expect(joinOcrWords([{ text: "  a  " }, { text: "   " }, { text: "b" }])).toBe("a b");
  });

  it("闭合标点前不补空格", () => {
    expect(joinOcrWords([{ text: "done" }, { text: "." }])).toBe("done.");
  });

  it("空输入返回空串", () => {
    expect(joinOcrWords([])).toBe("");
  });
});

describe("shouldInsertOcrSpace", () => {
  it("任一侧为空返回 false", () => {
    expect(shouldInsertOcrSpace("", "a")).toBe(false);
    expect(shouldInsertOcrSpace("a", "")).toBe(false);
  });

  it("CJK 与拉丁相邻不补空格", () => {
    expect(shouldInsertOcrSpace("中文", "a")).toBe(false);
    expect(shouldInsertOcrSpace("a", "中")).toBe(false);
    expect(shouldInsertOcrSpace("カタカナ", "a")).toBe(false);
  });

  it("闭合标点前与开括号后不补空格", () => {
    expect(shouldInsertOcrSpace("word", "，")).toBe(false);
    expect(shouldInsertOcrSpace("word", ")")).toBe(false);
    expect(shouldInsertOcrSpace("(", "word")).toBe(false);
    expect(shouldInsertOcrSpace("（", "word")).toBe(false);
  });

  it("拉丁/数字词之间补空格", () => {
    expect(shouldInsertOcrSpace("hello", "world")).toBe(true);
    expect(shouldInsertOcrSpace("1", "2")).toBe(true);
    expect(shouldInsertOcrSpace("a", "(b")).toBe(true);
  });

  it("以标点结尾时不再补空格", () => {
    expect(shouldInsertOcrSpace(".", "a")).toBe(false);
    expect(shouldInsertOcrSpace("，", "a")).toBe(false);
  });
});
