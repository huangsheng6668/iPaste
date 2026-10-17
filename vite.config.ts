/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],

  // Vitest：与构建共用同一 Vite 管道（含 vue 插件，组件测试可直接编译 SFC）。
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },

  // 构建产物拆分：框架层单独成 chunk，不随业务代码改动而失效（本地应用
  // 收益主要在窗口启动时的解析粒度；辅助窗口组件已在 App.vue 侧按
  // defineAsyncComponent 拆成按需 chunk）。
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ["vue", "pinia", "@tauri-apps/api"],
        },
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
