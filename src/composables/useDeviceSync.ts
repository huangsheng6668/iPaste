import { onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ipasteApi } from "../lib/ipasteApi";
import { isTauri } from "../lib/env";
import { errorMessage } from "../lib/appError";
import { cleanTicketInput, sortDevices } from "../lib/deviceDisplay";
import { IPASTE_EVENTS } from "../types/generated/events";
import type { AutoSyncMode } from "../types/generated/AutoSyncMode";
import type { DeviceInfo } from "../types/generated/DeviceInfo";
import type { PairInviteState } from "../types/generated/PairInviteState";
import type { PairJoinFailed } from "../types/generated/PairJoinFailed";
import type { PairRequested } from "../types/generated/PairRequested";

/** join() 拒绝格式非法票据时写入 joinError 的哨兵值（面板映射为 i18n 文案）。 */
export const INVALID_TICKET = "notValidTicket";

/**
 * 跨设备同步面板状态（v5 iroh）：设备列表 + 邀请/加入配对流 + 配对确认。
 * 仅在 lan-sync 窗口的组件 setup 内使用；非 Tauri 环境所有动作静默为空。
 */
export function useDeviceSync() {
  const devices = ref<DeviceInfo[]>([]);
  const inviteTicket = ref<string | null>(null);
  const inviteExpiresAt = ref<number | null>(null);
  const joinError = ref<string | null>(null);
  const pairRequest = ref<PairRequested | null>(null);
  // 配对应答命令失败（如 120s 自动拒绝后已无待确认请求）的透出通道（B2）。
  const pairError = ref<string | null>(null);
  // 初始/事件刷新的设备列表加载失败横幅（B4）：降级提示，不阻断任何订阅。
  const loadError = ref<string | null>(null);

  const refresh = async () => {
    if (!isTauri) return;
    try {
      devices.value = sortDevices(await ipasteApi.deviceList());
      loadError.value = null;
    } catch (unknownError) {
      // 加载失败就地降级为横幅（不再向上抛出）：onMounted 的监听订阅、
      // 后续事件的恢复性刷新都不受阻断；成功后横幅自动清除。
      loadError.value = errorMessage(unknownError);
    }
  };

  const createInvite = async () => {
    joinError.value = null;
    inviteTicket.value = await ipasteApi.pairingCreateInvite();
    // 后端随后经 pairInviteState 事件推送权威 expiresAt；这里先给 10 分钟兜底。
    inviteExpiresAt.value = Date.now() + 10 * 60 * 1000;
  };

  const cancelInvite = async () => {
    await ipasteApi.pairingCancelInvite();
    inviteTicket.value = null;
    inviteExpiresAt.value = null;
  };

  const join = async (raw: string) => {
    joinError.value = null;
    const ticket = cleanTicketInput(raw);
    if (!ticket.startsWith("ipaste-pair-v1:")) {
      joinError.value = INVALID_TICKET;
      return;
    }
    try {
      await ipasteApi.pairingJoin(ticket);
    } catch (unknownError) {
      // 票据格式错误由命令直达报错；连接类失败走 pairJoinFailed 事件。
      joinError.value = errorMessage(unknownError);
    }
  };

  const respondPair = (accept: boolean) => {
    pairRequest.value = null;
    pairError.value = null;
    void ipasteApi.pairingRespond(accept).catch((unknownError: unknown) => {
      // 应答命令失败（如 120s 自动拒绝后请求已不存在）：错误串就地上浮到
      // 面板配对区展示，而不是被 void 静默吞掉。
      pairError.value = errorMessage(unknownError);
    });
  };

  const disconnect = (nodeId: string) => ipasteApi.deviceDisconnect(nodeId).then(refresh);
  const revoke = (nodeId: string) => ipasteApi.deviceRevoke(nodeId).then(refresh);
  const remove = (nodeId: string) => ipasteApi.deviceDelete(nodeId).then(refresh);
  const setAutoSync = (nodeId: string, mode: AutoSyncMode) =>
    ipasteApi.deviceSetAutoSync(nodeId, mode).then(refresh);

  let unlistenFns: UnlistenFn[] = [];
  onMounted(async () => {
    if (!isTauri) return;
    // 初始列表失败已降级为 loadError 横幅（refresh 内部捕获），此处不再让
    // 异常中断 onMounted——五种事件监听必须无条件注册（B4）。
    await refresh();
    // 逐个类型化订阅；refresh 之外的回调直接消费生成的 payload 类型。
    unlistenFns.push(await listen(IPASTE_EVENTS.deviceListChanged, () => void refresh()));
    unlistenFns.push(await listen(IPASTE_EVENTS.deviceStatusChanged, () => void refresh()));
    unlistenFns.push(
      await listen<PairInviteState>(IPASTE_EVENTS.pairInviteState, (event) => {
        // 票据被后端作废/过期时（ticket = null）同步清空本地邀请态。
        inviteTicket.value = event.payload.ticket;
        inviteExpiresAt.value = event.payload.expiresAt;
      }),
    );
    unlistenFns.push(
      await listen<PairRequested>(IPASTE_EVENTS.pairRequest, (event) => {
        // 新请求到达时清除上一轮应答的残留错误（B2）。
        pairError.value = null;
        pairRequest.value = event.payload;
      }),
    );
    unlistenFns.push(
      await listen<PairJoinFailed>(IPASTE_EVENTS.pairJoinFailed, (event) => {
        // 后端 reason 为原始字符串，面板原样展示（不做映射）。
        joinError.value = event.payload.reason;
      }),
    );
    // 面板重开时恢复后端尚未超时作废的待确认配对请求（B1b）：lan-sync 窗口
    // 是瞬态窗，请求到达时窗口多半已重建；120s 时限仍由后端权威掌控。
    try {
      const pending = await ipasteApi.pairingPending();
      if (pending) pairRequest.value = pending;
    } catch (unknownError) {
      // 查询失败仅跳过恢复；后续新请求仍经事件正常到达。
      console.warn("[ipaste] pairing_pending query failed:", unknownError);
    }
  });
  onUnmounted(() => {
    unlistenFns.forEach((fn) => fn());
    unlistenFns = [];
  });

  return {
    devices,
    inviteTicket,
    inviteExpiresAt,
    joinError,
    pairRequest,
    pairError,
    loadError,
    refresh,
    createInvite,
    cancelInvite,
    join,
    respondPair,
    disconnect,
    revoke,
    remove,
    setAutoSync,
  };
}
