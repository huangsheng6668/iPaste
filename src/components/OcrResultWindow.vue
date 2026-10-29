<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { windowHandle } from "../platform/window";
import { Check, Copy, ExternalLink, LoaderCircle, ScanText, X } from "lucide-vue-next";
import { t } from "../i18n";
import { ipasteApi } from "../lib/ipasteApi";
import { errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";
import { useSettingsStore } from "../stores/settingsStore";
import OcrEngineSelect from "./ocr/OcrEngineSelect.vue";
import { useWindowDrag } from "../composables/useWindowDrag";
import {
  OCR_LANGUAGE_OPTIONS,
  loadOcrLanguage,
  normalizeOcrLanguage,
  saveOcrLanguage,
  type OcrLanguageId,
} from "../lib/ocrLanguages";
import type { ClipViewItem, OcrResultPayload } from "../types";

const token = new URLSearchParams(window.location.search).get("token") ?? "";
const status = ref<"loading" | "ready" | "empty" | "error" | "expired">("loading");
const payload = ref<OcrResultPayload | null>(null);
const selectedProfile = ref<"default" | "manga">("default");
const selectedOcrLanguage = ref<OcrLanguageId>(loadOcrLanguage());
const text = ref("");
/** 失败原因原文（服务商返回的 message / HTTP 状态）：只显示通用文案会把用户引错方向。 */
const errorDetail = ref("");
const copied = ref(false);
const textareaRef = ref<HTMLTextAreaElement | null>(null);
let copiedTimer: number | null = null;

// 独立窗口不走 App.vue 的 store.load（见 App.vue 早退分支），这里自行加载，
// 供顶栏引擎切换读取当前引擎与云 OCR 配置状态
const store = useIpasteStore(); // 窗口快照自举
const settings = useSettingsStore();

const charCount = computed(() => text.value.length);
const canCopy = computed(() => status.value === "ready" && text.value.trim().length > 0);

async function runRecognition(profile: "default" | "manga") {
  if (!payload.value?.imagePath) return;
  status.value = "loading";
  errorDetail.value = "";
  try {
    const result = await ipasteApi.recognizeImageText(
      payload.value.imagePath,
      profile,
      selectedOcrLanguage.value,
    );
    text.value = result.text;
    status.value = result.text.trim() ? "ready" : "empty";
    // 识别成功后默认把结果复制到剪贴板（copyText 会给出“已复制”反馈）；
    // 复制失败不回退成“识别失败”
    if (status.value === "ready") {
      await copyText().catch(() => undefined);
    }
    await nextTick();
    textareaRef.value?.focus();
    textareaRef.value?.select();
  } catch (unknownError) {
    // 此前把错误整个丢掉、只显示通用文案：额度用尽/限流/凭据过期都会被
    // 说成「请在设置中检查 OCR 资源」，用户只能去查一个没问题的配置。
    errorDetail.value = errorMessage(unknownError);
    status.value = "error";
  }
}

function selectProfile(profile: "default" | "manga") {
  if (selectedProfile.value === profile || status.value === "loading") return;
  selectedProfile.value = profile;
  void runRecognition(profile);
}

function selectLanguage(event: Event) {
  if (status.value === "loading") return;
  const next = normalizeOcrLanguage((event.target as HTMLSelectElement).value);
  if (!next || next === selectedOcrLanguage.value) return;
  selectedOcrLanguage.value = next;
  saveOcrLanguage(next);
  void runRecognition(selectedProfile.value);
}

onMounted(async () => {
  document.addEventListener("keydown", handleKeydown, true);
  void store.load().catch(() => undefined);
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
  await runRecognition(selectedProfile.value);
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
    await windowHandle().close();
  } catch {
    // 浏览器 dev 模式下无 Tauri 窗口 API，忽略
  }
}

// 无边框窗口无系统标题栏：标题区（图标/标题/空白）左键拖动移动窗口，
// 交互控件（识别档位/语言/关闭）阻止冒泡以免触发原生拖动吞掉点击
const { startWindowDrag } = useWindowDrag();

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
    <header
      class="ocr-result-header"
      @mousedown="startWindowDrag"
    >
      <div class="ocr-result-brand">
        <span class="ocr-result-icon-badge">
          <ScanText class="size-3.5" />
        </span>
        <h1 class="ocr-result-title">
          {{ t("ocrScreenshot.title") }}
        </h1>
      </div>

      <div
        class="ocr-result-profiles"
        @mousedown.stop
      >
        <button
          type="button"
          class="ocr-profile-pill"
          :class="{ active: selectedProfile === 'default' }"
          :disabled="status === 'loading'"
          @click="selectProfile('default')"
        >
          {{ t("ocrScreenshot.profileGeneral") }}
        </button>
        <button
          type="button"
          class="ocr-profile-pill"
          :class="{ active: selectedProfile === 'manga' }"
          :disabled="status === 'loading'"
          @click="selectProfile('manga')"
        >
          {{ t("ocrScreenshot.profileManga") }}
        </button>
      </div>

      <button
        type="button"
        class="ocr-result-close"
        :aria-label="t('topBar.closePanel')"
        @mousedown.stop
        @click="closeWindow"
      >
        <X class="size-4" />
      </button>
    </header>

    <div
      class="ocr-result-toolbar"
      @mousedown.stop
    >
      <OcrEngineSelect
        class="ocr-result-engine-select"
        :engine="settings.ocrEngine"
        :disabled="status === 'loading'"
        @rerun="runRecognition(selectedProfile)"
      />
      <select
        class="ocr-language-select"
        :value="selectedOcrLanguage"
        :disabled="status === 'loading'"
        :aria-label="t('ocr.languageLabel')"
        :title="t('ocr.languageLabel')"
        @mousedown.stop
        @change="selectLanguage"
      >
        <option
          v-for="option in OCR_LANGUAGE_OPTIONS"
          :key="option.id"
          :value="option.id"
        >
          {{ t(option.labelKey) }}
        </option>
      </select>
    </div>

    <div class="ocr-result-body">
      <div
        v-if="status === 'loading'"
        class="ocr-result-state"
      >
        <span class="ocr-result-state-badge ocr-result-state-badge-accent">
          <LoaderCircle class="size-5 update-spin" />
        </span>
        <p>{{ t("ocrScreenshot.recognizing") }}</p>
      </div>
      <div
        v-else-if="status === 'empty'"
        class="ocr-result-state"
      >
        <span class="ocr-result-state-badge">
          <ScanText class="size-5" />
        </span>
        <p>{{ t("ocrScreenshot.emptyResult") }}</p>
      </div>
      <div
        v-else-if="status === 'error'"
        class="ocr-result-state ocr-result-state-error"
      >
        <p>{{ t("ocrScreenshot.recognizeFailed") }}</p>
        <p
          v-if="errorDetail"
          class="ocr-result-state-detail"
        >
          {{ errorDetail }}
        </p>
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
        class="ocr-result-textarea subtle-scrollbar"
        spellcheck="false"
      />
    </div>

    <footer class="ocr-result-footer">
      <span class="ocr-result-count">{{ t("ocrScreenshot.charCount", { count: charCount }) }}</span>
      <div class="ocr-result-actions">
        <button
          v-if="payload"
          type="button"
          class="ocr-result-button"
          @click="openImage"
        >
          <ExternalLink class="size-3.5" />
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
            class="size-3.5"
          />
          <Copy
            v-else
            class="size-3.5"
          />
          <span>{{ copied ? t("ocrScreenshot.copied") : t("ocrScreenshot.copyText") }}</span>
        </button>
      </div>
    </footer>
  </div>
</template>

<style scoped>
/* 独立窗口不依赖设置页样式：全部自包含，令牌取 src/styles/theme.css（Hairline Ledger） */
.ocr-result {
  display: flex;
  flex-direction: column;
  width: 100vw;
  height: 100vh;
  border: 1px solid var(--border-hairline);
  border-radius: var(--window-radius);
  clip-path: inset(0 round var(--window-radius));
  background: var(--bg-app);
  color: var(--text-1);
  font-size: var(--text-md);
  overflow: hidden;
}

.ocr-result-header {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 6px 12px;
  background: var(--surface);
  border-bottom: 1px solid var(--border-hairline);
  user-select: none;
}

.ocr-result-brand {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  flex-shrink: 0;
}

.ocr-result-icon-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--r-sm);
  background: var(--accent-soft);
  color: var(--accent);
  flex-shrink: 0;
}

