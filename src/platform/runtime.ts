// 运行环境判定的唯一实现。所有浏览器全局访问都收在本模块并带守卫；
// 其余代码（含 lib/env.ts 的常量形态再导出）一律从这里取值。
// 测试通过 vi.mock 本模块单点控制平台分支，不再需要 stubGlobal window/navigator。

/** Tauri 桌面运行时（window 上挂有注入的 internals）；浏览器 dev 与测试环境为 false。 */
export function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** macOS 平台判定（快捷键修饰键展示等）。无 navigator 的环境（SSR/部分测试）返回 false。 */
export function isMacOsPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /mac/i.test(navigator.platform) || /Mac OS/i.test(navigator.userAgent);
}
