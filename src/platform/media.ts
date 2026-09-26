import { convertFileSrc } from "@tauri-apps/api/core";
import { hasTauriRuntime } from "./runtime";

// 本地资源 URL 的唯一出口：Tauri 下走 asset 协议转换；
// 浏览器 dev / 测试环境原样返回路径（调用方自行处理 data: 等内联源）。
export function fileSrc(path: string): string {
  return hasTauriRuntime() ? convertFileSrc(path) : path;
}
