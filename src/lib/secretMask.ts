// 掩码哨兵：后端设置读取层对非空 API Key 的统一掩码（见
// docs/superpowers/specs/2026-11-14-secret-masking-csp-design.md）。
// 前端永不持有明文 Key；输入框 focus 时为哨兵则清空，便于直接输入新值。
export const MASKED_SECRET = "••••••••";

export function isMaskedSecret(value: string): boolean {
  return value.trim() === MASKED_SECRET;
}

export function clearIfMasked(value: string): string {
  return isMaskedSecret(value) ? "" : value;
}
