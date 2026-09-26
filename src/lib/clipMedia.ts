import { isTauri } from "./env";
import { fileSrc } from "../platform/media";
import type { ClipViewItem } from "../types";

export function clipImageSrc(item: ClipViewItem) {
  if (item.clipType !== "image") return "";
  if (!isTauri || item.text.startsWith("data:")) return item.text;
  return fileSrc(item.text);
}
