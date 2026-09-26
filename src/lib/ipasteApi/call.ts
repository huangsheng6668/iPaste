import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "../env";

/**
 * 命令调用统一封装：Tauri 下走 invoke；浏览器 dev 下返回 fallback（无 fallback 返回 undefined）。
 * 重载让“带 fallback”的调用诚实收敛为 Promise<T>（两条路径都必然返回 T），
 * “无 fallback”的调用保持 Promise<T | undefined>（浏览器 dev 下确实是 undefined）。
 */
export function call<T>(command: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T>;
export function call<T>(command: string, args?: Record<string, unknown>): Promise<T | undefined>;
export async function call<T>(command: string, args?: Record<string, unknown>, fallback?: T): Promise<T | undefined> {
  if (isTauri) return invoke<T>(command, args);
  if (fallback !== undefined) return structuredClone(fallback);
  return undefined;
}

/**
 * 严格调用（Task 41）：给本就没有 mock fallback 的写命令用——Tauri 下走 invoke，
 * 浏览器 dev 下同样拒绝（与裸 invoke 一致），但错误信息可读。
 * 全仓不再出现 ipasteApi 域模块直接 import invoke 的第二套约定。
 */
export function callStrict<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return invoke<T>(command, args);
  return Promise.reject(new Error(`命令 ${command} 仅在 Tauri 环境可用（浏览器开发模式不支持写命令）`));
}
