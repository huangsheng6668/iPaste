<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Wifi, WifiOff, ArrowUp, ArrowDown, Check, X, History, FolderUp } from "lucide-vue-next";
import { useLanSync } from "../composables/useLanSync";
import { useIpasteStore } from "../stores/ipasteStore";
import { t } from "../i18n";

// Destructure refs from `lan` so the template can use top-level ref names
// (auto-unwrapped) for v-model. `info` stays as the reactive object.
const {
  info,
  manualAddress,
  manualCode,
  error,
  notice,
  pendingPeerName,
  createSession,
  joinByAddress,
  acceptPair,
  sendCurrent,
  sendItem,
  sendCategoryItem,
  sendCategory,
  sendingCategory,
  lastCategorySent,
  lastCategoryReceived,
  requestClip,
  disconnect,
  portConflict,
  rejectedGuest,
  killPortProcess,
  quitApp,
  cancelPortConflict,
} = useLanSync();
const store = useIpasteStore();
// 条目选择器：先展开/收起；展开后用 sourceTab 在「历史 / 各分组」之间切换。
const showPicker = ref(false);
// "history" 或某个 category.id。默认历史，与主面板一致。
const sourceTab = ref<string>("history");

const pickerItems = computed(() => {
  if (sourceTab.value === "history") {
    // 历史：取最近 20 条，点击发送走 sendItem(clip.id)
    return store.clips.slice(0, 20).map((clip) => ({
      key: clip.id,
      label: clip.previewText || clip.displayName || clip.clipType,
      onClick: () => sendItem(clip.id),
    }));
  }
  // 分组：该分组下所有条目，点击发送走 sendCategoryItem(item.id, item.categoryId)
  return store.categoryItems
    .filter((item) => item.categoryId === sourceTab.value)
    .slice(0, 20)
    .map((item) => ({
      key: item.id,
      label: item.previewText || item.displayName || item.clipType,
      onClick: () => sendCategoryItem(item.id, item.categoryId),
    }));
});

// 当前分类 tab 下的全部条目数（整组发送按钮用，不受列表 20 条截断影响）。
const activeCategoryItemCount = computed(() => {
  if (sourceTab.value === "history") return 0;
  return store.categoryItems.filter((item) => item.categoryId === sourceTab.value).length;
});
const activeCategoryName = computed(() => {
  if (sourceTab.value === "history") return "";
  return store.categories.find((cat) => cat.id === sourceTab.value)?.name ?? "";
});
function sendWholeCategory() {
  if (sourceTab.value !== "history") {
    void sendCategory(sourceTab.value);
  }
}
function categorySentText(): string {
  const r = lastCategorySent.value;
  if (!r) return "";
  return r.failed > 0
    ? t("lan.categorySentFailed", { category: r.categoryName, sent: r.sent, failed: r.failed })
    : t("lan.categorySent", { category: r.categoryName, sent: r.sent });
}
function categoryReceivedText(): string {
  const r = lastCategoryReceived.value;
  if (!r) return "";
  return r.failed > 0
    ? t("lan.categoryReceivedFailed", { category: r.categoryName, count: r.count, failed: r.failed })
    : t("lan.categoryReceived", { category: r.categoryName, count: r.count });
}

const statusText = computed(() => {
  switch (info.status) {
    case "hosting": return t("lan.waitingJoin");
    case "waitingPair": return t("lan.waitingConfirm");
    case "connected": return t("lan.connected", { peer: info.peerDeviceName ?? "" });
    default: return t("lan.title");
  }
});

// 把 host 状态枚举（来自后端的 camelCase）翻成本地化标签，复用现有文案。
function hostStatusLabel(s: string): string {
  switch (s) {
    case "hosting": return t("lan.waitingJoin");
    case "waitingPair": return t("lan.waitingConfirm");
    case "connected": return t("lan.connected", { peer: "" });
    default: return s;
  }
}
function dismissRejected() {
  rejectedGuest.value = null;
}

onMounted(() => store.load());

async function closeWindow() {
  await getCurrentWindow().close();
}

function onAccept() {
  acceptPair(true);
  notice.value = null;
  pendingPeerName.value = "";
}
function onReject() {
  acceptPair(false);
  notice.value = null;
  pendingPeerName.value = "";
}
</script>

