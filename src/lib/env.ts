// 运行环境判定入口：实现收口在 platform/runtime（守卫浏览器全局、可单点 mock）。
// 这里保留常量形态以兼容存量调用点（约 22 个文件），不做常量→函数的调用点改造。
import { hasTauriRuntime, isMacOsPlatform } from "../platform/runtime";

/** Tauri 桌面（true）vs 浏览器开发模式（false）。 */
export const isTauri = hasTauriRuntime();

/** 平台判定（macOS 快捷键修饰键展示等）。 */
export const isMacOs = isMacOsPlatform();
