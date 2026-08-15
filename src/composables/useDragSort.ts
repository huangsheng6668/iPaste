import { ref, type CSSProperties } from "vue";

/**
 * 指针拖拽排序引擎（原 App.vue 条目拖拽与 CategoryRail.vue 分类拖拽的并集泛化）。
 *
 * 状态机：handle 按下（canStart 通过）→ 记录起点/ghost 尺寸并挂 window
 * pointermove/up/cancel 监听 → 位移超过 moveThreshold（3px）才进入"已拖动"
 * （draggingKey 置位、ghost 跟随）→ pointerup 时经 reorderedIdsAfter 换算
 * 新顺序并提交 onReorder；pointercancel 只清理不提交。
 *
 * 语义保留点：阈值 3px；放置侧 = 指针相对目标中点（竖向看 Y、横向看 X）；
 * 指针在容器边缘 band 内时按 orientation 轴步进滚动；拖动结束（发生过位移）
 * 经 onDragFinished 供调用点做 suppressNextClick/Select 的 setTimeout(0) 复位。
 */

/** 放置目标发现结果：key/id 与目标矩形（引擎据此计算 before/after）。 */
export type DragSortTarget = { key: string; id: string; rect: DOMRect };

export interface DragSortConfig<T> {
  /** 拖拽句柄按下时调用；返回 false 立即取消（如左键以外的按钮）。 */
  canStart: (payload: { item: T; event: PointerEvent }) => boolean;
  /** 参与排序的当前条目（有序）。 */
  items: () => T[];
  itemKey: (item: T) => string;
  itemId: (item: T) => string;
  /** 放置目标的 DOM 发现：返回 { key, id, rect } 或 null。 */
  targetFromPoint: (clientX: number, clientY: number) => DragSortTarget | null;
  /** 提交排序（已换算好的完整 id 序列）。 */
  onReorder: (orderedIds: string[]) => void | Promise<void>;
  /** 条目容器（边缘滚动用）。 */
  container: () => HTMLElement | null;
  /** 排列方向（允许 getter：CategoryRail 的 orientation 随面板布局变化）。 */
  orientation: "horizontal" | "vertical" | (() => "horizontal" | "vertical");
  /** 拖拽启动阈值 px（默认 3）与边缘滚动带宽 px（默认 48）。 */
  moveThreshold?: number;
  edge?: number;
  onDragStarted?: () => void;
  onDragFinished?: () => void;

  /** 边缘滚动步进 px（默认 14；CategoryRail 为 12）。 */
  scrollStep?: number;
  /** 每次 pointermove 复检；返回 false 时本帧不更新（App: canReorderVisibleItems）。 */
  isActive?: () => boolean;
  /** ghost 变换锁定在 orientation 轴（CategoryRail 的 translate3d 单轴样式）。 */
  lockAxis?: boolean;
  /** 按下时测量 ghost 尺寸的选择器（默认 "[data-item-key]"，CategoryRail 用 "[data-category-id]"）。 */
  sourceSelector?: string;
  /** 每次有效移动的边缘滚动尝试后回调（调用点借此点亮滚动条）。 */
  onEdgeScroll?: (clientX: number, clientY: number) => void;
}

type DragSession = {
  key: string;
  id: string;
  startX: number;
  startY: number;
  width: number;
  height: number;
  hasMoved: boolean;
  targetKey: string | null;
  targetId: string | null;
  side: "before" | "after" | null;
};

/**
 * 纯换算：把 draggedId 移到 targetId 的 before/after 位置。
 * 未知拖拽项或未知目标返回 null；顺序不变时返回原数组引用（供提交守卫做同引用判断）。
 */
export function reorderedIdsAfter(
  currentIds: string[],
  draggedId: string,
  targetId: string,
  side: "before" | "after",
): string[] | null {
  if (!currentIds.includes(draggedId)) return null;

  const remaining = currentIds.filter((id) => id !== draggedId);
  const targetIndex = remaining.indexOf(targetId);
  if (targetIndex < 0) return null;

  const nextIds = [...remaining];
  nextIds.splice(side === "after" ? targetIndex + 1 : targetIndex, 0, draggedId);
  return nextIds.join("\n") === currentIds.join("\n") ? currentIds : nextIds;
}