<template>
  <div class="lan-sync-panel">
    <header
      class="lan-header"
      data-tauri-drag-region
    >
      <Wifi :size="18" />
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

    <p
      v-if="error"
      class="lan-error"
    >
      {{ error }}
    </p>
    <p
      v-if="notice === 'clip-received'"
      class="lan-notice"
    >
      {{ t("lan.received") }}
    </p>
    <p
      v-if="notice === 'category-sent' && lastCategorySent"
      class="lan-notice"
    >
      {{ categorySentText() }}
    </p>
    <p
      v-if="notice === 'category-received' && lastCategoryReceived"
      class="lan-notice"
    >
      {{ categoryReceivedText() }}
    </p>
    <p
      v-if="rejectedGuest"
      class="lan-hint lan-rejected"
      @click="dismissRejected"
    >
      {{ t("lan.guestRejected", { device: rejectedGuest.deviceName, status: hostStatusLabel(rejectedGuest.hostStatus) }) }}
    </p>

    <!-- 初始：创建 / 加入 -->
    <div
      v-if="info.status === 'idle'"
      class="lan-section items-center text-center py-4"
    >
      <div class="lan-radar-container my-2">
        <div class="lan-radar-circle" />
        <div class="lan-radar-circle lan-radar-delay" />
        <div class="lan-radar-center">
          <Wifi class="size-6 text-[var(--accent)]" />
        </div>
      </div>

      <button
        class="lan-btn primary px-6 py-2.5 text-sm"
        @click="createSession"
      >
        {{ t("lan.createSession") }}
      </button>

      <details class="w-full text-left mt-2">
        <summary class="cursor-pointer text-xs text-[var(--text-3)] hover:text-[var(--text-1)]">
          {{ t("lan.manual") }}
        </summary>
        <div class="lan-row mt-2">
          <input
            v-model="manualAddress"
            class="dialog-input text-xs"
            :placeholder="t('lan.address')"
          >
          <input
            v-model="manualCode"
            class="dialog-input text-xs"
            :placeholder="t('lan.code')"
          >
          <button
            class="lan-btn w-full justify-center text-xs"
            @click="joinByAddress"
          >
            {{ t("lan.joinSession") }}
          </button>
        </div>
      </details>
      <p class="lan-hint mt-2">
        {{ t("lan.firewallHint") }}
      </p>
    </div>

    <!-- 接入确认（优先于 hosting：guest 发 pair-request 时 info.status 仍为 hosting） -->
    <div
      v-else-if="notice === 'pair-request'"
      class="lan-section text-center py-4"
    >
      <p class="font-semibold text-sm text-[var(--text-1)]">
        {{ t("lan.pairRequest", { device: pendingPeerName }) }}
      </p>
      <div class="lan-row justify-center mt-3">
        <button
          class="lan-btn primary"
          @click="onAccept"
        >
          <Check :size="14" /> {{ t("lan.accept") }}
        </button>
        <button
          class="lan-btn"
          @click="onReject"
        >
          <X :size="14" /> {{ t("lan.reject") }}
        </button>
      </div>
    </div>

    <!-- Host 等待加入 -->
    <div
      v-else-if="info.status === 'hosting'"
      class="lan-section items-center text-center py-2"
    >
      <div class="lan-radar-container my-1">
        <div class="lan-radar-circle" />
        <div class="lan-radar-circle lan-radar-delay" />
        <div class="lan-radar-center">
          <Wifi class="size-6 text-[var(--accent)]" />
        </div>
      </div>

      <label class="text-xs font-semibold text-[var(--text-3)] uppercase tracking-wider">{{ t("lan.code") }}</label>
      <div
        v-if="info.code"
        class="lan-pin-grid my-2"
      >
        <span
          v-for="(digit, idx) in info.code.split('')"
          :key="idx"
          class="lan-pin-cell"
        >{{ digit }}</span>
      </div>
      <code
        v-else
        class="lan-code"
      >{{ info.code }}</code>

      <label class="text-xs font-semibold text-[var(--text-3)] uppercase tracking-wider mt-1">{{ t("lan.listenAddr") }}</label>
      <code class="lan-code text-xs text-[var(--text-2)]">{{ info.listenAddr }}</code>
      
      <p class="text-xs text-[var(--text-2)] my-2">
        {{ statusText }}
      </p>
      <button
        class="lan-btn danger text-xs"
        @click="disconnect"
      >
        {{ t("lan.disconnect") }}
      </button>
    </div>

    <!-- 已连接 -->
    <div
      v-else-if="info.status === 'connected'"
      class="lan-section"
    >
      <div class="flex items-center gap-2 px-1 py-1">
        <span class="size-2.5 rounded-full bg-[var(--success)] shadow-sm animate-pulse" />
        <span class="font-semibold text-sm text-[var(--text-1)]">{{ statusText }}</span>
      </div>

      <div class="lan-row">
        <button
          class="lan-btn primary flex-1 justify-center"
          @click="sendCurrent"
        >
          <ArrowUp :size="14" /> {{ t("lan.pushMine") }}
        </button>
        <button
          class="lan-btn flex-1 justify-center"
          @click="requestClip"
        >
          <ArrowDown :size="14" /> {{ t("lan.pullTheirs") }}
        </button>
      </div>

      <button
        class="lan-btn justify-center"
        @click="showPicker = !showPicker"
      >
        <History :size="14" /> {{ t("lan.selectSource") }}
      </button>

      <div
        v-if="showPicker"
        class="lan-picker"
      >
        <div class="lan-tabs">
          <button
            class="lan-tab"
            :class="{ active: sourceTab === 'history' }"
            @click="sourceTab = 'history'"
          >
            {{ t("lan.history") }}
          </button>
          <button
            v-for="cat in store.categories"
            :key="cat.id"
            class="lan-tab"
            :class="{ active: sourceTab === cat.id }"
            @click="sourceTab = cat.id"
          >
            <span
              class="lan-tab-dot"
              :style="{ backgroundColor: cat.color }"
            />
            {{ cat.name }}
          </button>
        </div>
        <div
          v-if="sourceTab !== 'history' && activeCategoryItemCount > 0"
          class="lan-row lan-send-category-row"
        >
          <button
            type="button"
            class="lan-btn primary w-full justify-center"
            :disabled="sendingCategory === sourceTab"
            @click="sendWholeCategory"
          >
            <FolderUp :size="14" />
            {{ t("lan.sendCategory", { category: activeCategoryName, count: activeCategoryItemCount }) }}
          </button>
        </div>
        <ul class="lan-history">
          <li
            v-for="item in pickerItems"
            :key="item.key"
            @click="item.onClick"
          >
            {{ item.label }}
          </li>
          <li
            v-if="pickerItems.length === 0"
            class="lan-empty"
          >
            {{ t("lan.empty") }}
          </li>
        </ul>
      </div>

      <button
        class="lan-btn danger mt-2"
        @click="disconnect"
      >
        <WifiOff :size="14" /> {{ t("lan.disconnect") }}
      </button>
    </div>

    <!-- Guest 等待确认 -->
    <div
      v-else
      class="lan-section text-center py-4"
    >
      <p class="text-sm text-[var(--text-2)]">
        {{ statusText }}
      </p>
      <button
        class="lan-btn danger mt-3"
        @click="disconnect"
      >
        {{ t("lan.disconnect") }}
      </button>
    </div>

    <!-- 端口占用弹窗：覆盖在面板上，三选一 -->
    <div
      v-if="portConflict"
      class="lan-conflict-overlay"
    >
      <div class="lan-conflict-dialog">
        <p>{{ t("lan.portInUse", { port: "45130", name: portConflict.name, pid: String(portConflict.pid) }) }}</p>
        <div class="lan-row">
          <button
            type="button"
            class="lan-btn primary"
            @click="killPortProcess"
          >
            {{ t("lan.killProcess") }}
          </button>
          <button
            type="button"
            class="lan-btn"
            @click="quitApp"
          >
            {{ t("lan.quitApp") }}
          </button>
          <button
            type="button"
            class="lan-btn"
            @click="cancelPortConflict"
          >
            {{ t("common.cancel") }}
          </button>
        </div>
      </div>
    </div>
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
.lan-section { display: flex; flex-direction: column; gap: 8px; }
.lan-row { display: flex; gap: 8px; flex-wrap: wrap; }
.lan-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  border-radius: 9px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-2);
  font-weight: 600;
  cursor: pointer;
  box-shadow: var(--shadow-xs);
  transition: background-color 140ms ease, border-color 140ms ease, color 140ms ease, filter 140ms ease;
}
.lan-btn:hover { border-color: var(--border-accent); background: var(--accent-soft); color: var(--accent-strong); }
.lan-btn.primary {
  background: var(--accent-grad);
  color: var(--on-accent);
  border-color: transparent;
  box-shadow: 0 4px 14px rgba(255, 99, 99, 0.25);
}
.lan-btn.primary:hover { background: var(--accent-grad); color: var(--on-accent); filter: brightness(1.08); }
.lan-btn.danger { color: var(--danger); }
.lan-btn.danger:hover { background: var(--danger-soft); border-color: var(--danger-border); color: var(--danger-strong); }

