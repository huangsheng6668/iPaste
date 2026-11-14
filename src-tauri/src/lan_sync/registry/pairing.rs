//! 配对域（原 registry.rs「邀请（host 侧）/加入（guest 侧）」段整体迁入，纯移动）：
//! host 侧邀请（create_invite/cancel_invite）、guest 侧加入（join）与用户确认
//! 状态（PendingPair/respond_pair/pending_pair_info）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::endpoint::VarInt;
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use tokio::sync::oneshot;

use crate::events::{
    PairInviteState, PairJoinFailed, PairRequested, EVENT_PAIR_INVITE_STATE, EVENT_PAIR_JOIN_FAILED,
};
use crate::lan_sync::device_name;
use crate::lan_sync::frame::{FrameReader, FrameWriter};
use crate::lan_sync::protocol::{LanMessage, PairRejectReason, IPASTE_ALPN, LAN_PROTOCOL_VERSION};
use crate::lan_sync::session::fingerprint_hex;
use crate::lan_sync::ticket::{format_pin, is_pin, PairTicket, INVITE_TTL};
use crate::util::LockRecover;

use super::{
    endpoint_id_from_hex, hex_encode_32, send_stream_opener, DeviceLinkRegistry, CONNECT_TIMEOUT,
};

/// create_invite 等待中继连接（endpoint.online()）的上限；超时非致命，
/// LAN-only 票据对局域网配对依然有效。
const INVITE_ONLINE_WAIT: Duration = Duration::from_secs(5);

/// join 拨号后等待 PairAccept/PairReject 的限时：对端静默丢弃（如邀请无效）
/// 时拨号方不能无限挂起。测试构建缩短到 2s，超时路径可低成本回归。
#[cfg(not(test))]
const JOIN_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const JOIN_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// PIN 路径等待应答限时 8s：host 对无效邀请静默（防探测），不能干等票据的 30s。
#[cfg(not(test))]
const NEARBY_REPLY_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(test)]
const NEARBY_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// PIN 路径的快速失败文案：host 静默（错 PIN / 邀请过期 / 旧版本 host）时统一使用。
const NEARBY_SILENT_REASON: &str =
    "对方未响应，请检查 PIN 或网络；若对方使用旧版 iPaste，请升级后重试";

/// 直连地址 + 中继装配（票据 / 附近设备两条路径共享，避免各自解析）。
fn parse_transport_addrs(direct_addrs: &[String], relay: Option<&str>) -> Vec<TransportAddr> {
    let mut addrs: Vec<TransportAddr> = direct_addrs
        .iter()
        .filter_map(|addr| {
            addr.parse::<std::net::SocketAddr>()
                .ok()
                .map(TransportAddr::Ip)
        })
        .collect();
    if let Some(url) = relay.and_then(|url| url.parse::<RelayUrl>().ok()) {
        addrs.push(TransportAddr::Relay(url));
    }
    addrs
}

/// 待用户确认的配对请求：decision_tx 由 respond_pair 消费。
/// device_name/node_id 同时供 pending_pair_info（B1：设备管理窗重开时恢复
/// 确认弹窗的只读快照）读取。
pub(super) struct PendingPair {
    pub(super) device_name: String,
    pub(super) node_id: String,
    pub(super) decision_tx: oneshot::Sender<bool>,
}

impl DeviceLinkRegistry {
    // —— 邀请（host 侧）——

