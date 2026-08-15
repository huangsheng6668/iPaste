/** Rust `error.rs::AppError` 序列化后的形状（Tauri invoke 的 rejection 值）。 */
export type AppErrorShape = { code: string; message: string; params?: unknown };

export function isAppError(value: unknown): value is AppErrorShape {
  if (typeof value !== "object" || value === null) return false;
  const record = value as { code?: unknown; message?: unknown };
  return typeof record.code === "string" && typeof record.message === "string";
}

export function appErrorCode(value: unknown): string | null {
  return isAppError(value) ? value.code : null;
}

export function appErrorParams(value: unknown): unknown {
  return isAppError(value) ? (value.params ?? null) : null;
}

/** 统一的错误文案提取：AppError.message → Error.message → string → String(x)。 */
export function errorMessage(value: unknown): string {
  if (isAppError(value)) return value.message;
  if (value instanceof Error) return value.message;
  if (typeof value === "string") return value;
  return String(value);
}

/** Tauri「命令不存在」错误判定（老版本二进制无新命令时用于降级回退）。 */
export function isCommandMissing(error: unknown, command: string): boolean {
  const message = errorMessage(error).toLowerCase();
  return message.includes(command.toLowerCase()) && (message.includes("command") || message.includes("not found"));
}
