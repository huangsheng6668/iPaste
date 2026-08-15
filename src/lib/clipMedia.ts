import { convertFileSrc } from "@tauri-apps/api/core";
import { isTauri } from "./env";
import type { ClipViewItem } from "../types";

export function clipImageSrc(item: ClipViewItem) {
  if (item.clipType !== "image") return "";
  if (!isTauri || item.text.startsWith("data:")) return item.text;
  return convertFileSrc(item.text);
}