.ocr-result-title {
  margin: 0;
  font-size: var(--text-md);
  font-weight: var(--fw-semibold);
  letter-spacing: -0.01em;
  white-space: nowrap;
}

.ocr-result-profiles {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
  background: var(--surface-inset);
  border: 1px solid var(--border-hairline);
  padding: 2px;
  border-radius: var(--r-md);
}

.ocr-profile-pill {
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: var(--text-xs);
  font-weight: var(--fw-medium);
  padding: 0 9px;
  border-radius: var(--r-sm);
  white-space: nowrap;
  flex-shrink: 0;
  cursor: pointer;
  transition:
    color var(--dur-fast) var(--ease-out),
    background-color var(--dur-fast) var(--ease-out);
  user-select: none;
}

.ocr-profile-pill:hover:not(:disabled) {
  color: var(--text-1);
}

.ocr-profile-pill.active {
  background: var(--surface);
  color: var(--accent-strong);
  box-shadow: var(--shadow-xs);
}

.ocr-profile-pill:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.ocr-result-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border: none;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition:
    color var(--dur-fast) var(--ease-out),
    background-color var(--dur-fast) var(--ease-out);
}

.ocr-result-close:hover {
  color: var(--text-1);
  background: var(--surface-hover);
}

.ocr-result-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--border-hairline);
  user-select: none;
}

