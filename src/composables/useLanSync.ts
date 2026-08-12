import { onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipasteApi } from "../lib/ipasteApi";
import { useIpasteStore } from "../stores/ipasteStore";
import type { LanClipReceivedEvent, LanClipSource, LanSessionInfo, PortConflict } from "../types";

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
  // pair-request 时携带的对方设备名；与 info.peerDeviceName（仅 Connected 后由
  // set_connected 写入）分离，避免确认弹窗读到空名。
  const pendingPeerName = ref("");
  // 端口占用：lan_create_session 因 45130 被占用失败时，由后端错误字符串解析得到。
  // 非 null 时面板覆盖显示「杀进程 / 退出应用 / 取消」三按钮弹窗。
  const portConflict = ref<PortConflict | null>(null);
  // host 因非 Hosting 态拒绝 guest（如本机已在会话中）——既给 host 端反馈，
  // 也用于定位"加入被报码错"的真因。带设备名 + 当时的 host 状态。
  const rejectedGuest = ref<{ deviceName: string; hostStatus: string } | null>(null);
  let unlistenFns: UnlistenFn[] = [];
  // 收到分组条目时需要刷新本地分类数据（分组/条目落库后才能在 UI 显示）。
  const store = useIpasteStore();

  function applyInfo(next: LanSessionInfo) {
    Object.assign(info, next);
  }

  async function refresh() {
    if (!isTauri) return;
    applyInfo(await ipasteApi.lanGetState());
  }

  async function createSession() {
    error.value = null;
    portConflict.value = null;
    try {
      applyInfo(await ipasteApi.lanCreateSession(code.value.trim() || null));
    } catch (e) {
      // 后端错误格式「端口 45130 被 <name>（PID <pid>）占用。{原始 bind 错误}」。
      // 正则不带 `$` 锚定——错误尾巴还有 bind 原因。name 用非贪婪避免吞掉 PID。
      const message = String(e);
      if (message.includes("端口") && message.includes("占用")) {
        const m = message.match(/端口 (\d+) 被 (.+?)（PID (\d+)）占用/);
        if (m) {
          portConflict.value = { name: m[2], pid: Number(m[3]) };
        } else {
          portConflict.value = { name: "未知进程", pid: 0 };
        }
      } else {
        error.value = message;
      }
    }
  }

  async function killPortProcess() {
    if (!portConflict.value || portConflict.value.pid === 0) return;
    error.value = null;
    try {
      await ipasteApi.lanKillPortProcess(portConflict.value.pid);
      portConflict.value = null;
      // 杀掉占用进程后自动重试创建会话，免去用户再点一次「Create Session」。
      await createSession();
    } catch (e) { error.value = String(e); }
  }

  async function quitApp() {
    await ipasteApi.lanQuitApp();
  }

  function cancelPortConflict() {
    portConflict.value = null;
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

  async function sendCategoryItem(id: string, categoryId: string) {
    const source: LanClipSource = { kind: "categoryItem", id, categoryId };
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
      ["ipaste://lan-pair-request", (e) => {
        pendingPeerName.value = (e.payload as { deviceName?: string })?.deviceName ?? "";
        notice.value = "pair-request";
      }],
      ["ipaste://lan-session-ready", () => refresh()],
      ["ipaste://lan-disconnected", () => { notice.value = "disconnected"; refresh(); }],
      ["ipaste://lan-clip-received", (e) => {
        notice.value = "clip-received";
        // 3s 后自动清掉"已接收"提示，避免常驻。
        setTimeout(() => { if (notice.value === "clip-received") notice.value = null; }, 3000);
        // 分组条目落到 category_items 表，需刷新才能在分组标签下显示。
        const p = e.payload as LanClipReceivedEvent | undefined;
        if (p?.categoryName) void store.load();
      }],
      ["ipaste://lan-join-failed", (v) => { error.value = String((v.payload as { reason?: string })?.reason ?? "join failed"); refresh(); }],
      ["ipaste://lan-guest-rejected", (v) => {
        const p = v.payload as { guestDeviceName?: string; hostStatus?: string } | undefined;
        rejectedGuest.value = {
          deviceName: p?.guestDeviceName ?? "",
          hostStatus: p?.hostStatus ?? "",
        };
        // 8s 后自动清掉，避免常驻遮挡面板。
        setTimeout(() => { rejectedGuest.value = null; }, 8000);
      }],
    ];
    for (const [event, handler] of handlers) {
      const un = await listen(event, handler);
      unlistenFns.push(un);
    }
  });

  onUnmounted(() => {
    unlistenFns.forEach((fn) => fn());
    unlistenFns = [];
    // 关窗 → 自动断开清理（spec §8）。fire-and-forget，不阻塞卸载。
    if (isTauri) {
      void ipasteApi.lanDisconnect().catch(() => {});
    }
  });

  return {
    isTauri, info, manualAddress, manualCode, error, notice, pendingPeerName,
    portConflict, rejectedGuest,
    refresh, createSession, joinByAddress,
    acceptPair, sendCurrent, sendItem, sendCategoryItem, requestClip, disconnect,
    killPortProcess, quitApp, cancelPortConflict,
  };
}