export function useDragSort<T>(config: DragSortConfig<T>) {
  const draggingKey = ref<string | null>(null);
  const dropTargetKey = ref<string | null>(null);
  const dropSide = ref<"before" | "after" | null>(null);
  const dragOffset = ref({ x: 0, y: 0 });
  let session: DragSession | null = null;

  function orientation(): "horizontal" | "vertical" {
    return typeof config.orientation === "function" ? config.orientation() : config.orientation;
  }

  function start(payload: { item: T; event: PointerEvent }) {
    if (!config.canStart(payload)) return;

    const key = config.itemKey(payload.item);
    const dragSource = (payload.event.currentTarget ?? payload.event.target) as Element | null;
    const card = dragSource?.closest<HTMLElement>(config.sourceSelector ?? "[data-item-key]");
    const rect = card?.getBoundingClientRect();
    config.onDragStarted?.();
    session = {
      key,
      id: config.itemId(payload.item),
      startX: payload.event.clientX,
      startY: payload.event.clientY,
      width: rect?.width ?? 0,
      height: rect?.height ?? 0,
      hasMoved: false,
      targetKey: null,
      targetId: null,
      side: null,
    };
    dragOffset.value = { x: 0, y: 0 };
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", cancelDrag);
  }

  function handlePointerMove(event: PointerEvent) {
    const state = session;
    if (!state || (config.isActive && !config.isActive())) return;

    event.preventDefault();
    if (Math.hypot(event.clientX - state.startX, event.clientY - state.startY) > (config.moveThreshold ?? 3)) {
      if (!state.hasMoved) {
        draggingKey.value = state.key;
      }
      state.hasMoved = true;
    }
    if (!state.hasMoved) return;

    dragOffset.value = {
      x: event.clientX - state.startX,
      y: event.clientY - state.startY,
    };

    const target = config.targetFromPoint(event.clientX, event.clientY);
    if (!target || target.key === state.key) {
      state.targetKey = null;
      state.targetId = null;
      state.side = null;
      dropTargetKey.value = null;
      dropSide.value = null;
      return;
    }

    const side = dropSideFromPoint(target.rect, event.clientX, event.clientY);
    state.targetKey = target.key;
    state.targetId = target.id;
    state.side = side;
    dropTargetKey.value = target.key;
    dropSide.value = side;
    edgeScroll(event.clientX, event.clientY);
  }

  function dropSideFromPoint(rect: DOMRect, clientX: number, clientY: number): "before" | "after" {
    return orientation() === "vertical"
      ? clientY < rect.top + rect.height / 2
        ? "before"
        : "after"
      : clientX < rect.left + rect.width / 2
        ? "before"
        : "after";
  }

  function edgeScroll(clientX: number, clientY: number) {
    const container = config.container();
    if (!container) return;

    const rect = container.getBoundingClientRect();
    const band = config.edge ?? 48;
    const step = config.scrollStep ?? 14;
    if (orientation() === "vertical") {
      if (clientY < rect.top + band) {
        container.scrollTop -= step;
      } else if (clientY > rect.bottom - band) {
        container.scrollTop += step;
      }
    } else if (clientX < rect.left + band) {
      container.scrollLeft -= step;
    } else if (clientX > rect.right - band) {
      container.scrollLeft += step;
    }
    config.onEdgeScroll?.(clientX, clientY);
  }

  async function finishDrag(event?: PointerEvent) {
    event?.preventDefault();

    const state = session;
    if (state?.hasMoved) {
      config.onDragFinished?.();
    }
    cleanup();
    if (!state?.hasMoved || !state.targetId || !state.side || state.id === state.targetId) return;

    const currentIds = config.items().map(config.itemId);
    const nextIds = reorderedIdsAfter(currentIds, state.id, state.targetId, state.side);
    if (!nextIds || nextIds === currentIds) return;

    await config.onReorder(nextIds);
  }

  function cancelDrag() {
    cleanup();
  }

  function cleanup() {
    window.removeEventListener("pointermove", handlePointerMove);
    window.removeEventListener("pointerup", finishDrag);
    window.removeEventListener("pointercancel", cancelDrag);
    session = null;
    draggingKey.value = null;
    dropTargetKey.value = null;
    dropSide.value = null;
    dragOffset.value = { x: 0, y: 0 };
  }

  function dragStyle(item: T): CSSProperties | undefined {
    if (draggingKey.value !== config.itemKey(item)) return undefined;

    const { x, y } = dragOffset.value;
    const transform = config.lockAxis
      ? orientation() === "vertical"
        ? `translate3d(0, ${y}px, 0)`
        : `translate3d(${x}px, 0, 0)`
      : `translate(${x}px, ${y}px)`;
    return {
      transform,
      width: session?.width ? `${session.width}px` : undefined,
      height: session?.height ? `${session.height}px` : undefined,
    };
  }

  return { draggingKey, dropTargetKey, dropSide, dragOffset, start, dragStyle, cleanup };
}