    /// 生成配对票据（覆盖旧邀请）并 emit PairInviteState。票据携带本端
    /// EndpointId + 当前中继 + 当前直连地址。
    ///
    /// 先尽力等待中继连接（`endpoint.online()`，上限 5s）：刚 bind 的端点在
    /// relay 分配前 `endpoint.addr().relay_urls()` 为空，会产出 LAN-only 票据，
    /// 跨网配对必失败。超时非致命——LAN-only 票据对局域网配对依然有效；
    /// 中继禁用（测试）时无从等待，直接跳过。
    pub(crate) async fn create_invite(&self) -> Result<String, String> {
        if !self.inner.relay_disabled {
            let _ = tokio::time::timeout(INVITE_ONLINE_WAIT, self.inner.endpoint.online()).await;
        }
        let endpoint_addr = self.inner.endpoint.addr();
        let relay = endpoint_addr.relay_urls().next().map(|url| url.to_string());
        let direct_addrs: Vec<String> = endpoint_addr
            .ip_addrs()
            .take(8)
            .map(|addr| addr.to_string())
            .collect();
        let (pin, secret) = self.inner.invites.lock_recover("invites").create();
        // 邀请存活期间广播固定快照（绝不含 PIN / secret）。测试构建不触真实 UDP。
        #[cfg(not(test))]
        {
            let generation = self
                .inner
                .invites
                .lock_recover("invites")
                .active_generation()
                .unwrap_or_default();
            let packet = crate::lan_sync::discovery::DiscoveryPacket {
                magic: crate::lan_sync::discovery::DISCOVERY_MAGIC.to_string(),
                device_name: crate::lan_sync::device_name(),
                endpoint_id: hex_encode_32(self.inner.endpoint.id().as_bytes()),
                addrs: direct_addrs.clone(),
                relay: relay.clone(),
            };
            let invites = self.inner.invites.clone();
            crate::lan_sync::discovery::spawn_broadcast_loop(packet, move || {
                invites.lock_recover("invites").active_generation() == Some(generation)
            });
        }
        let ticket = PairTicket {
            version: 1,
            endpoint_id: *self.inner.endpoint.id().as_bytes(),
            relay_url: relay,
            direct_addrs,
            invite_secret: secret,
        };
        let encoded = ticket.encode();
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64
            + INVITE_TTL.as_millis() as u64;
        self.emit(
            EVENT_PAIR_INVITE_STATE,
            &PairInviteState {
                ticket: Some(encoded.clone()),
                expires_at: Some(expires_at),
                pin: Some(format_pin(pin)),
                // 主机自身指纹：guest 侧确认步与本地卡片双端人工核对（H2）。
                fingerprint: Some(fingerprint_hex(self.inner.endpoint.id().as_bytes())),
            },
        );
        Ok(encoded)
    }

    /// 作废当前邀请并 emit PairInviteState{None, None}。
    pub(crate) fn cancel_invite(&self) -> Result<(), String> {
        self.inner.invites.lock_recover("invites").cancel();
        self.emit(
            EVENT_PAIR_INVITE_STATE,
            &PairInviteState {
                ticket: None,
                expires_at: None,
                pin: None,
                fingerprint: None,
            },
        );
        Ok(())
    }

    /// 用户对 pending 配对请求的决定。无 pending 时报错。
    pub(crate) fn respond_pair(&self, accept: bool) -> Result<(), String> {
        let pending = self.inner.pending_pair.lock_recover("pending").take();
        match pending {
            Some(pending) => {
                // Err（接收方已不在）只可能是流程竞态，忽略——连接侧按拒绝处理
                let _ = pending.decision_tx.send(accept);
                Ok(())
            }
            None => Err("当前没有待确认的配对请求".to_string()),
        }
    }

    /// 当前待确认配对请求的只读快照（v0.9.2 B1）：pair-request 事件是
    /// fire-and-forget，设备管理窗重开时前端凭本方法恢复确认弹窗。无 pending
    ///（无请求/已被消费）返回 None。payload 口径与 handle_inbound emit
    /// EVENT_PAIR_REQUEST 时一致（device_name + 对端 EndpointId 指纹短码）。
    pub(crate) fn pending_pair_info(&self) -> Option<PairRequested> {
        self.inner
            .pending_pair
            .lock_recover("pending")
            .as_ref()
            .map(|slot| PairRequested {
                device_name: slot.device_name.clone(),
                fingerprint: endpoint_id_from_hex(&slot.node_id)
                    .map(|id| fingerprint_hex(id.as_bytes()))
                    .unwrap_or_default(),
            })
    }

    // —— 加入（guest 侧，spec §4.3）——

