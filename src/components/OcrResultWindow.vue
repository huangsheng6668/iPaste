<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, Copy, ExternalLink, LoaderCircle, ScanText, X } from "lucide-vue-next";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import type { ClipViewItem, OcrResultPayload } from "../types";

const token = new URLSearchParams(window.location.search).get("token") ?? "";
const status = ref<"loading" | "ready" | "empty" | "error" | "expired">("loading");
const payload = ref<OcrResultPayload | null>(null);
const text = ref("");
const copied = ref(false);
const textareaRef = ref<HTMLTextAreaElement | null>(null);
let copiedTimer: number | null = null;

const charCount = computed(() => text.value.length);
const canCopy = computed(() => status.value === "ready" && text.value.trim().length > 0);

onMounted(async () => {
  document.addEventListener("keydown", handleKeydown, true);
  if (!token) {
    status.value = "expired";
    return;
  }
  try {
    payload.value = await ipasteApi.getOcrResultPayload(token);
  } catch {
    status.value = "expired";
    return;
  }
  if (!payload.value) {
    // 浏览器 dev 模式 mock 返回 null
    status.value = "expired";
    return;
  }
  try {
    const result = await ipasteApi.recognizeImageText(payload.value.imagePath);
    text.value = result.text;
    status.value = result.text.trim() ? "ready" : "empty";
    await nextTick();
    textareaRef.value?.focus();
    textareaRef.value?.select();
  } catch {
    status.value = "error";
  }
});

onUnmounted(() => {
  document.removeEventListener("keydown", handleKeydown, true);
  if (copiedTimer !== null) window.clearTimeout(copiedTimer);
});

function handleKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
    return;
  }
  event.preventDefault();
  void closeWindow();
}

async function closeWindow() {
  try {
    await getCurrentWindow().close();
  } catch {
    // 浏览器 dev 模式下无 Tauri 窗口 API，忽略
  }
}

async function copyText() {
  if (!canCopy.value) return;
  await ipasteApi.copyClip("text", text.value);
  copied.value = true;
  if (copiedTimer !== null) window.clearTimeout(copiedTimer);
  copiedTimer = window.setTimeout(() => {
    copied.value = false;
  }, 1500);
}

async function openImage() {
  if (!payload.value) return;
  const item: ClipViewItem = {
    id: payload.value.itemId,
    collection: "history",
    clipType: "image",
    contentHash: "",
    displayName: null,
    previewText: "",
    text: payload.value.imagePath,
    sourceApp: null,
    lastCapturedAt: new Date().toISOString(),
    favoriteCount: 0,
    isPinned: false,
  };
  await ipasteApi.openClipViewer(item, payload.value.itemId);
}
</script>

<template>
  <div class="ocr-result">
    <header class="ocr-result-header">
      <ScanText class="size-4 text-[var(--accent)]" />
      <h1 class="ocr-result-title">
        {{ t("ocrScreenshot.title") }}
      </h1>
      <button
        type="button"
        class="ocr-result-close"
        :aria-label="t('topBar.closePanel')"
        @click="closeWindow"
      >
        <X class="size-4" />
      </button>
    </header>

    <div class="ocr-result-body">
      <div
        v-if="status === 'loading'"
        class="ocr-result-state"
      >
        <LoaderCircle class="size-6 update-spin text-[var(--text-3)]" />
        <p>{{ t("ocrScreenshot.recognizing") }}</p>
      </div>
      <div
        v-else-if="status === 'empty'"
        class="ocr-result-state"
      >
        <ScanText class="size-6 text-[var(--text-3)]" />
        <p>{{ t("ocrScreenshot.emptyResult") }}</p>
      </div>
      <div
        v-else-if="status === 'error'"
        class="ocr-result-state ocr-result-state-error"
      >
        <p>{{ t("ocrScreenshot.recognizeFailed") }}</p>
      </div>
      <div
        v-else-if="status === 'expired'"
        class="ocr-result-state ocr-result-state-error"
      >
        <p>{{ t("ocrScreenshot.payloadExpired") }}</p>
      </div>
      <textarea
        v-else
        ref="textareaRef"
        v-model="text"
        class="ocr-result-textarea"
        spellcheck="false"
      />
    </div>

    <footer class="ocr-result-footer">
      <span class="ocr-result-count">{{ t("ocrScreenshot.charCount", { count: charCount }) }}</span>
      <div class="flex items-center gap-2">
        <button
          v-if="payload"
          type="button"
          class="ocr-result-button"
          @click="openImage"
        >
          <ExternalLink class="size-4" />
          <span>{{ t("ocrScreenshot.openImage") }}</span>
        </button>
        <button
          type="button"
          class="ocr-result-button ocr-result-button-primary"
          :disabled="!canCopy"
          @click="copyText"
        >
          <Check
            v-if="copied"
            class="size-4"
          />
          <Copy
            v-else
            class="size-4"
          />
          <span>{{ copied ? t("ocrScreenshot.copied") : t("ocrScreenshot.copyText") }}</span>
        </button>
      </div>
    </footer>
  </div>
</template>

<style scoped>
/* 独立窗口不依赖设置页样式：全部自包含，令牌取 src/styles/theme.css */
.ocr-result {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--surface);
  color: var(--text-1);
  font-size: 0.875rem;
}

.ocr-result-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  user-select: none;
}

.ocr-result-title {
  font-size: 0.875rem;
  font-weight: 600;
}

.ocr-result-close {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: color 150ms ease, background 150ms ease;
}

.ocr-result-close:hover {
  color: var(--text-1);
  background: var(--surface-inset);
}

.ocr-result-body {
  flex: 1;
  min-height: 0;
  padding: 12px 14px;
}

.ocr-result-state {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-3);
}

.ocr-result-state-error {
  color: var(--accent);
}

.ocr-result-textarea {
  width: 100%;
  height: 100%;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface-inset);
  color: var(--text-1);
  font: inherit;
  line-height: 1.6;
  resize: none;
}

.ocr-result-textarea:focus {
  outline: 2px solid var(--accent);
  outline-offset: -1px;
}

.ocr-result-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
  border-top: 1px solid var(--border);
}

.ocr-result-count {
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}

.ocr-result-button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-1);
  font-size: 0.8125rem;
  cursor: pointer;
  transition: background 150ms ease, border-color 150ms ease;
}

.ocr-result-button:hover:not(:disabled) {
  background: var(--surface-inset);
}

.ocr-result-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ocr-result-button-primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #fff;
}

.ocr-result-button-primary:hover:not(:disabled) {
  background: var(--accent);
  opacity: 0.9;
}
</style>
