import type { ImageOcrWord } from "../../types";

// OCR 文本层排版纯函数（Task 18 从 useImageOcr 原样搬出）：
// 只依赖词框几何与引擎标注，不碰 DOM、不碰响应式状态，可独立单测。

/** 源词：引擎返回的词 + 在结果数组中的稳定下标（行内排序的最终兜底键）。 */
export type OcrSourceWord = ImageOcrWord & {
  sourceIndex: number;
};

/** 行种子：结构化分组（引擎给出 block/paragraph/line）或几何分组（按 y 邻近聚合）。 */
export type OcrLineSeed = {
  key: string;
  words: OcrSourceWord[];
  top: number;
  left: number;
  /** 几何分支专有：行底边（结构化分支不需要）。 */
  bottom?: number;
  /** 结构化分支专有：引擎标注的三级序号与首个源词下标。 */
  blockIndex?: number;
  paragraphIndex?: number;
  lineIndex?: number;
  firstSourceIndex?: number;
};

export function buildOcrLineSeeds(words: OcrSourceWord[]): OcrLineSeed[] {
  if (!words.length) return [];

  const hasStructuredLines = words.every((word) => (
    Number.isFinite(word.blockIndex)
    && Number.isFinite(word.paragraphIndex)
    && Number.isFinite(word.lineIndex)
  ));

  if (hasStructuredLines) {
    const groups = new Map<string, {
      key: string;
      words: OcrSourceWord[];
      blockIndex: number;
      paragraphIndex: number;
      lineIndex: number;
      firstSourceIndex: number;
      top: number;
      left: number;
    }>();

    for (const word of words) {
      const blockIndex = word.blockIndex ?? 0;
      const paragraphIndex = word.paragraphIndex ?? 0;
      const lineIndex = word.lineIndex ?? 0;
      const key = `${blockIndex}:${paragraphIndex}:${lineIndex}`;
      const group = groups.get(key);
      if (group) {
        group.words.push(word);
        group.firstSourceIndex = Math.min(group.firstSourceIndex, word.sourceIndex);
        group.top = Math.min(group.top, word.top);
        group.left = Math.min(group.left, word.left);
      } else {
        groups.set(key, {
          key,
          words: [word],
          blockIndex,
          paragraphIndex,
          lineIndex,
          firstSourceIndex: word.sourceIndex,
          top: word.top,
          left: word.left,
        });
      }
    }

    return [...groups.values()].sort((a, b) => (
      a.blockIndex - b.blockIndex
      || a.paragraphIndex - b.paragraphIndex
      || a.lineIndex - b.lineIndex
      || a.firstSourceIndex - b.firstSourceIndex
      || a.top - b.top
      || a.left - b.left
    ));
  }

  const sorted = [...words].sort((a, b) => (a.top - b.top) || (a.left - b.left) || (a.sourceIndex - b.sourceIndex));
  const lines: Array<{
    key: string;
    words: OcrSourceWord[];
    top: number;
    bottom: number;
    left: number;
  }> = [];

  for (const word of sorted) {
    const centerY = word.top + word.height / 2;
    const bestLine = lines
      .map((line) => ({
        line,
        distance: centerY < line.top ? line.top - centerY : Math.max(0, centerY - line.bottom),
      }))
      .sort((a, b) => a.distance - b.distance)[0];
    const tolerance = Math.max(4, word.height * 0.55);

    if (bestLine && bestLine.distance <= tolerance) {
      bestLine.line.words.push(word);
      bestLine.line.top = Math.min(bestLine.line.top, word.top);
      bestLine.line.bottom = Math.max(bestLine.line.bottom, word.top + word.height);
      bestLine.line.left = Math.min(bestLine.line.left, word.left);
    } else {
      lines.push({
        key: `geometry:${lines.length}`,
        words: [word],
        top: word.top,
        bottom: word.top + word.height,
        left: word.left,
      });
    }
  }

  return lines.sort((a, b) => (a.top - b.top) || (a.left - b.left));
}

/** 行内排序：引擎词序号优先，缺失时回落左边界与源下标。 */
export function compareOcrWordsInLine(a: OcrSourceWord, b: OcrSourceWord): number {
  if (Number.isFinite(a.wordIndex) && Number.isFinite(b.wordIndex) && a.wordIndex !== b.wordIndex) {
    return (a.wordIndex ?? 0) - (b.wordIndex ?? 0);
  }
  return (a.left - b.left) || (a.sourceIndex - b.sourceIndex);
}

/** 行文本拼接：按 shouldInsertOcrSpace 的规则决定词间是否补空格。 */
export function joinOcrWords(words: ReadonlyArray<{ text: string }>): string {
  return words.reduce((result, word) => {
    const text = word.text.trim();
    if (!text) return result;
    if (!result) return text;
    const previous = result[result.length - 1] ?? "";
    const separator = shouldInsertOcrSpace(previous, text[0]) ? " " : "";
    return `${result}${separator}${text}`;
  }, "");
}

/** 是否需要在两个词之间插入空格：CJK 之间、闭合标点前、开括号后一律不加。 */
export function shouldInsertOcrSpace(previous: string, next: string): boolean {
  if (!previous || !next) return false;
  const cjkPattern = /[\u3040-\u30ff\u3400-\u9fff\uf900-\ufaff]/u;
  if (cjkPattern.test(previous) || cjkPattern.test(next)) return false;
  if (/^[,.;:!?%)}\]，。；：！？、）】》]/u.test(next)) return false;
  if (/[([{$（【《]$/u.test(previous)) return false;
  return /[A-Za-z0-9)\]}]$/u.test(previous) && /^[A-Za-z0-9({[]/u.test(next);
}
