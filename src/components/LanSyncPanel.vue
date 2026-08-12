<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Wifi, WifiOff, ArrowUp, ArrowDown, Check, X, History } from "lucide-vue-next";
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
  requestClip,
  disconnect,
  scannedDevices,
  isScanning,
  scanDevices,
  joinScanned,
  portConflict,
  killPortProcess,
  quitApp,
  cancelPortConflict,
} = useLanSync();
const store = useIpasteStore();
const showHistory = ref(false);

const statusText = computed(() => {
  switch (info.status) {
    case "hosting": return t("lan.waitingJoin");
    case "waitingPair": return t("lan.waitingConfirm");
    case "connected": return t("lan.connected", { peer: info.peerDeviceName ?? "" });
    default: return t("lan.title");
  }
});

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
    <header class="lan-header" data-tauri-drag-region>
      <Wifi :size="18" />
      <span>{{ t("lan.title") }}</span>
      <button type="button" class="lan-close" :aria-label="t('topBar.closePanel')" @click="closeWindow">
        <X :size="16" />
      </button>
    </header>

    <p v-if="error" class="lan-error">{{ error }}</p>
    <p v-if="notice === 'clip-received'" class="lan-notice">{{ t("lan.received") }}</p>

    <!-- 初始：创建 / 加入 -->
    <div v-if="info.status === 'idle'" class="lan-section">
      <button class="lan-btn primary" @click="createSession">{{ t("lan.createSession") }}</button>
      <details>
        <summary>{{ t("lan.manual") }}</summary>
        <div class="lan-row">
          <input v-model="manualAddress" :placeholder="t('lan.address')" />
          <input v-model="manualCode" :placeholder="t('lan.code')" />
          <button class="lan-btn" @click="joinByAddress">{{ t("lan.joinSession") }}</button>
        </div>
      </details>
      <details>
        <summary>{{ t("lan.scanNearby") }}</summary>
        <button type="button" class="lan-btn" :disabled="isScanning" @click="scanDevices">
          {{ isScanning ? t("lan.scanning") : t("lan.startScan") }}
        </button>
        <ul v-if="scannedDevices.length" class="lan-history">
          <li
            v-for="d in scannedDevices"
            :key="d.addr"
            @click="joinScanned(d)"
          >
            {{ d.deviceName }}
          </li>
        </ul>
        <p v-else-if="!isScanning" class="lan-hint">{{ t("lan.scanEmpty") }}</p>
      </details>
      <p class="lan-hint">{{ t("lan.firewallHint") }}</p>
    </div>

    <!-- 接入确认（优先于 hosting：guest 发 pair-request 时 info.status 仍为 hosting） -->
    <div v-else-if="notice === 'pair-request'" class="lan-section">
      <p>{{ t("lan.pairRequest", { device: pendingPeerName }) }}</p>
      <div class="lan-row">
        <button class="lan-btn primary" @click="onAccept"><Check :size="14" /> {{ t("lan.accept") }}</button>
        <button class="lan-btn" @click="onReject"><X :size="14" /> {{ t("lan.reject") }}</button>
      </div>
    </div>

    <!-- Host 等待加入 -->
    <div v-else-if="info.status === 'hosting'" class="lan-section">
      <label>{{ t("lan.code") }}</label>
      <code class="lan-code">{{ info.code }}</code>
      <label>{{ t("lan.listenAddr") }}</label>
      <code class="lan-code">{{ info.listenAddr }}</code>
      <p>{{ statusText }}</p>
      <button class="lan-btn" @click="disconnect">{{ t("lan.disconnect") }}</button>
    </div>

    <!-- 已连接 -->
    <div v-else-if="info.status === 'connected'" class="lan-section">
      <p>{{ statusText }}</p>
      <div class="lan-row">
        <button class="lan-btn primary" @click="sendCurrent"><ArrowUp :size="14" /> {{ t("lan.pushMine") }}</button>
        <button class="lan-btn" @click="requestClip"><ArrowDown :size="14" /> {{ t("lan.pullTheirs") }}</button>
      </div>
      <button class="lan-btn" @click="showHistory = !showHistory"><History :size="14" /> {{ t("lan.fromHistory") }}</button>
      <ul v-if="showHistory" class="lan-history">
        <li v-for="clip in store.clips.slice(0, 20)" :key="clip.id" @click="sendItem(clip.id)">
          {{ clip.previewText || clip.displayName || clip.clipType }}
        </li>
      </ul>
      <button class="lan-btn danger" @click="disconnect"><WifiOff :size="14" /> {{ t("lan.disconnect") }}</button>
    </div>

    <!-- Guest 等待确认 -->
    <div v-else class="lan-section">
      <p>{{ statusText }}</p>
      <button class="lan-btn" @click="disconnect">{{ t("lan.disconnect") }}</button>
    </div>

    <!-- 端口占用弹窗：覆盖在面板上，三选一 -->
    <div v-if="portConflict" class="lan-conflict-overlay">
      <div class="lan-conflict-dialog">
        <p>{{ t("lan.portInUse", { port: "45130", name: portConflict.name, pid: String(portConflict.pid) }) }}</p>
        <div class="lan-row">
          <button type="button" class="lan-btn primary" @click="killPortProcess">
            {{ t("lan.killProcess") }}
          </button>
          <button type="button" class="lan-btn" @click="quitApp">
            {{ t("lan.quitApp") }}
          </button>
          <button type="button" class="lan-btn" @click="cancelPortConflict">
            {{ t("common.cancel") }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lan-sync-panel { padding: 16px; display: flex; flex-direction: column; gap: 12px; font-size: 14px; }
.lan-header { display: flex; align-items: center; gap: 8px; font-weight: 600; }
.lan-close { margin-left: auto; display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; border-radius: 6px; border: none; background: transparent; cursor: pointer; color: #6b7280; }
.lan-close:hover { background: #f3f4f6; color: #111827; }
.lan-section { display: flex; flex-direction: column; gap: 8px; }
.lan-row { display: flex; gap: 8px; flex-wrap: wrap; }
.lan-btn { display: inline-flex; align-items: center; gap: 6px; padding: 8px 12px; border-radius: 8px; border: 1px solid #d1d5db; background: #fff; cursor: pointer; }
.lan-btn.primary { background: #0D9488; color: #fff; border-color: #0D9488; }
.lan-btn.danger { color: #b91c1c; }
.lan-code { font-family: monospace; background: #f3f4f6; padding: 8px; border-radius: 6px; }
.lan-error { color: #b91c1c; }
.lan-notice { color: #0D9488; }
.lan-hint { color: #6b7280; font-size: 12px; }
.lan-history { max-height: 200px; overflow-y: auto; list-style: none; padding: 0; margin: 0; border: 1px solid #e5e7eb; border-radius: 6px; }
.lan-history li { padding: 8px; cursor: pointer; border-bottom: 1px solid #f3f4f6; }
.lan-history li:hover { background: #f9fafb; }

/* 端口占用弹窗：绝对定位覆盖整个 lan-sync-panel。根 div 需 position: relative。 */
.lan-sync-panel { position: relative; }
.lan-conflict-overlay {
  position: absolute;
  inset: 0;
  background: rgba(17, 24, 39, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  z-index: 20;
}
.lan-conflict-dialog {
  background: #fff;
  border-radius: 12px;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 320px;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.2);
}
.lan-conflict-dialog p { margin: 0; color: #111827; }
.lan-conflict-dialog .lan-row { justify-content: space-between; }
</style>
