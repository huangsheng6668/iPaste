<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-vue-next";
import { t } from "../i18n";

// v5 迁移占位：v4 LAN 面板与配套 composable 已随 TCP 传输栈移除，
// Task 10 以设备管理 UI 重写本组件。当前仅保留窗口外壳（拖拽 + 关闭）。
async function closeWindow() {
  await getCurrentWindow().close();
}
</script>

<template>
  <div class="lan-sync-panel">
    <header
      class="lan-header"
      data-tauri-drag-region
    >
      <span>{{ t("lan.title") }}</span>
      <button
        type="button"
        class="lan-close"
        :aria-label="t('topBar.closePanel')"
        @click="closeWindow"
      >
        <X :size="16" />
      </button>
    </header>

    <p class="lan-placeholder">同步功能升级中</p>
  </div>
</template>

<style scoped>
.lan-sync-panel {
  position: relative;
  width: 100vw;
  height: 100vh;
  overflow: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 14px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--window-radius);
  clip-path: inset(0 round var(--window-radius));
  background: var(--bg-app);
  color: var(--text-1);
}
html.dark .lan-sync-panel { border-color: var(--border-hairline); }
.lan-header { display: flex; align-items: center; gap: 8px; font-weight: 600; }
.lan-close {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 8px;
  border: none;
  background: transparent;
  cursor: pointer;
  color: var(--text-2);
  transition: background-color 140ms ease, color 140ms ease;
}
.lan-close:hover { background: var(--surface-hover); color: var(--text-1); }
.lan-placeholder {
  margin: auto;
  color: var(--text-2);
}
</style>
