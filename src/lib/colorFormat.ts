/** 颜色格式展示项（解析成功为 HEX/RGB/HSL/CSS，颜色样式但暂不可解析时回退 RAW/CSS）。 */
export interface ColorFormatItem {
  label: string;
  value: string;
}

const COLOR_LIKE_PATTERN = /^(?:#|rgba?\(|hsla?\()/i;

/**
 * 解析颜色字符串为 HEX/RGB/HSL 展示值；空串或非颜色样式文本返回 null。
 * 解析算法下沉自 ClipInspectorPane 的 colorFormats computed，行为保持一致：
 * 颜色样式但格式暂不支持（如 hsl()）时回退 RAW/CSS 两项。
 */
export function parseColorFormats(value: string): ColorFormatItem[] | null {
  const str = value.trim();
  if (!str || !COLOR_LIKE_PATTERN.test(str)) return null;

  const hexMatch = str.match(/^#?([0-9a-f]{3,8})$/i);
  let r = 0, g = 0, b = 0, a = 1;
  let parsed = false;

  if (hexMatch) {
    const hex = hexMatch[1];
    if (hex.length === 3) {
      r = parseInt(hex[0] + hex[0], 16);
      g = parseInt(hex[1] + hex[1], 16);
      b = parseInt(hex[2] + hex[2], 16);
      parsed = true;
    } else if (hex.length === 6) {
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      parsed = true;
    } else if (hex.length === 8) {
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      a = Math.round((parseInt(hex.slice(6, 8), 16) / 255) * 100) / 100;
      parsed = true;
    }
  }

  if (!parsed) {
    const rgbMatch = str.match(/rgba?\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)/i);
    if (rgbMatch) {
      r = Math.min(255, parseInt(rgbMatch[1], 10));
      g = Math.min(255, parseInt(rgbMatch[2], 10));
      b = Math.min(255, parseInt(rgbMatch[3], 10));
      if (rgbMatch[4] !== undefined) a = parseFloat(rgbMatch[4]);
      parsed = true;
    }
  }

  if (!parsed) {
    return [
      { label: "RAW", value: str },
      { label: "CSS", value: `color: ${str};` },
    ];
  }

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
