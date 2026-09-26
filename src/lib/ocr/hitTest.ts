// OCR 命中测试与坐标换算纯函数（Task 19 从 useImageOcr 搬出）：
// 所有隐式输入（舞台 rect、缩放、平移、旋转、行/词框）改为显式入参，
// 不读 DOM、不读响应式状态，可独立单测。

export type Rect = { left: number; top: number; right: number; bottom: number };
export type Point = { x: number; y: number };
export type Box = { left: number; top: number; width: number; height: number };

/** 参与最近词判定的行（须已按阅读顺序排好）。 */
export type HitLine = {
  top: number;
  height: number;
  words: ReadonlyArray<Box & { selectionIndex: number }>;
};

/** 多个矩形并集；无有效矩形返回 null。 */
export function unionRects(rects: ReadonlyArray<Rect>): Rect | null {
  if (!rects.length) return null;

  const left = Math.min(...rects.map((rect) => rect.left));
  const top = Math.min(...rects.map((rect) => rect.top));
  const right = Math.max(...rects.map((rect) => rect.right));
  const bottom = Math.max(...rects.map((rect) => rect.bottom));
  return { left, top, right, bottom };
}

/** 命中容差：缩得越小容差越大，钳制在 [2, 18] 图片像素。 */
export function hitTolerance(scale: number): number {
  return Math.max(2, Math.min(18, 6 / Math.max(0.001, scale)));
}

export type ImagePointParams = {
  /** 舞台元素客户区矩形。 */
  stageRect: Rect;
  naturalWidth: number;
  naturalHeight: number;
  scale: number;
  pan: Point;
  /** 归一化后的旋转角（度）。 */
  normalizedRotation: number;
};

/** 客户区坐标 → 图片像素坐标（撤销平移、旋转与缩放）；参数不足时返回 null。 */
export function clientPointToImagePoint(
  clientX: number,
  clientY: number,
  params: ImagePointParams,
): Point | null {
  const { stageRect, naturalWidth, naturalHeight, scale, pan, normalizedRotation } = params;
  if (!naturalWidth || !naturalHeight || scale <= 0) return null;

  const centeredX = clientX - stageRect.left - (stageRect.right - stageRect.left) / 2 - pan.x;
  const centeredY = clientY - stageRect.top - (stageRect.bottom - stageRect.top) / 2 - pan.y;
  const radians = -normalizedRotation * Math.PI / 180;
  const rotatedX = centeredX * Math.cos(radians) - centeredY * Math.sin(radians);
  const rotatedY = centeredX * Math.sin(radians) + centeredY * Math.cos(radians);

  return {
    x: rotatedX / scale + naturalWidth / 2,
    y: rotatedY / scale + naturalHeight / 2,
  };
}

/**
 * 最近词命中：先按"到行带的垂直距离"选行，再在行内按横向中心距离选词。
 * y 落在首行之上/末行之下时直接取该行首/末词（与拖选到边缘的预期一致）。
 */
export function nearestWordIndex(x: number, y: number, lines: ReadonlyArray<HitLine>): number | null {
  if (!lines.length) return null;

  if (y <= lines[0].top) return lines[0].words[0]?.selectionIndex ?? null;

  const lastLine = lines[lines.length - 1];
  if (lastLine && y >= lastLine.top + lastLine.height) {
    return lastLine.words[lastLine.words.length - 1]?.selectionIndex ?? null;
  }

  const line = lines
    .map((entry) => ({
      line: entry,
      distance: y < entry.top ? entry.top - y : Math.max(0, y - entry.top - entry.height),
    }))
    .sort((a, b) => a.distance - b.distance)[0]?.line;
  if (!line) return null;

  const words = line.words;
  if (!words.length) return null;
  const firstWord = words[0];
  const lastWord = words[words.length - 1];
  if (x <= firstWord.left + firstWord.width / 2) return firstWord.selectionIndex;
  if (x >= lastWord.left + lastWord.width / 2) return lastWord.selectionIndex;

  return words
    .map((word) => ({
      word,
      distance: Math.abs(x - (word.left + word.width / 2)),
    }))
    .sort((a, b) => a.distance - b.distance)[0]?.word.selectionIndex ?? null;
}

/** 精确命中（词框按容差外扩）；未命中且 allowNearest 时回落到最近词。 */
export function wordIndexFromPoint(
  point: Point,
  words: ReadonlyArray<Box & { selectionIndex: number }>,
  lines: ReadonlyArray<HitLine>,
  options: { scale: number; allowNearest: boolean },
): number | null {
  const tolerance = hitTolerance(options.scale);
  const exactWord = words.find((word) => (
    point.x >= word.left - tolerance
    && point.x <= word.left + word.width + tolerance
    && point.y >= word.top - tolerance
    && point.y <= word.top + word.height + tolerance
  ));
  if (exactWord) return exactWord.selectionIndex;
  if (!options.allowNearest) return null;

  return nearestWordIndex(point.x, point.y, lines);
}
