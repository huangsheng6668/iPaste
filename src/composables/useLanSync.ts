import { onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipasteApi } from "../lib/ipasteApi";
import type { LanClipSource, LanSessionInfo } from "../types";

const isTauri = "__TAURI_INTERNALS__" in window;

export function useLanSync() {
  const info = reactive<LanSessionInfo>({
    role: null, status: "idle", code: null, listenAddr: null, peerDeviceName: null,
  });
  const code = ref("");
  const manualAddress = ref("");
  const manualCode = ref("");
  const error = ref<string | null>(null);
  const notice = ref<string | null>(null);
  let unlistenFns: UnlistenFn[] = [];

  function applyInfo(next: LanSessionInfo) {
    Object.assign(info, next);
  }

  async function refresh() {
    if (!isTauri) return;
    applyInfo(await ipasteApi.lanGetState());
  }

  async function createSession() {
    error.value = null;
    try {
      applyInfo(await ipasteApi.lanCreateSession(code.value.trim() || null));
    } catch (e) { error.value = String(e); }
  }

  async function joinSession() {
    error.value = null;
    try {
      await ipasteApi.lanJoinSession(code.value.trim());
    } catch (e) { error.value = String(e); }
  }

  async function joinByAddress() {
    error.value = null;
    try {
      await ipasteApi.lanJoinByAddress(manualAddress.value.trim(), manualCode.value.trim());
    } catch (e) { error.value = String(e); }
  }

  async function acceptPair(accept: boolean) {
    try { await ipasteApi.lanAcceptPair(accept); } catch (e) { error.value = String(e); }
  }

  async function sendCurrent() {
    const source: LanClipSource = { kind: "current" };
    try { await ipasteApi.lanSendClip(source); } catch (e) { error.value = String(e); }
  }

  async function sendItem(id: string) {
    const source: LanClipSource = { kind: "item", id };
    try { await ipasteApi.lanSendClip(source); } catch (e) { error.value = String(e); }
  }

  async function requestClip() {
    try { await ipasteApi.lanRequestClip(); } catch (e) { error.value = String(e); }
  }

  async function disconnect() {
    try { await ipasteApi.lanDisconnect(); } catch (e) { error.value = String(e); }
  }

  onMounted(async () => {
    await refresh();
    if (!isTauri) return;
    const handlers: Array<[string, (v: { payload: unknown }) => void]> = [
      ["ipaste://lan-pair-request", () => notice.value = "pair-request"],
      ["ipaste://lan-session-ready", () => refresh()],
      ["ipaste://lan-disconnected", () => { notice.value = "disconnected"; refresh(); }],
      ["ipaste://lan-clip-received", () => { notice.value = "clip-received"; }],
      ["ipaste://lan-join-failed", (v) => { error.value = String((v.payload as { reason?: string })?.reason ?? "join failed"); refresh(); }],
    ];
    for (const [event, handler] of handlers) {
      const un = await listen(event, handler);
      unlistenFns.push(un);
    }
  });

  onUnmounted(() => {
    unlistenFns.forEach((fn) => fn());
    unlistenFns = [];
  });

  return {
    isTauri, info, code, manualAddress, manualCode, error, notice,
    refresh, createSession, joinSession, joinByAddress,
    acceptPair, sendCurrent, sendItem, requestClip, disconnect,
  };
}