/* ---- Radar Wave Visual ---- */
.lan-radar-container {
  position: relative;
  width: 72px;
  height: 72px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.lan-radar-circle {
  position: absolute;
  inset: 0;
  border-radius: 50%;
  border: 1.5px solid var(--accent);
  animation: radar-pulse 2.4s infinite cubic-bezier(0.16, 1, 0.3, 1);
  pointer-events: none;
}
.lan-radar-delay {
  animation-delay: 1.2s;
}
.lan-radar-center {
  position: relative;
  z-index: 2;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--surface);
  border: 1px solid var(--border-accent);
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 0 16px var(--focus-ring);
}

/* ---- 6-digit PIN Grid ---- */
.lan-pin-grid {
  display: flex;
  gap: 6px;
  justify-content: center;
}
.lan-pin-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 40px;
  border-radius: 8px;
  border: 1px solid var(--border-accent);
  background: var(--surface);
  font-family: var(--font-mono);
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--accent);
  box-shadow: var(--shadow-card);
}

.lan-code {
  font-family: var(--font-mono);
  background: var(--surface-code);
  padding: 6px 12px;
  border-radius: 8px;
  color: var(--text-1);
  border: 1px solid var(--border);
}
.lan-error { color: var(--danger); font-size: 13px; }
.lan-notice { color: var(--success); font-size: 13px; }
.lan-hint { color: var(--text-3); font-size: 12px; }
.lan-rejected { cursor: pointer; }
.lan-history {
  max-height: 180px;
  overflow-y: auto;
  list-style: none;
  padding: 0;
  margin: 0;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
}
.lan-history li { padding: 8px 10px; cursor: pointer; border-bottom: 1px solid var(--border); }
.lan-history li:last-child { border-bottom: 0; }
.lan-history li:hover { background: var(--accent-soft); }
.lan-history li.lan-empty { color: var(--text-3); cursor: default; }
.lan-history li.lan-empty:hover { background: transparent; }

