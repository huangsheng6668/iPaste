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

/** 粘贴票据清洗：去首尾空白与意外换行/内部空白。 */
export function cleanTicketInput(raw: string): string {
  return raw.trim().replace(/\s+/g, "");
}
