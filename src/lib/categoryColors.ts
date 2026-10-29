import type { CSSProperties } from "vue";
import { contrastText, parseRgb } from "./colorFormat";

/** 分类色板是数据而非主题：值与持久化保持不变。 */
export const CATEGORY_COLOR_OPTIONS = [
  "#2563EB",
  "#0891B2",
  "#0D9488",
  "#059669",
  "#65A30D",
  "#CA8A04",
  "#D97706",
  "#EA580C",
  "#DC2626",
  "#E11D48",
  "#DB2777",
  "#C026D3",
  "#9333EA",
  "#7C3AED",
  "#4F46E5",
  "#0284C7",
  "#475569",
  "#334155",
  "#111827",
  "#78716C",
  "#A16207",
  "#BE123C",
  "#6D28D9",
  "#0F766E",
] as const;

/**
 * 由分类色派生 CSS 变量，供圆点、彩色底上的文字/图标与同色系弱底统一消费：
 * - `--category-color`：原始色值
 * - `--category-on-color`：按相对亮度自动选近黑或白
 * - `--category-soft` / `--category-ring`：同色系的弱底与描边
 */
export function categoryColorVars(color: string): CSSProperties {
  const rgb = parseRgb(color);
  return {
    "--category-color": color,
    "--category-on-color": contrastText(color),
    "--category-soft": rgb ? `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, 0.12)` : "transparent",
    "--category-ring": rgb ? `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, 0.36)` : "transparent",
  };
}
