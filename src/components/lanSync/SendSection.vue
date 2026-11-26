<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Check, Search, Send } from "lucide-vue-next";
import { t, type I18nKey } from "../../i18n";
import { errorMessage } from "../../lib/appError";
import { ipasteApi } from "../../lib/ipasteApi";
import { clipTypeIcon } from "../../lib/clipTypeIcons";
import {
  fingerprintOf,
  sendTargets as buildSendTargets,
  SEND_TARGET_ALL,
} from "../../lib/deviceDisplay";
import { categoryDisplayName, formatTime, typeLabel } from "../../lib/format";
import type { Category } from "../../types/generated/Category";
import type { ClipItem } from "../../types/generated/ClipItem";
import type { ClipType } from "../../types";
import type { DeviceInfo } from "../../types/generated/DeviceInfo";

/**
 * 发送剪贴板（设备同步窗口底部）：内容来源三选一（当前剪贴板 / 历史条目多选 /
 * 分类整组）+ 目标设备 + 就地发送反馈。
 *
 * 反馈必须留在本窗口内：设备同步是独立 Tauri 窗口，主面板的 Pinia/toast
 * 不跨窗口共享，`useUiStore().pushToast` 在这里没有任何宿主可渲染。
 */

const props = defineProps<{
  devices: DeviceInfo[];
}>();

type SendMode = "current" | "history" | "category";

const MODE_OPTIONS: ReadonlyArray<{ id: SendMode; labelKey: I18nKey }> = [
  { id: "current", labelKey: "deviceSync.sendContent.current" },
  { id: "history", labelKey: "deviceSync.sendContent.history" },
  { id: "category", labelKey: "deviceSync.sendContent.category" },
];

/** 历史条目单页条数：够选常用片段，又不至于把窗口撑成第二个历史面板。 */
const CLIP_PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 250;
const SUCCESS_STATUS_MS = 6000;

const mode = ref<SendMode>("current");
const targetId = ref<string>(SEND_TARGET_ALL);

const categories = ref<Category[]>([]);
const categoryId = ref<string>("");

const clips = ref<ClipItem[]>([]);
const clipsLoading = ref(false);
const clipsError = ref<string | null>(null);
const query = ref("");
const selected = ref<ClipItem[]>([]);

const sending = ref(false);
const status = ref<{ tone: "info" | "success" | "error"; text: string } | null>(null);
let statusTimer: ReturnType<typeof setTimeout> | null = null;

const onlineTargets = computed(() => buildSendTargets(props.devices));
// 与 DeviceListSection 一致的口径：除「全部在线」外的真实在线设备数。
const hasOnlineDevice = computed(() => onlineTargets.value.length > 1);

const targetLabel = computed(() => {
  if (targetId.value === SEND_TARGET_ALL) return t("deviceSync.sendTo.all");
  const target = onlineTargets.value.find((entry) => entry.id === targetId.value);
  return target?.name ?? fingerprintOf(targetId.value);
});

const canSend = computed(() => {
  if (sending.value || !hasOnlineDevice.value) return false;
  if (mode.value === "history") return selected.value.length > 0;
  if (mode.value === "category") return Boolean(categoryId.value);
  return true;
});

// 设备离线/撤销后目标可能已不在列表：回落到「全部在线」，避免对幽灵目标发送。
watch(
  () => props.devices,
  () => {
    if (!onlineTargets.value.some((target) => target.id === targetId.value)) {
      targetId.value = SEND_TARGET_ALL;
    }
  },
);

// —— 历史条目加载（输入防抖 + 请求序号防乱序回包）——

let loadSeq = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;

async function loadClips() {
  const seq = ++loadSeq;
  clipsLoading.value = true;
  clipsError.value = null;
  try {
    const page = await ipasteApi.listClips(0, CLIP_PAGE_SIZE, query.value.trim());
    if (seq !== loadSeq) return;
    clips.value = page.clips;
  } catch (unknownError) {
    if (seq !== loadSeq) return;
    clips.value = [];
    clipsError.value = errorMessage(unknownError);
  } finally {
    if (seq === loadSeq) clipsLoading.value = false;
  }
}

