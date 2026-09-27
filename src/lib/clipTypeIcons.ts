import { Code, FileText, Image, Link, Palette, Type } from "lucide-vue-next";
import { isCodeText } from "./format";
import type { ClipType } from "../types";

/** 剪贴板类型 → lucide 图标的唯一映射（列表卡片与检查器共用，避免两处各配一份漂移）。 */
export type ClipTypeIcon = typeof Type;

export function clipTypeIcon(clipType: ClipType, text: string): ClipTypeIcon {
  if (clipType === "link") return Link;
  if (clipType === "color") return Palette;
  if (clipType === "image") return Image;
  if (clipType === "file") return FileText;
  if (isCodeText(clipType, text)) return Code;
  return Type;
}
