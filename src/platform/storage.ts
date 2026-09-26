// localStorage 的唯一出口：统一守卫 + 静默降级。
// 读写异常（隐私模式 / 配额 / 被禁用）与无 localStorage 的环境一律静默，
// 与各调用点原先"裸访问或自行 try/catch"相比只增不减保护。
export function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStored(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 存不进就当会话态：见模块注释 */
  }
}

export function removeStored(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* 同上 */
  }
}
