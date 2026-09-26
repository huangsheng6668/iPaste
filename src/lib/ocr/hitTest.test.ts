import { describe, expect, it } from "vitest";
import {
  clientPointToImagePoint,
  hitTolerance,
  nearestWordIndex,
  unionRects,
  wordIndexFromPoint,
  type HitLine,
} from "./hitTest";

/** 造一行：words 按给的 left 依次排开，行高固定 10。 */
function line(top: number, wordLefts: number[], offset = 0, width = 20): HitLine {
  return {
    top,
    height: 10,
    words: wordLefts.map((left, index) => ({
      left,
      top,
      width,
      height: 10,
      selectionIndex: offset + index,
    })),
  };
}

describe("unionRects", () => {
  it("无矩形返回 null", () => {
    expect(unionRects([])).toBeNull();
  });

  it("单个矩形原样返回", () => {
    expect(unionRects([{ left: 1, top: 2, right: 3, bottom: 4 }])).toEqual({ left: 1, top: 2, right: 3, bottom: 4 });
  });

  it("多个矩形取并集", () => {
    expect(unionRects([
      { left: 10, top: 10, right: 20, bottom: 20 },
      { left: 5, top: 30, right: 40, bottom: 35 },
    ])).toEqual({ left: 5, top: 10, right: 40, bottom: 35 });
  });
});

describe("hitTolerance", () => {
  it("按缩放缩放并钳制在 [2, 18]", () => {
    expect(hitTolerance(1)).toBe(6);
    expect(hitTolerance(0.5)).toBe(12);
    expect(hitTolerance(100)).toBe(2);
    expect(hitTolerance(0.001)).toBe(18);
  });

  it("缩放为 0 或负值时按下限保护（不产生 Infinity）", () => {
    expect(hitTolerance(0)).toBe(18);
    expect(hitTolerance(-2)).toBe(18);
  });
});

describe("clientPointToImagePoint", () => {
  const base = {
    stageRect: { left: 0, top: 0, right: 200, bottom: 100 },
    naturalWidth: 200,
    naturalHeight: 100,
    scale: 1,
    pan: { x: 0, y: 0 },
    normalizedRotation: 0,
  };

  it("无缩放无旋转时，舞台中心映射到图片中心", () => {
    expect(clientPointToImagePoint(100, 50, base)).toEqual({ x: 100, y: 50 });
  });

  it("缩放 2 倍时偏移减半", () => {
    expect(clientPointToImagePoint(120, 50, { ...base, scale: 2 })).toEqual({ x: 110, y: 50 });
  });

  it("平移量从客户坐标中扣除", () => {
    expect(clientPointToImagePoint(120, 50, { ...base, pan: { x: 20, y: 0 } })).toEqual({ x: 100, y: 50 });
  });

  it("旋转 90 度后坐标轴互换（带符号）", () => {
    expect(clientPointToImagePoint(110, 50, { ...base, normalizedRotation: 90 })).toEqual({ x: 100, y: 40 });
  });

  it("参数不足返回 null", () => {
    expect(clientPointToImagePoint(100, 50, { ...base, scale: 0 })).toBeNull();
    expect(clientPointToImagePoint(100, 50, { ...base, naturalWidth: 0 })).toBeNull();
    expect(clientPointToImagePoint(100, 50, { ...base, naturalHeight: 0 })).toBeNull();
  });
});

describe("nearestWordIndex", () => {
  const lines = [line(0, [0, 30], 0), line(50, [0, 30], 2)];

  it("无行返回 null", () => {
    expect(nearestWordIndex(0, 0, [])).toBeNull();
  });

  it("落在首行之上取首行首词，落在末行之下取末行末词", () => {
    expect(nearestWordIndex(10, -5, lines)).toBe(0);
    expect(nearestWordIndex(10, 999, lines)).toBe(3);
  });

  it("行内左端取首词、右端取末词、中间取中心最近者", () => {
    expect(nearestWordIndex(-100, 5, lines)).toBe(0);
    expect(nearestWordIndex(999, 5, lines)).toBe(1);
    // 两个词中心分别在 10 与 40：x=38 更靠近第二个词
    expect(nearestWordIndex(38, 5, lines)).toBe(1);
    expect(nearestWordIndex(12, 5, lines)).toBe(0);
  });

  it("按到行带的垂直距离选行（行内间隙归更近的一行）", () => {
    expect(nearestWordIndex(0, 12, lines)).toBe(0);
    expect(nearestWordIndex(0, 48, lines)).toBe(2);
  });

  it("空行返回 null", () => {
    expect(nearestWordIndex(0, 5, [{ top: 0, height: 10, words: [] }])).toBeNull();
  });
});

describe("wordIndexFromPoint", () => {
  const words = [
    { left: 0, top: 0, width: 20, height: 10, selectionIndex: 0 },
    { left: 100, top: 0, width: 20, height: 10, selectionIndex: 1 },
  ];
  const lines = [line(0, [0, 100], 0)];

  it("落在词框内命中该词", () => {
    expect(wordIndexFromPoint({ x: 10, y: 5 }, words, lines, { scale: 1, allowNearest: false })).toBe(0);
    expect(wordIndexFromPoint({ x: 110, y: 5 }, words, lines, { scale: 1, allowNearest: false })).toBe(1);
  });

  it("词框外但在容差内仍命中", () => {
    // scale=1 → 容差 6；x=25 距第一词右边 20 有 5
    expect(wordIndexFromPoint({ x: 25, y: 5 }, words, lines, { scale: 1, allowNearest: false })).toBe(0);
    // x=28 距 20 有 8 > 6，且不允许回落
    expect(wordIndexFromPoint({ x: 28, y: 5 }, words, lines, { scale: 1, allowNearest: false })).toBeNull();
  });

  it("允许回落时未精确命中则取最近词", () => {
    expect(wordIndexFromPoint({ x: 28, y: 5 }, words, lines, { scale: 1, allowNearest: true })).toBe(0);
    expect(wordIndexFromPoint({ x: 95, y: 5 }, words, lines, { scale: 1, allowNearest: true })).toBe(1);
  });
});
