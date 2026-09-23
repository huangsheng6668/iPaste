import type { AutoPushSettings } from "../../types/generated/AutoPushSettings";
import type { AutoSyncMode } from "../../types/generated/AutoSyncMode";
import type { ClipSource } from "../../types/generated/ClipSource";
import type { DeviceInfo } from "../../types/generated/DeviceInfo";
import type { PairRequested } from "../../types/generated/PairRequested";
import { call } from "./index";

/** device_send_clip 的 source 参数（Rust ClipSource 的 serde 形状，camelCase）。 */
export type LanClipSource = ClipSource;

/** 跨设备同步域（lan_sync v5 iroh）：配对、设备管理、条目推送与传输/自动推送偏好。 */
export const deviceSyncApi = {
  openLanSync() {
    return call<void>("open_lan_sync");
  },
  deviceList() {
    return call<DeviceInfo[]>("device_list", undefined, []);
  },
  deviceRevoke(nodeId: string) {
    return call<void>("device_revoke", { nodeId });
  },
  deviceDelete(nodeId: string) {
    return call<void>("device_delete", { nodeId });
  },
  deviceDisconnect(nodeId: string) {
    return call<void>("device_disconnect", { nodeId });
  },
  deviceSetAutoSync(nodeId: string, mode: AutoSyncMode) {
    return call<void>("device_set_auto_sync", { nodeId, mode });
  },
  pairingCreateInvite() {
    return call<string>("pairing_create_invite", undefined, "ipaste-pair-v1:pending");
  },
  pairingCancelInvite() {
    return call<void>("pairing_cancel_invite");
  },
  pairingJoin(ticket: string) {
    return call<void>("pairing_join", { ticket });
  },
  pairingRespond(accept: boolean) {
    return call<void>("pairing_respond", { accept });
  },
  pairingPending() {
    return call<PairRequested | null>("pairing_pending", undefined, null);
  },
  deviceSendClip(target: string | null, source: LanClipSource) {
    return call<void>("device_send_clip", { target, source });
  },
  deviceSendCategory(target: string | null, categoryId: string) {
    return call<{ categoryName: string; sent: number; failed: number }>(
      "device_send_category",
      { target, categoryId },
      { categoryName: "", sent: 0, failed: 0 },
    );
  },
  deviceRequestClip(nodeId: string) {
    return call<void>("device_request_clip", { nodeId });
  },
  syncTransportSettingsGet() {
    return call<{ relayUrl: string | null }>("sync_transport_settings_get", undefined, { relayUrl: null });
  },
  syncTransportSettingsSet(relayUrl: string | null) {
    return call<{ relayUrl: string | null; hint: string }>("sync_transport_settings_set", { relayUrl }, {
      relayUrl,
      hint: "",
    });
  },
  syncAutoPushSettingsGet() {
    return call<AutoPushSettings>("sync_auto_push_settings_get", undefined, { master: true, notify: false });
  },
  syncAutoPushSettingsSet(master: boolean, notify: boolean) {
    return call<AutoPushSettings>("sync_auto_push_settings_set", { master, notify }, { master, notify });
  },
};
