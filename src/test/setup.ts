// Vitest 全局 setup（由 vite.config.ts 的 test.setupFiles 挂载，jsdom 环境下每个测试文件运行前执行）。
// 只做环境级兜底，不 mock 业务模块；需要 Tauri 路径的用例自行 vi.mock 对应封装。

// jsdom 未实现 matchMedia，而 lib/theme.ts 在模块加载时探测系统深浅色。
if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}
