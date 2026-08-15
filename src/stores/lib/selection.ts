/** 面板选择状态机的纯函数：被 ipasteStore 与键盘导航共用（独立可测）。 */

export function clampIndex(index: number, length: number): number {
  if (length <= 0) return 0;
  return Math.min(Math.max(index, 0), length - 1);
}

export function moveIndex(index: number, delta: number, length: number): number {
  if (length <= 0) return 0;
  return Math.min(Math.max(index + delta, 0), length - 1);
}

export function indexForKey<K>(items: K[], keyOf: (item: K) => string, key: string): number {
  return items.findIndex((item) => keyOf(item) === key);
}
