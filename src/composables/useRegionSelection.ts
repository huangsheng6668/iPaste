import { computed, ref } from "vue";

export type SelectionRect = { left: number; top: number; width: number; height: number };

/** 方向无关归一化：任意拖拽方向转为正宽高的左上角矩形（CSS 逻辑像素）。 */
export function normalizeSelection(
  startX: number,
  startY: number,
  endX: number,
  endY: number,
): SelectionRect {
  return {
    left: Math.min(startX, endX),
    top: Math.min(startY, endY),
    width: Math.abs(endX - startX),
    height: Math.abs(endY - startY),
  };
}

/** 遮罩窗内拖拽框选状态机；rect 归一化后直接用于渲染与提交。 */
export function useRegionSelection() {
  const origin = ref<{ x: number; y: number } | null>(null);
  const current = ref<{ x: number; y: number } | null>(null);

  const isSelecting = computed(() => origin.value !== null);
  const rect = computed<SelectionRect>(() => {
    if (!origin.value || !current.value) return { left: 0, top: 0, width: 0, height: 0 };
    return normalizeSelection(origin.value.x, origin.value.y, current.value.x, current.value.y);
  });

  function beginSelection(x: number, y: number) {
    origin.value = { x, y };
    current.value = { x, y };
  }

  function updateSelection(x: number, y: number) {
    if (origin.value) current.value = { x, y };
  }

  function endSelection(): SelectionRect | null {
    const result = origin.value && current.value ? rect.value : null;
    origin.value = null;
    current.value = null;
    return result;
  }

  return { rect, isSelecting, beginSelection, updateSelection, endSelection };
}