.ocr-result-engine-select {
  flex: 1 1 0;
  min-width: 0;
}

.ocr-language-select {
  flex: 0 0 auto;
  height: var(--control-sm);
  padding: 0 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  background: var(--surface);
  color: var(--text-1);
  font-size: var(--text-sm);
  font-weight: var(--fw-medium);
  white-space: nowrap;
  cursor: pointer;
  transition:
    border-color var(--dur-fast) var(--ease-out),
    background-color var(--dur-fast) var(--ease-out);
}

.ocr-language-select:hover:not(:disabled) {
  border-color: var(--border-accent);
  background: var(--surface-hover);
}

.ocr-language-select:focus-visible {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.ocr-language-select:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ocr-result-body {
  flex: 1;
  min-height: 0;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
}

.ocr-result-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 20px 16px;
  border: 1px solid var(--border-hairline);
  border-radius: var(--r-lg);
  background: var(--surface);
  color: var(--text-2);
  font-size: var(--text-md);
}

.ocr-result-state p {
  margin: 0;
}

.ocr-result-state-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: var(--r-lg);
  background: var(--surface-inset);
  color: var(--text-3);
}

.ocr-result-state-badge-accent {
  background: var(--accent-soft);
  color: var(--accent);
}

.ocr-result-state-error {
  border-color: var(--danger-border);
  background: var(--danger-soft);
  color: var(--danger-strong);
}

.ocr-result-state-detail {
  max-width: 92%;
  color: var(--text-2);
  font-size: var(--text-sm);
  line-height: 1.6;
  text-align: center;
  overflow-wrap: anywhere;
}

.ocr-result-textarea {
  width: 100%;
  flex: 1;
  min-height: 0;
  padding: 10px 12px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-lg);
  background: var(--surface);
  color: var(--text-1);
  font-family: var(--font-sans);
  font-size: var(--text-md);
  line-height: 1.65;
  resize: none;
  transition: border-color var(--dur-fast) var(--ease-out);
}

.ocr-result-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--focus-ring);
}

.ocr-result-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 44px;
  padding: 8px 12px;
  background: var(--surface);
  border-top: 1px solid var(--border-hairline);
}

.ocr-result-count {
  color: var(--text-3);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.ocr-result-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.ocr-result-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: var(--control-md);
  padding: 0 12px;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-md);
  background: var(--surface);
  color: var(--text-1);
  font-size: var(--text-sm);
  font-weight: var(--fw-medium);
  white-space: nowrap;
  flex-shrink: 0;
  cursor: pointer;
  transition:
    background-color var(--dur-fast) var(--ease-out),
    border-color var(--dur-fast) var(--ease-out);
}

.ocr-result-button:hover:not(:disabled) {
  background: var(--surface-hover);
  border-color: var(--border-accent);
}

.ocr-result-button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.ocr-result-button-primary {
  border-color: transparent;
  background: var(--accent-fill);
  color: var(--on-accent-fill);
}

.ocr-result-button-primary:hover:not(:disabled) {
  background: var(--accent-fill);
  border-color: transparent;
  filter: brightness(1.06);
}
</style>