watch(query, () => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => {
    void loadClips();
  }, SEARCH_DEBOUNCE_MS);
});

// 首次进入「历史条目」时补一次最新列表（窗口可能已开很久，历史早变了）。
watch(mode, (next) => {
  if (next === "history") void loadClips();
});

onMounted(() => {
  void loadClips();
  void (async () => {
    try {
      categories.value = await ipasteApi.listCategories();
    } catch (unknownError) {
      // 分类读取失败只降级为「不能整组发送」，不阻断当前剪贴板与历史条目。
      console.warn("[ipaste] list_categories failed:", unknownError);
    }
  })();
});

onUnmounted(() => {
  if (searchTimer) clearTimeout(searchTimer);
  if (statusTimer) clearTimeout(statusTimer);
});

// —— 选择 ——

function isSelected(clipId: string) {
  return selected.value.some((clip) => clip.id === clipId);
}

function toggleClip(clip: ClipItem) {
  selected.value = isSelected(clip.id)
    ? selected.value.filter((item) => item.id !== clip.id)
    : [...selected.value, clip];
}

function selectAllVisible() {
  const picked = new Set(selected.value.map((clip) => clip.id));
  const additions = clips.value.filter((clip) => !picked.has(clip.id));
  if (additions.length === 0) return;
  selected.value = [...selected.value, ...additions];
}

function clearSelection() {
  selected.value = [];
}

function clipLabel(clip: ClipItem) {
  return clip.displayName?.trim() || clip.previewText?.trim() || typeLabel(clip.clipType as ClipType);
}

// —— 发送与就地反馈 ——

function setStatus(tone: "info" | "success" | "error", text: string) {
  if (statusTimer) {
    clearTimeout(statusTimer);
    statusTimer = null;
  }
  status.value = { tone, text };
  if (tone === "success") {
    statusTimer = setTimeout(() => {
      status.value = null;
      statusTimer = null;
    }, SUCCESS_STATUS_MS);
  }
}

