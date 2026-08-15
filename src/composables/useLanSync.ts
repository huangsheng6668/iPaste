import { onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipasteApi } from "../lib/ipasteApi";
import { appErrorCode, appErrorParams, errorMessage } from "../lib/appError";
import { useIpasteStore } from "../stores/ipasteStore";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { LanCategoryReceivedEvent, LanCategorySentEvent, LanClipSource, LanSessionInfo, PortConflict } from "../types";

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
  // 整组发送进行中的分组 id（防重复点击）；完成后清空。
  const sendingCategory = ref<string | null>(null);
  // 整组发送完成汇总（发送端）。
  const lastCategorySent = ref<LanCategorySentEvent | null>(null);
  // 整组接收完成汇总（接收端）。
  const lastCategoryReceived = ref<LanCategoryReceivedEvent | null>(null);
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
      if (appErrorCode(e) === "port_in_use") {
        const params = (appErrorParams(e) ?? {}) as { name?: string; pid?: number };
        portConflict.value = { name: params.name ?? "未知进程", pid: params.pid ?? 0 };
      } else {
        error.value = errorMessage(e);
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
    } catch (e) { error.value = errorMessage(e); }
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
    } catch (e) { error.value = errorMessage(e); }
  }

  async function acceptPair(accept: boolean) {
    try { await ipasteApi.lanAcceptPair(accept); } catch (e) { error.value = errorMessage(e); }
  }

  async function sendCurrent() {
    const source: LanClipSource = { kind: "current" };
    try { await ipasteApi.lanSendClip(source); } catch (e) { error.value = errorMessage(e); }
  }

  async function sendItem(id: string) {
    const source: LanClipSource = { kind: "item", id };
    try { await ipasteApi.lanSendClip(source); } catch (e) { error.value = errorMessage(e); }
  }

  async function sendCategoryItem(id: string, categoryId: string) {
    const source: LanClipSource = { kind: "categoryItem", id, categoryId };
    try { await ipasteApi.lanSendClip(source); } catch (e) { error.value = errorMessage(e); }
  }

  /** 整组发送：把分组下全部条目一次性推给对端（后端 BatchStart→逐条→BatchEnd）。 */
  async function sendCategory(categoryId: string) {
    if (sendingCategory.value) return;
    error.value = null;
    sendingCategory.value = categoryId;
    try {
      lastCategorySent.value = await ipasteApi.lanSendCategory(categoryId);
      notice.value = "category-sent";
      setTimeout(() => {
        if (notice.value === "category-sent") notice.value = null;
        lastCategorySent.value = null;
      }, 5000);
    } catch (e) {
      error.value = errorMessage(e);
    } finally {
      sendingCategory.value = null;
    }
  }

  async function requestClip() {
    try { await ipasteApi.lanRequestClip(); } catch (e) { error.value = errorMessage(e); }
  }

  async function disconnect() {
    // 主动断开时清掉旧错误（如上一次 join 失败的提示），避免陈旧错误
    // 挂在面板上被误认为是本次断开引出的新问题。
    error.value = null;
    try { await ipasteApi.lanDisconnect(); } catch (e) { error.value = errorMessage(e); }
  }

  onMounted(async () => {
    await refresh();
    if (!isTauri) return;
    const handlers: Array<[string, (v: { payload: unknown }) => void]> = [
      [IPASTE_EVENTS.lanPairRequest, (e) => {
        pendingPeerName.value = (e.payload as { deviceName?: string })?.deviceName ?? "";
        notice.value = "pair-request";
      }],
      // 连接成功清掉旧错误（期间可能有「已有进行中的会话」等守门报错残留）。
      [IPASTE_EVENTS.lanSessionReady, () => { error.value = null; refresh(); }],
      [IPASTE_EVENTS.lanDisconnected, () => { notice.value = "disconnected"; refresh(); }],
      [IPASTE_EVENTS.lanClipReceived, () => {
        notice.value = "clip-received";
        // 3s 后自动清掉"已接收"提示，避免常驻。
        setTimeout(() => { if (notice.value === "clip-received") notice.value = null; }, 3000);
        // 无论分组还是历史条目都已落库，刷新面板内的列表（分组条目落到
        // category_items、历史条目落到 clips，都需重新加载才能显示）。
        void store.load();
      }],
      // 整组接收完成：一次汇总提示 + 一次刷新（批量中后端不逐条 emit）。
      [IPASTE_EVENTS.lanCategoryReceived, (e) => {
        lastCategoryReceived.value = e.payload as LanCategoryReceivedEvent;
        notice.value = "category-received";
        setTimeout(() => {
          if (notice.value === "category-received") notice.value = null;
          lastCategoryReceived.value = null;
        }, 5000);
        void store.load();
      }],
      // 接收对端条目落库/解析失败：把原因显示出来，避免「已接收但没入库」无从排查。
      [IPASTE_EVENTS.lanClipReceiveFailed, (e) => {
        const reason = String((e.payload as { reason?: string })?.reason ?? "receive failed");
        error.value = reason;
        notice.value = null;
      }],
      [IPASTE_EVENTS.lanJoinFailed, (v) => { error.value = String((v.payload as { reason?: string })?.reason ?? "join failed"); refresh(); }],
      [IPASTE_EVENTS.lanGuestRejected, (v) => {
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
    portConflict, rejectedGuest, sendingCategory, lastCategorySent, lastCategoryReceived,
    refresh, createSession, joinByAddress,
    acceptPair, sendCurrent, sendItem, sendCategoryItem, sendCategory, requestClip, disconnect,
    killPortProcess, quitApp, cancelPortConflict,
  };
}
