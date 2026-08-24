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

  const refresh = async () => {
    if (!isTauri) return;
    devices.value = sortDevices(await ipasteApi.deviceList());
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
    void ipasteApi.pairingRespond(accept);
  };

  const disconnect = (nodeId: string) => ipasteApi.deviceDisconnect(nodeId).then(refresh);
  const revoke = (nodeId: string) => ipasteApi.deviceRevoke(nodeId).then(refresh);
  const remove = (nodeId: string) => ipasteApi.deviceDelete(nodeId).then(refresh);
  const setAutoSync = (nodeId: string, mode: AutoSyncMode) =>
    ipasteApi.deviceSetAutoSync(nodeId, mode).then(refresh);

  let unlistenFns: UnlistenFn[] = [];
  onMounted(async () => {
    if (!isTauri) return;
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
        pairRequest.value = event.payload;
      }),
    );
    unlistenFns.push(
      await listen<PairJoinFailed>(IPASTE_EVENTS.pairJoinFailed, (event) => {
        // 后端 reason 为原始字符串，面板原样展示（不做映射）。
        joinError.value = event.payload.reason;
      }),
    );
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