async function onSend() {
  if (!canSend.value) return;
  sending.value = true;
  setStatus("info", t("deviceSync.send.sending"));
  const target = targetId.value === SEND_TARGET_ALL ? null : targetId.value;
  try {
    if (mode.value === "current") {
      await ipasteApi.deviceSendClip(target, { kind: "current" });
      setStatus("success", t("deviceSync.send.currentSentTo", { target: targetLabel.value }));
      return;
    }
    if (mode.value === "category") {
      const result = await ipasteApi.deviceSendCategory(target, categoryId.value);
      if (result.failed > 0) {
        setStatus("error", t("category.sendPartial", { sent: result.sent, failed: result.failed }));
        return;
      }
      setStatus("success", t("category.sent", { name: result.categoryName, sent: result.sent }));
      return;
    }
    const result = await ipasteApi.deviceSendItems(target, selected.value.map((clip) => clip.id));
    if (result.failed > 0) {
      setStatus("error", t("deviceSync.send.partial", { sent: result.sent, failed: result.failed }));
      return;
    }
    setStatus(
      "success",
      t("deviceSync.send.sentTo", { count: result.sent, target: targetLabel.value }),
    );
  } catch (unknownError) {
    setStatus("error", errorMessage(unknownError));
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <section class="lan-section">
    <h2 class="lan-section-title">
      {{ t("deviceSync.send.title") }}
    </h2>

    <!-- 内容来源：三态标签页（radio 语义，键盘可 Tab + Enter） -->
    <div
      class="lan-send-modes"
      role="tablist"
      :aria-label="t('deviceSync.sendContent.label')"
    >
      <button
        v-for="option in MODE_OPTIONS"
        :key="option.id"
        type="button"
        role="tab"
        class="lan-send-mode"
        :class="{ 'lan-send-mode-active': mode === option.id }"
        :aria-selected="mode === option.id"
        @click="mode = option.id"
      >
        {{ t(option.labelKey) }}
      </button>
    </div>

    <!-- 当前剪贴板 -->
    <p
      v-if="mode === 'current'"
      class="lan-hint"
    >
      {{ t("deviceSync.send.currentHint") }}
    </p>

    <!-- 历史条目多选 -->
    <div
      v-else-if="mode === 'history'"
      class="lan-picker"
    >
      <div class="lan-picker-toolbar">
        <span class="lan-picker-search">
          <Search class="size-3.5" />
          <input
            v-model="query"
            type="search"
            class="lan-picker-input"
            :placeholder="t('deviceSync.send.searchPlaceholder')"
            :aria-label="t('deviceSync.send.searchPlaceholder')"
          >
        </span>
        <button
          type="button"
          class="lan-action"
          :disabled="clips.length === 0"
          @click="selectAllVisible"
        >
          {{ t("deviceSync.send.selectAll") }}
        </button>
        <button
          type="button"
          class="lan-action"
          :disabled="selected.length === 0"
          @click="clearSelection"
        >
          {{ t("deviceSync.send.clear") }}
        </button>
      </div>

      <p
        v-if="clipsLoading"
        class="lan-hint"
      >
        {{ t("deviceSync.send.loading") }}
      </p>
      <p
        v-else-if="clipsError"
        class="lan-error"
      >
        {{ clipsError }}
      </p>
      <p
        v-else-if="clips.length === 0"
        class="lan-hint"
      >
        {{ query.trim() ? t("deviceSync.send.emptySearch") : t("deviceSync.send.emptyHistory") }}
      </p>
      <ul
        v-else
        class="lan-picker-list"
        role="listbox"
        aria-multiselectable="true"
      >
        <li
          v-for="clip in clips"
          :key="clip.id"
        >
          <button
            type="button"
            class="lan-picker-row"
            :class="{ 'lan-picker-row-selected': isSelected(clip.id) }"
            role="option"
            :aria-selected="isSelected(clip.id)"
            @click="toggleClip(clip)"
          >
            <span
              class="lan-picker-check"
              aria-hidden="true"
            >
              <Check
                v-if="isSelected(clip.id)"
                class="size-3"
              />
            </span>
            <component
              :is="clipTypeIcon(clip.clipType as ClipType, clip.text)"
              class="size-3.5 lan-picker-icon"
            />
            <span class="lan-picker-text">{{ clipLabel(clip) }}</span>
            <span class="lan-picker-time">{{ formatTime(clip.lastCapturedAt) }}</span>
          </button>
        </li>
      </ul>

      <p class="lan-hint">
        {{ t("deviceSync.send.selectedCount", { count: selected.length }) }}
      </p>
    </div>

    <!-- 分类整组 -->
    <template v-else>
      <p
        v-if="categories.length === 0"
        class="lan-hint"
      >
        {{ t("deviceSync.send.categoryEmpty") }}
      </p>
      <select
        v-else
        v-model="categoryId"
        class="lan-select"
        :aria-label="t('deviceSync.sendContent.category')"
      >
        <option
          value=""
          disabled
        >
          {{ t("deviceSync.sendContent.category") }}
        </option>
        <option
          v-for="category in categories"
          :key="category.id"
          :value="category.id"
        >
          {{ categoryDisplayName(category.name) }}
        </option>
      </select>
    </template>

    <!-- 目标设备 -->
    <div class="lan-send-target">
      <label
        class="lan-label"
        for="lan-send-target"
      >{{ t("deviceSync.sendTarget.label") }}</label>
      <select
        id="lan-send-target"
        v-model="targetId"
        class="lan-select"
        :disabled="!hasOnlineDevice"
      >
        <option
          v-for="target in onlineTargets"
          :key="target.id"
          :value="target.id"
        >
          {{ target.isAll ? t("deviceSync.sendTo.all") : (target.name ?? fingerprintOf(target.id)) }}
        </option>
      </select>
    </div>

    <div class="lan-send-footer">
      <button
        type="button"
        class="lan-button lan-button-primary"
        :disabled="!canSend"
        @click="onSend"
      >
        <Send class="size-3.5" />
        {{ sending ? t("deviceSync.send.sending") : t("deviceSync.send.button") }}
      </button>
      <p
        v-if="status"
        class="lan-send-status"
        :class="`lan-send-status-${status.tone}`"
        :role="status.tone === 'error' ? 'alert' : 'status'"
        aria-live="polite"
      >
        {{ status.text }}
      </p>
    </div>
  </section>
</template>