    /// 凭票据配对。失败路径 emit `EVENT_PAIR_JOIN_FAILED`（票据本身的格式错误
    /// 直达返回，不 emit——那是用户贴错内容，不是连接失败）。
    pub(crate) async fn join(self: &Arc<Self>, ticket_str: &str) -> Result<(), String> {
        let ticket = PairTicket::decode(ticket_str)?;
        let secret_hex: String = ticket
            .invite_secret
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.pair_with_peer(
            ticket.endpoint_id,
            ticket.direct_addrs,
            ticket.relay_url,
            secret_hex,
            JOIN_REPLY_TIMEOUT,
            "等待对方响应超时",
            None,
        )
        .await
    }

    /// 票据与 PIN 两条路径共享的拨号 → 等待应答 → 落库 → 会话全流程。
    /// reply_timeout / timeout_reason 由调用路径决定（票据 30s、PIN 8s）。
    async fn pair_with_peer(
        self: &Arc<Self>,
        endpoint_id: [u8; 32],
        direct_addrs: Vec<String>,
        relay_url: Option<String>,
        invite_secret: String,
        reply_timeout: Duration,
        timeout_reason: &'static str,
        // PIN 路径专用：host 对无效邀请静默（不回 PairReject 且直接断连），
        // 此时把「对方已断开：connection lost」这类底层错误换成可行动的提示。
        silent_reason: Option<&'static str>,
    ) -> Result<(), String> {
        let fail = |reason: String| -> Result<(), String> {
            self.emit(
                EVENT_PAIR_JOIN_FAILED,
                &PairJoinFailed {
                    reason: reason.clone(),
                },
            );
            Err(reason)
        };
        // 目标是本地已撤销的设备：直接失败，不拨号（spec §3 撤销即失联——
        // 对端不会接受配对，拨号只会换来静默超时）。重新配对需先删除记录。
        let target_hex = hex_encode_32(&endpoint_id);
        if self.is_locally_revoked(&target_hex) {
            return fail("该设备已被撤销，如需重新配对请先在设备管理中删除它".to_string());
        }
        let addrs = parse_transport_addrs(&direct_addrs, relay_url.as_deref());
        if addrs.is_empty() {
            return fail("票据中没有可用的连接地址".to_string());
        }
        let Ok(peer_id) = EndpointId::from_bytes(&endpoint_id) else {
            return fail("票据中的设备标识无效".to_string());
        };
        let addr = EndpointAddr::from_parts(peer_id, addrs);
        let conn = match tokio::time::timeout(
            CONNECT_TIMEOUT,
            self.inner.endpoint.connect(addr, IPASTE_ALPN),
        )
        .await
        {
            Ok(Ok(conn)) => conn,
            Ok(Err(_)) => {
                return fail(match silent_reason {
                    Some(hint) => hint.to_string(),
                    None => "无法连接对方：网络不通或中继不可用".to_string(),
                });
            }
            Err(_) => {
                return fail(match silent_reason {
                    Some(hint) => hint.to_string(),
                    None => "连接对方超时".to_string(),
                });
            }
        };
        // 拨号方开首条流（配对流），发送 PairRequest
        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(pair) => pair,
            Err(e) => return fail(format!("对方已断开：{e}")),
        };
        let mut writer = FrameWriter::new(&mut send);
        if let Err(e) = writer
            .write_message(
                &LanMessage::PairRequest {
                    version: LAN_PROTOCOL_VERSION,
                    device_name: device_name(),
                    invite_secret,
                },
                None,
            )
            .await
        {
            return fail(format!("对方已断开：{e}"));
        }
        let mut reader = FrameReader::new(&mut recv);
        // 等应答限时：对端可能静默丢弃（无邀请/封禁期），不能无限挂起
        let reply = match tokio::time::timeout(reply_timeout, reader.read_message()).await {
            Ok(Ok((msg, _))) => msg,
            Ok(Err(e)) => {
                return fail(match silent_reason {
                    Some(hint) => hint.to_string(),
                    None => format!("对方已断开：{e}"),
                });
            }
            Err(_) => return fail(timeout_reason.to_string()),
        };
        let node_hex = hex_encode_32(conn.remote_id().as_bytes());
        match reply {
            LanMessage::PairAccept {
                device_name: host_name,
                ..
            } => {
                // 记录对端元数据（地址线索来自票据）
                if let Err(reason) = self.inner.store.upsert_paired_device(
                    &node_hex,
                    &host_name,
                    relay_url.as_deref(),
                    &direct_addrs,
                ) {
                    eprintln!("[lan-sync] 配对落库失败：{reason}");
                    return fail(format!("保存配对信息失败：{reason}"));
                }
                // 撤销过的行不复活（spec §3）
                if !self.inner.store.is_trusted(&node_hex).unwrap_or(false) {
                    return fail("该设备此前已被撤销，请先在设备管理中删除后再配对".to_string());
                }
                // 重新配对成功：解除此前的「显式断开」标记
                self.clear_disconnected(&node_hex);
                drop((send, recv)); // 关配对流（FIN），随后开第二条（会话）流
                                    // 拨号方开第二条流（会话流），并立即发首发帧让对端 accept_bi 解除挂起
                let (mut session_send, session_recv) = match conn.open_bi().await {
                    Ok(pair) => pair,
                    Err(e) => return fail(format!("对方已断开：{e}")),
                };
                if let Err(e) = send_stream_opener(&mut session_send).await {
                    return fail(format!("对方已断开：{e}"));
                }
                let dead_rx = Self::watch_conn_death(conn.clone());
                self.clone()
                    .run_session(
                        Some(conn.clone()),
                        node_hex,
                        session_recv,
                        session_send,
                        dead_rx,
                    )
                    .await;
                conn.close(VarInt::from_u32(0), b"session-end");
                Ok(())
            }
            LanMessage::PairReject { reason } => {
                let msg = match reason {
                    PairRejectReason::InviteInvalid => "邀请已失效，请让对方重新生成".to_string(),
                    PairRejectReason::Declined => "对方拒绝了配对请求".to_string(),
                    PairRejectReason::VersionMismatch => {
                        "对方 iPaste 版本过旧，请双方升级到 v0.9+".to_string()
                    }
                    PairRejectReason::Unknown => "配对失败".to_string(),
                };
                fail(msg)
            }
            _ => fail("对方响应异常".to_string()),
        }
    }

    /// 附近设备 + 6 位 PIN：地址取自 discovery 缓存，不信任前端拼地址。
    pub(crate) async fn join_nearby(
        self: &Arc<Self>,
        endpoint_id_hex: &str,
        pin: &str,
    ) -> Result<(), String> {
        let fail = |reason: String| -> Result<(), String> {
            self.emit(
                EVENT_PAIR_JOIN_FAILED,
                &PairJoinFailed {
                    reason: reason.clone(),
                },
            );
            Err(reason)
        };
        if !is_pin(pin) {
            return fail("PIN 必须是 6 位数字".to_string());
        }
        let Some(peer_id) = endpoint_id_from_hex(endpoint_id_hex) else {
            return fail("附近设备标识无效".to_string());
        };
        let Some((direct_addrs, relay_url)) = self
            .inner
            .discovery
            .table
            .lock_recover("nearby")
            .peer(endpoint_id_hex, Instant::now())
        else {
            return fail("附近设备已消失，请让对方重新生成邀请".to_string());
        };
        self.pair_with_peer(
            *peer_id.as_bytes(),
            direct_addrs,
            relay_url,
            pin.to_string(),
            NEARBY_REPLY_TIMEOUT,
            NEARBY_SILENT_REASON,
            Some(NEARBY_SILENT_REASON),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::TransportAddr;

    #[test]
    fn parse_transport_addrs_keeps_ticket_equivalence() {
        let direct = vec!["192.168.1.5:52123".to_string(), "not-an-addr".to_string()];
        let addrs = parse_transport_addrs(&direct, Some("https://relay.example.com"));
        assert_eq!(addrs.len(), 2, "非法直连地址被过滤，中继追加");
        assert!(matches!(addrs[0], TransportAddr::Ip(_)));
        assert!(matches!(addrs[1], TransportAddr::Relay(_)));
        assert!(parse_transport_addrs(&[], None).is_empty());
    }
}
