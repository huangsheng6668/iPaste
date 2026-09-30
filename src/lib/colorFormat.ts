/** 颜色格式展示项（解析成功为 HEX/RGB/HSL/CSS，颜色样式但暂不可解析时回退 RAW/CSS）。 */
export interface ColorFormatItem {
  label: string;
  value: string;
}

const COLOR_LIKE_PATTERN = /^(?:#|rgba?\(|hsla?\()/i;

/**
 * 解析颜色字符串为 `[r, g, b, a]`（a 为 0–1）；无法解析返回 null。
 * 仅支持 `#rgb` / `#rrggbb` / `#rrggbbaa` 与 `rgb()` / `rgba()`——
 * 与 `parseColorFormats` 的历史行为一致（`hsl()` 之类一律判为不可解析）。
 */
export function parseRgb(value: string): [number, number, number, number] | null {
  const str = value.trim();
  if (!str) return null;

  const hexMatch = str.match(/^#?([0-9a-f]{3,8})$/i);
  if (hexMatch) {
    const hex = hexMatch[1];
    if (hex.length === 3) {
      return [
        parseInt(hex[0] + hex[0], 16),
        parseInt(hex[1] + hex[1], 16),
        parseInt(hex[2] + hex[2], 16),
        1,
      ];
    }
    if (hex.length === 6) {
      return [
        parseInt(hex.slice(0, 2), 16),
        parseInt(hex.slice(2, 4), 16),
        parseInt(hex.slice(4, 6), 16),
        1,
      ];
    }
    if (hex.length === 8) {
      return [
        parseInt(hex.slice(0, 2), 16),
        parseInt(hex.slice(2, 4), 16),
        parseInt(hex.slice(4, 6), 16),
        Math.round((parseInt(hex.slice(6, 8), 16) / 255) * 100) / 100,
      ];
    }
    return null;
  }

  const rgbMatch = str.match(/rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)/i);
  if (!rgbMatch) return null;

  return [
    Math.min(255, parseInt(rgbMatch[1], 10)),
    Math.min(255, parseInt(rgbMatch[2], 10)),
    Math.min(255, parseInt(rgbMatch[3], 10)),
    rgbMatch[4] !== undefined ? parseFloat(rgbMatch[4]) : 1,
  ];
}

/** WCAG 2.1 相对亮度。 */
function relativeLuminance(r: number, g: number, b: number): number {
  const channel = (value: number) => {
    const s = value / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** 前景色候选：与 --text-1（浅色主题）同值的近黑，以及纯白。 */
const FOREGROUND_DARK = "#17181c";
const FOREGROUND_LIGHT = "#ffffff";

/**
 * 为任意背景色挑选对比度更高的前景色（近黑或纯白）。
 *
 * 分类色板横跨明度两端：`#111827` 这类近黑配白字没问题，但 `#CA8A04` / `#65A30D`
 * 这类亮色配白字只有约 2.9:1，低于非文本对比度 3:1 的下限——色板上的勾选图标
 * 与分类角标数字会糊掉。按相对亮度在两端各算一次，取对比度更高者，
 * 于是整块色板都能落进可读区间。
 */
export function contrastText(background: string): string {
  const parsed = parseRgb(background);
  if (!parsed) return FOREGROUND_LIGHT;

  const [r, g, b] = parsed;
  const bg = relativeLuminance(r, g, b);
  const dark = relativeLuminance(0x17, 0x18, 0x1c);

  const withDark = (Math.max(bg, dark) + 0.05) / (Math.min(bg, dark) + 0.05);
  const withLight = (Math.max(bg, 1) + 0.05) / (Math.min(bg, 1) + 0.05);

  return withDark >= withLight ? FOREGROUND_DARK : FOREGROUND_LIGHT;
}

/**
 * 解析颜色字符串为 HEX/RGB/HSL 展示值；空串或非颜色样式文本返回 null。
 * 解析算法下沉自 ClipInspectorPane 的 colorFormats computed，行为保持一致：
 * 颜色样式但格式暂不支持（如 hsl()）时回退 RAW/CSS 两项。
 */
export function parseColorFormats(value: string): ColorFormatItem[] | null {
  const str = value.trim();
  if (!str || !COLOR_LIKE_PATTERN.test(str)) return null;

  const parsed = parseRgb(str);
  if (!parsed) {
    return [
      { label: "RAW", value: str },
      { label: "CSS", value: `color: ${str};` },
    ];
  }

  const [r, g, b, a] = parsed;
  const toHex = (n: number) => n.toString(16).padStart(2, "0").toUpperCase();
  const hexVal = `#${toHex(r)}${toHex(g)}${toHex(b)}`;
  const rgbVal = a === 1 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${a})`;

  const rNorm = r / 255, gNorm = g / 255, bNorm = b / 255;
  const max = Math.max(rNorm, gNorm, bNorm), min = Math.min(rNorm, gNorm, bNorm);
  let h = 0, s = 0, l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case rNorm: h = (gNorm - bNorm) / d + (gNorm < bNorm ? 6 : 0); break;
      case gNorm: h = (bNorm - rNorm) / d + 2; break;
      case bNorm: h = (rNorm - gNorm) / d + 4; break;
    }
    h = Math.round(h * 60);
  }
  s = Math.round(s * 100);
  l = Math.round(l * 100);
  const hslVal = a === 1 ? `hsl(${h}, ${s}%, ${l}%)` : `hsla(${h}, ${s}%, ${l}%, ${a})`;

  return [
    { label: "HEX", value: hexVal },
    { label: "RGB", value: rgbVal },
    { label: "HSL", value: hslVal },
    { label: "CSS", value: `color: ${hexVal};` },
  ];
}