/* 条目选择器：顶部一行 tab（历史 + 各分组），下方列表 */
.lan-picker { display: flex; flex-direction: column; gap: 8px; }
.lan-send-category-row { margin-top: 2px; }
.lan-btn:disabled { opacity: 0.55; cursor: default; }
.lan-tabs { display: flex; flex-wrap: wrap; gap: 6px; }
.lan-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text-2);
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  transition: background-color 140ms ease, border-color 140ms ease, color 140ms ease;
}
.lan-tab:hover { border-color: var(--border-accent); color: var(--accent-strong); }
.lan-tab.active {
  background: var(--accent-grad);
  color: var(--on-accent);
  border-color: transparent;
}
.lan-tab-dot { width: 8px; height: 8px; border-radius: 999px; display: inline-block; }

/* 端口占用弹窗：绝对定位覆盖整个 lan-sync-panel。 */
.lan-conflict-overlay {
  position: absolute;
  inset: 0;
  background: var(--backdrop);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  z-index: 20;
  backdrop-filter: var(--backdrop-blur);
}
.lan-conflict-dialog {
  background: var(--surface);
  border: 1px solid var(--border-soft);
  border-radius: 14px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 320px;
  box-shadow: var(--shadow-dialog);
}
.lan-conflict-dialog p { margin: 0; color: var(--text-1); }
.lan-conflict-dialog .lan-row { justify-content: space-between; }
</style>
