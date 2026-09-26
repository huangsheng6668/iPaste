import { describe, expect, it, vi } from "vitest";
import { computed, ref } from "vue";
import { useOcrSelection } from "./useOcrSelection";
import type { OcrLine } from "../lib/ocr/textLayout";

// 词工厂：box 与选中序号是最小必需字段。
function line(key: string, order: number, texts: string[], startIndex: number, top = 0): OcrLine {
  let index = startIndex;
  const words = texts.map((text, position) => ({
    text,
    left: position * 30,
    top,
    width: 20,
    height: 10,
    confidence: 0.9,
    blockIndex: 0,
    paragraphIndex: 0,
    lineIndex: order,
    wordIndex: position,
    sourceIndex: startIndex + position,
    selectionIndex: index++,
    lineKey: key,
    lineOrder: order,
  }));
  return {
    key,
    text: texts.join(" "),
    left: 0,
    top,
    width: 60,
    height: 10,
    order,
    words,
  };
}

/** 词元素替身：dataset.ocrWordIndex + getBoundingClientRect。 */
function wordElement(selectionIndex: number, rect: Partial<DOMRect> = {}) {
  return {
    dataset: { ocrWordIndex: String(selectionIndex) },
    getBoundingClientRect: () => ({
      left: 100, top: 200, right: 140, bottom: 220, width: 40, height: 20,
      x: 100, y: 200, toJSON: () => ({}), ...rect,
    } as DOMRect),
  } as unknown as HTMLElement;
}

function pointerEvent(init: {
  pointerId?: number;
  button?: number;
  currentTarget?: unknown;
} = {}): PointerEvent & { preventDefault: ReturnType<typeof vi.fn>; stopPropagation: ReturnType<typeof vi.fn> } {
  const preventDefault = vi.fn();
  const stopPropagation = vi.fn();
  return {
    button: init.button ?? 0,
    pointerId: init.pointerId ?? 7,
    clientX: 0,
    clientY: 0,
    currentTarget: init.currentTarget ?? captureElement(),
    preventDefault,
    stopPropagation,
  } as unknown as PointerEvent & { preventDefault: ReturnType<typeof vi.fn>; stopPropagation: ReturnType<typeof vi.fn> };
}

function captureElement() {
  return {
    setPointerCapture: vi.fn(),
    hasPointerCapture: vi.fn(() => true),
    releasePointerCapture: vi.fn(),
  };
}

function setup(options: {
  lines?: OcrLine[];
  pointToIndex?: (event: PointerEvent, allowNearest: boolean) => number | null;
  stage?: HTMLElement | null;
} = {}) {
  const lines = computed<OcrLine[]>(() => options.lines ?? [line("0:0:0", 0, ["alpha", "beta"], 0), line("0:0:1", 1, ["gamma"], 2, 50)]);
  const stageElement = ref<HTMLElement | null>(options.stage ?? null);
  const pointToIndex = vi.fn(options.pointToIndex ?? (() => 1));
  const setSelectionAction = vi.fn();
  const hideSelectionAction = vi.fn();
  const onBeforeDragStart = vi.fn();

  const selection = useOcrSelection({
    lines,
    stageElement,
    pointToIndex,
    onBeforeDragStart,
    setSelectionAction,
    hideSelectionAction,
  });

  return { selection, lines, stageElement, pointToIndex, setSelectionAction, hideSelectionAction, onBeforeDragStart };
}

