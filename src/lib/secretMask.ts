// 掩码哨兵：后端设置读取层对非空 API Key 的统一掩码（见
// docs/superpowers/specs/2026-11-14-secret-masking-csp-design.md）。
// 与后端 src-tauri/src/util.rs 的 MASKED_SECRET 必须逐字一致（两侧测试各自钉住）。
// 已知边界：若用户真实 Key 恰为哨兵字面量（8 个 U+2022），将无法通过输入框设置。
// 前端永不持有明文 Key；输入框 focus 时为哨兵则清空便于直接输入新值，
// blur 时为空则回填哨兵，避免「聚焦清空 → 未输入 → 保存」把已存凭据悄悄抹掉。
export const MASKED_SECRET = "••••••••";

export function isMaskedSecret(value: string): boolean {
  return value.trim() === MASKED_SECRET;
}

export function clearIfMasked(value: string): string {
  return isMaskedSecret(value) ? "" : value;
}

/** blur 补偿：空（或纯空白）字段回填哨兵 = 保存时按 KeepExisting 保留已存凭据。 */
export function restoreSentinelIfEmpty(value: string): string {
  return value.trim().length === 0 ? MASKED_SECRET : value;
}
