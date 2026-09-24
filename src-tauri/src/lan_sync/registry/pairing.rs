//! 配对域（原 registry.rs「邀请（host 侧）/加入（guest 侧）」段整体迁入，纯移动）：
//! host 侧邀请（create_invite/cancel_invite）、guest 侧加入（join）与用户确认
//! 状态（PendingPair/respond_pair/pending_pair_info）。

use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::VarInt;
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};
use tokio::sync::oneshot;

use crate::events::{
    PairInviteState, PairJoinFailed, PairRequested, EVENT_PAIR_INVITE_STATE,
    EVENT_PAIR_JOIN_FAILED,
};
use crate::lan_sync::frame::{FrameReader, FrameWriter};
use crate::lan_sync::protocol::{LanMessage, PairRejectReason, IPASTE_ALPN, LAN_PROTOCOL_VERSION};
use crate::lan_sync::session::fingerprint_hex;
use crate::lan_sync::ticket::{PairTicket, INVITE_TTL};
use crate::lan_sync::device_name;

use super::{
    send_stream_opener, DeviceLinkRegistry, CONNECT_TIMEOUT, endpoint_id_from_hex, hex_encode_32,
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
            let _ = tokio::time::timeout(
                INVITE_ONLINE_WAIT,
                self.inner.endpoint.online(),
            )
            .await;
        }
        let endpoint_addr = self.inner.endpoint.addr();
        let relay = endpoint_addr.relay_urls().next().map(|url| url.to_string());
        let direct_addrs: Vec<String> =
            endpoint_addr.ip_addrs().take(8).map(|addr| addr.to_string()).collect();
        let secret = self.inner.invites.lock().expect("invites 锁中毒").create();
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
            &PairInviteState { ticket: Some(encoded.clone()), expires_at: Some(expires_at) },
        );
        Ok(encoded)
    }

    /// 作废当前邀请并 emit PairInviteState{None, None}。
    pub(crate) fn cancel_invite(&self) -> Result<(), String> {
        self.inner.invites.lock().expect("invites 锁中毒").cancel();
        self.emit(
            EVENT_PAIR_INVITE_STATE,
            &PairInviteState { ticket: None, expires_at: None },
        );
        Ok(())
    }

    /// 用户对 pending 配对请求的决定。无 pending 时报错。
    pub(crate) fn respond_pair(&self, accept: bool) -> Result<(), String> {
        let pending = self.inner.pending_pair.lock().expect("pending 锁中毒").take();
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
            .lock()
            .expect("pending 锁中毒")
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
        let fail = |reason: String| -> Result<(), String> {
            self.emit(EVENT_PAIR_JOIN_FAILED, &PairJoinFailed { reason: reason.clone() });
            Err(reason)
        };
        // 目标是本地已撤销的设备：直接失败，不拨号（spec §3 撤销即失联——
        // 对端不会接受配对，拨号只会换来静默超时）。重新配对需先删除记录。
        let target_hex = hex_encode_32(&ticket.endpoint_id);
        if self.is_locally_revoked(&target_hex) {
            return fail("该设备已被撤销，如需重新配对请先在设备管理中删除它".to_string());
        }
        let mut addrs: Vec<TransportAddr> = ticket
            .direct_addrs
            .iter()
            .filter_map(|addr| addr.parse::<std::net::SocketAddr>().ok().map(TransportAddr::Ip))
            .collect();
        if let Some(relay) = &ticket.relay_url {
            if let Ok(url) = relay.parse::<RelayUrl>() {
                addrs.push(TransportAddr::Relay(url));
            }
        }
        if addrs.is_empty() {
            return fail("票据中没有可用的连接地址".to_string());
        }
        let Ok(peer_id) = EndpointId::from_bytes(&ticket.endpoint_id) else {
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
            Ok(Err(_)) => return fail("无法连接对方：网络不通或中继不可用".to_string()),
            Err(_) => return fail("连接对方超时".to_string()),
        };
        // 拨号方开首条流（配对流），发送 PairRequest
        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(pair) => pair,
            Err(e) => return fail(format!("对方已断开：{e}")),
        };
        let secret_hex: String = ticket.invite_secret.iter().map(|b| format!("{b:02x}")).collect();
        let mut writer = FrameWriter::new(&mut send);
        if let Err(e) = writer
            .write_message(
                &LanMessage::PairRequest {
                    version: LAN_PROTOCOL_VERSION,
                    device_name: device_name(),
                    invite_secret: secret_hex,
                },
                None,
            )
            .await
        {
            return fail(format!("对方已断开：{e}"));
        }
        let mut reader = FrameReader::new(&mut recv);
        // 等应答限时：对端可能静默丢弃（无邀请/封禁期），不能无限挂起
        let reply = match tokio::time::timeout(JOIN_REPLY_TIMEOUT, reader.read_message()).await {
            Ok(Ok((msg, _))) => msg,
            Ok(Err(e)) => return fail(format!("对方已断开：{e}")),
            Err(_) => return fail("等待对方响应超时".to_string()),
        };
        let node_hex = hex_encode_32(conn.remote_id().as_bytes());
        match reply {
            LanMessage::PairAccept { device_name: host_name, .. } => {
                // 记录对端元数据（地址线索来自票据）
                if let Err(reason) = self.inner.store.upsert_paired_device(
                    &node_hex,
                    &host_name,
                    ticket.relay_url.as_deref(),
                    &ticket.direct_addrs,
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
                    .run_session(Some(conn.clone()), node_hex, session_recv, session_send, dead_rx)
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
}