describe("useOcrSelection 拖选状态机", () => {
  it("非左键不启动拖选", () => {
    const { selection, onBeforeDragStart } = setup();
    const element = captureElement();

    selection.startSelection(pointerEvent({ button: 2, currentTarget: element }), 0);

    expect(selection.selection.value).toBeNull();
    expect(onBeforeDragStart).not.toHaveBeenCalled();
    expect(element.setPointerCapture).not.toHaveBeenCalled();
  });

  it("左键启动：收起上一轮拖拽、捕获指针、选区落在起始词", () => {
    const { selection, onBeforeDragStart } = setup();
    const element = captureElement();
    const event = pointerEvent({ currentTarget: element });

    selection.startSelection(event, 1);

    expect(onBeforeDragStart).toHaveBeenCalledTimes(1);
    expect(selection.selection.value).toEqual({ startIndex: 1, endIndex: 1 });
    expect(element.setPointerCapture).toHaveBeenCalledWith(7);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
  });

  it("移动：命中测试结果扩展选区终点（允许就近回落到最近词）", () => {
    const { selection, pointToIndex } = setup();
    const element = captureElement();
    selection.startSelection(pointerEvent({ currentTarget: element }), 0);

    selection.moveSelection(pointerEvent());
    expect(pointToIndex).toHaveBeenLastCalledWith(expect.anything(), true);
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 1 });

    selection.moveSelection(pointerEvent());
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 1 });
  });

  it("移动：pointerId 不匹配或命中测试返回 null 时保持原选区", () => {
    const { selection } = setup({ pointToIndex: () => null });
    const element = captureElement();
    selection.startSelection(pointerEvent({ currentTarget: element }), 0);

    selection.moveSelection(pointerEvent({ pointerId: 99 }));
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 0 });

    selection.moveSelection(pointerEvent());
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 0 });
  });

  it("收尾：释放指针捕获并结束拖拽（后续移动不再生效）", () => {
    const { selection } = setup();
    const element = captureElement();
    selection.startSelection(pointerEvent({ currentTarget: element }), 0);
    selection.moveSelection(pointerEvent());

    selection.finishSelection(pointerEvent({ currentTarget: element }));

    expect(element.releasePointerCapture).toHaveBeenCalledWith(7);
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 1 });

    selection.moveSelection(pointerEvent());
    expect(selection.selection.value).toEqual({ startIndex: 0, endIndex: 1 });
  });

  it("选区文本跨行拼接、高亮按行分组", () => {
    const { selection } = setup();
    const element = captureElement();
    selection.startSelection(pointerEvent({ currentTarget: element }), 0);
    selection.moveSelection(pointerEvent());

    expect(selection.selectionText.value).toBe("alpha beta");
    expect(selection.selectionHighlights.value).toHaveLength(1);
    // 高亮框相对词框外扩 2px（左/上不小于 0）
    expect(selection.selectionHighlights.value[0]).toMatchObject({ left: 0, top: 0, width: 54, height: 14 });

    selection.finishSelection(pointerEvent({ currentTarget: element }));
    expect(selection.selectionText.value).toBe("alpha beta");
  });

  it("清空选区：区间归零并收起操作条", () => {
    const { selection, hideSelectionAction } = setup();
    const element = captureElement();
    selection.startSelection(pointerEvent({ currentTarget: element }), 0);
    hideSelectionAction.mockClear();

    selection.clearSelection();

    expect(selection.selection.value).toBeNull();
    expect(selection.selectionText.value).toBe("");
    expect(selection.selectionHighlights.value).toEqual([]);
    expect(hideSelectionAction).toHaveBeenCalledTimes(1);
  });

  it("操作条落点：按选中词元素并集定位，元素缺失时收起", () => {
    const stage = {
      querySelectorAll: () => [wordElement(0)],
    } as unknown as HTMLElement;
    const { selection, setSelectionAction, hideSelectionAction } = setup({ stage });
    const element = captureElement();

    selection.startSelection(pointerEvent({ currentTarget: element }), 0);

    expect(setSelectionAction).toHaveBeenCalledTimes(1);
    const action = setSelectionAction.mock.calls[0][0];
    expect(action.text).toBe("alpha");
    expect(action.mode).toBe("copy");
    expect(action.left).toBe(Math.min(window.innerWidth - 132, Math.max(16, 140 - 112)));
    expect(action.top).toBe(Math.min(window.innerHeight - 56, 220 + 8));
    expect(hideSelectionAction).not.toHaveBeenCalled();
  });

  it("操作条落点：无词元素时（rect 并集为空）收起操作条", () => {
    const { selection, setSelectionAction, hideSelectionAction } = setup();
    const element = captureElement();

    selection.startSelection(pointerEvent({ currentTarget: element }), 0);

    expect(setSelectionAction).not.toHaveBeenCalled();
    expect(hideSelectionAction).toHaveBeenCalled();
  });
});
