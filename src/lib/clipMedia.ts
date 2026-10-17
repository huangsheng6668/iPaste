import { isTauri } from "./env";
import { fileSrc } from "../platform/media";
import type { ClipViewItem } from "../types";

export function clipImageSrc(item: ClipViewItem) {
  if (item.clipType !== "image") return "";
  if (!isTauri || item.text.startsWith("data:")) return item.text;
  return fileSrc(item.text);
}

/**
 * 列表/检查器用的缩略图源：Rust 侧在原图同目录的 thumbs/ 下生成同名文件
 *（≤480px）。4K 截图按原图渲染时，浏览器解码与 GPU 纹理都是几十 MB 量级，
 * 密集图片历史下面板滚动/唤出的卡顿主要来自这里。
 *
 * 缩略图可能尚未生成（升级后的存量、生成失败）：调用方用 @error 回退
 * clipImageSrc（原图），见 ClipCard / ClipInspectorPane。
 */
export function clipThumbnailSrc(item: ClipViewItem) {
  if (item.clipType !== "image") return "";
  if (!isTauri || item.text.startsWith("data:")) return item.text;
  // 原图 …/{hash}.png → 缩略图 …/thumbs/{hash}.png（仅替换末段路径，
  // 与 Rust save_image_bytes 的落盘约定保持一致）。
  const separatorIndex = Math.max(item.text.lastIndexOf("/"), item.text.lastIndexOf("\\"));
  if (separatorIndex === -1) return item.text;
  return fileSrc(item.text.slice(0, separatorIndex + 1) + "thumbs/" + item.text.slice(separatorIndex + 1));
}
