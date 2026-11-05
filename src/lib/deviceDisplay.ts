import type { DeviceInfo } from "../types/generated/DeviceInfo";
import type { DeviceOnline } from "../types/generated/DeviceOnline";

/** UI 状态 key（serde snake_case 与生成的 DeviceOnline 联合同名）。 */
export type DeviceStatusKey = "offline" | "connecting" | "connected";

export function statusKey(online: DeviceOnline): DeviceStatusKey {
  return online as DeviceStatusKey;
}

/** 列表排序：在线在前（connected → connecting → offline），同组按 addedAt 升序（配对先后），已撤销垫底。 */
export function sortDevices(devices: DeviceInfo[]): DeviceInfo[] {
  const weight = (entry: DeviceInfo): number => {
    if (entry.device.revokedAt) return 3;
    if (entry.online === "connected") return 0;
    if (entry.online === "connecting") return 1;
    return 2;
  };
  return [...devices].sort(
    (a, b) => weight(a) - weight(b) || a.device.addedAt.localeCompare(b.device.addedAt),
  );
}

/** 面板显示名旁的指纹短码（nodeId 为 64 hex，取前 8 位）。 */
export function fingerprintOf(nodeId: string): string {
  return nodeId.slice(0, 8);
}

export const TICKET_PREFIX = "ipaste-pair-v1:";

const PUNCTUATION_MAP: Record<string, string> = {
  "：": ":",
  "－": "-",
  "—": "-",
  "–": "-",
  "＿": "_",
  "．": ".",
};

/** 粘贴票据清洗：零宽字符 → 全角标点归一化 → 去首尾与内部空白。 */
export function cleanTicketInput(raw: string): string {
  return raw
    .replace(/[\u200B-\u200D\u2060\uFEFF]/g, "")
    .replace(/[：－—–＿．]/g, (char) => PUNCTUATION_MAP[char])
    .trim()
    .replace(/\s+/g, "");
}

/** 票据正文：URL_SAFE_NO_PAD base64url，最短 51B 二进制 → 68 字符。 */
const TICKET_BODY = /^[A-Za-z0-9_-]{64,}$/;

export type TicketInputError = "missingPrefix" | "badEncoding";

export const TICKET_ERROR_SENTINELS: Record<TicketInputError, string> = {
  missingPrefix: "missingTicketPrefix",
  badEncoding: "badTicketEncoding",
};

export const PIN_ERROR_SENTINELS = {
  selectFirst: "selectNearbyFirst",
  invalid: "invalidPin",
} as const;

export type JoinErrorKey =
  | "deviceSync.join.missingPrefix"
  | "deviceSync.join.badEncoding"
  | "deviceSync.pin.selectFirst"
  | "deviceSync.pin.invalid";

/** 把前端哨兵映射成 i18n key；后端原始 reason 返回 null，由面板原样展示。 */
export function joinErrorKey(sentinel: string | null): JoinErrorKey | null {
  switch (sentinel) {
    case TICKET_ERROR_SENTINELS.missingPrefix:
      return "deviceSync.join.missingPrefix";
    case TICKET_ERROR_SENTINELS.badEncoding:
      return "deviceSync.join.badEncoding";
    case PIN_ERROR_SENTINELS.selectFirst:
      return "deviceSync.pin.selectFirst";
    case PIN_ERROR_SENTINELS.invalid:
      return "deviceSync.pin.invalid";
    default:
      return null;
  }
}

export function validateTicketInput(raw: string): TicketInputError | null {
  const ticket = cleanTicketInput(raw);
  if (!ticket.startsWith(TICKET_PREFIX)) return "missingPrefix";
  if (!TICKET_BODY.test(ticket.slice(TICKET_PREFIX.length))) return "badEncoding";
  return null;
}

export function isPinInput(value: string): boolean {
  return /^\d{6}$/.test(value);
}

/** 右键「发送到」子菜单的广播目标哨兵 id（deviceSendClip 的 target = null）。 */
export const SEND_TARGET_ALL = "__all__";

/** 「发送到」子菜单条目：`isAll` 表广播全部在线；设备条目携带显示名。 */
export type SendTarget = { id: string; name: string | null; isAll: boolean };

/** 右键「发送到」目标列表：恒有「全部在线」入口 + 各在线未撤销设备；离线/撤销不出现。 */
export function sendTargets(devices: DeviceInfo[]): SendTarget[] {
  const online = devices.filter((entry) => entry.online === "connected" && !entry.device.revokedAt);
  return [
    { id: SEND_TARGET_ALL, name: null, isAll: true },
    ...online.map((entry) => ({
      id: entry.device.nodeId,
      name: entry.device.deviceName,
      isAll: false,
    })),
  ];
}
