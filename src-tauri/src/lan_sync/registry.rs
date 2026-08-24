//! DeviceLinkRegistry：iroh Endpoint 生命周期 + 每设备连接管理（spec §5）。
//!
//! 职责：
//! - 持有 iroh `Endpoint`（协议 v5 的唯一传输），入站连接分流（已配对 → 会话流；
//!   陌生 → 票据配对流程）。
//! - 已配对设备各一条后台重拨任务（link_task）：拨号 → 会话 → 断开 → 指数退避重拨。
//! - 邀请/加入配对（`create_invite`/`join`/`respond_pair`）。
//! - 按设备分发发送指令（`send_raw`/`send_category`/`request_clip`）与设备管理
//!   （`revoke`/`delete_device`/`set_auto_sync`/`disconnect`）。
//!
//! 锁纪律：`links`/`invites`/`pending_pair`/`accept_task` 是 std `Mutex`，只在
//! 无 `.await` 的短临界区内持有；跨 await 的共享一律 clone 出来再操作。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use iroh::endpoint::presets;
use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey, TransportAddr};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};

use crate::events::{
    DeviceCategorySent, DeviceListChanged, DeviceStatusChanged, PairInviteState, PairJoinFailed,
    PairRequested, EVENT_DEVICE_CATEGORY_SENT, EVENT_DEVICE_LIST_CHANGED,
    EVENT_DEVICE_STATUS_CHANGED, EVENT_PAIR_INVITE_STATE, EVENT_PAIR_JOIN_FAILED,
    EVENT_PAIR_REQUEST,
};
use crate::lan_sync::autopush::{fan_out_targets, RecentReceived};
use crate::lan_sync::frame::{FrameReader, FrameWriter};
use crate::lan_sync::pair_guard::PairGuard;
use crate::lan_sync::protocol::{
    LanMessage, PairRejectReason, IPASTE_ALPN, LAN_MAX_PAYLOAD, LAN_PROTOCOL_VERSION,
};
use crate::lan_sync::session::{fingerprint_hex, run_session_loop, SessionCtx};
use crate::lan_sync::ticket::{InviteRegistry, PairTicket, INVITE_TTL};
use crate::lan_sync::{device_name, ControlMsg, LanEventSink};
use crate::models::{AutoSyncMode, ClipItem, DeviceInfo, DeviceOnline};
use crate::store::Store;

/// 重拨退避序列：5s→10s→20s→40s→80s→160s，之后恒为 300s（spec §5）。
const RECONNECT_BACKOFF: [Duration; 6] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
    Duration::from_secs(80),
    Duration::from_secs(160),
];
const RECONNECT_BACKOFF_CAP: Duration = Duration::from_secs(300);

/// 拨号/加入的超时：中继路径下 QUIC 握手可能较慢，给足 15s。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// 入站会话在线时 link_task 的轮询间隔：对端会话死亡后由下一轮接管重拨。
const INBOUND_SESSION_POLL: Duration = Duration::from_secs(5);

/// create_invite 等待中继连接（endpoint.online()）的上限；超时非致命，
/// LAN-only 票据对局域网配对依然有效。
const INVITE_ONLINE_WAIT: Duration = Duration::from_secs(5);

/// 陌生连接预认证（accept_bi + 首帧 PairRequest）的限时：迟迟不发首帧的
/// 连接到期静默关闭，防 slow-loris 式无限挂住配对任务（任务堆积）。
const STRANGER_PREAUTH_TIMEOUT: Duration = Duration::from_secs(60);

/// join 拨号后等待 PairAccept/PairReject 的限时：对端静默丢弃（如邀请无效）
/// 时拨号方不能无限挂起。测试构建缩短到 2s，超时路径可低成本回归。
#[cfg(not(test))]
const JOIN_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const JOIN_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// host 侧等待用户确认配对的限时：超时按拒绝处理（回 PairReject{Declined}），
/// 防陌生人持有效票据把确认弹窗永久钉死；正常用户 120s 足够操作。
const PAIR_CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// 第 N 次连续失败后的退避时长（0 基）。纯函数，独立测试。
fn reconnect_backoff(attempt: usize) -> Duration {
    RECONNECT_BACKOFF.get(attempt).copied().unwrap_or(RECONNECT_BACKOFF_CAP)
}

/// EndpointId（32B）→ 64 字符 hex（v5 的设备标识形态，同 paired_devices.node_id）。
fn hex_encode_32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 字符 hex → EndpointId；非法定长 hex 返回 None。
fn endpoint_id_from_hex(input: &str) -> Option<EndpointId> {
    if input.len() != 64 || !input.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in input.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)?;
        let lo = (chunk[1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    EndpointId::from_bytes(&out).ok()
}

struct Inner {
    endpoint: Endpoint,
    store: Store,
    sink: Arc<dyn LanEventSink>,
    invites: Mutex<InviteRegistry>,
    /// node_id hex -> 该设备的运行态（会话控制通道 + 在线状态 + 重拨任务句柄）。
    links: Mutex<HashMap<String, LinkHandle>>,
    pending_pair: Mutex<Option<PendingPair>>,
    /// 配对防爆破，key = 对端 node_id hex。
    guard: PairGuard,
    /// 用户显式断开的设备（node_id hex）：已配对也静默拒绝入站会话、本端不重拨。
    /// 仅内存态——重启即清空（与「重新配对或重启应用后恢复」语义一致）。
    disconnected: Mutex<HashSet<String>>,
    /// 最近接收哈希滑窗（registry 级单例）：auto 接收路径登记，Task 3 的发送侧
    /// 扇出经同一实例防回推。
    recent: Arc<RecentReceived>,
    /// auto 推送因目标队列满/会话死亡而丢弃的累计计数（诊断用；
    /// fan_out_auto 的 try_send 路径递增）。
    auto_dropped: AtomicU64,
    /// 中继是否禁用（RelayMode::Disabled）：禁用时 create_invite 无需等待 online。
    relay_disabled: bool,
    /// 入站接受循环任务句柄（shutdown 时 abort）。
    accept_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// links 条目代次计数：会话结束时只清理仍属于自己的登记（防误删新会话）。
    gen: AtomicU64,
}

/// 单设备链路登记。`control_tx` 是该设备**当前会话**的控制通道所有权：
/// 条目被移除/替换时 sender 被 drop，会话循环收到 `None` 即干净关闭（发 Disconnect 帧）。
struct LinkHandle {
    gen: u64,
    /// `None` = 无活跃会话（重拨任务在退避/拨号中，或已被撤销清理）。
    control_tx: Option<mpsc::Sender<ControlMsg>>,
    status: DeviceOnline,
    /// 该设备重拨任务（link_task）的句柄；配对/入站建立的会话为 `None`
    /// （会话结束后由 registry 重新起任务接管重拨）。abort(&self) 不需要所有权。
    task: Option<tokio::task::JoinHandle<()>>,
}

/// 待用户确认的配对请求：decision_tx 由 respond_pair 消费。
struct PendingPair {
    #[allow(dead_code)] // 调试用字段保留：定位是哪台设备的请求
    device_name: String,
    #[allow(dead_code)]
    node_id: String,
    decision_tx: oneshot::Sender<bool>,
}

/// send_category 的单目标发送状态（流式逐条发送时的聚合账本）：
/// `started=false`（BatchStart 未达）的目标不参与后续、不发汇总事件；
/// `dead=true` 后该目标剩余条目逐条计 failed（与会话中断前的语义一致）。
struct CategorySendTarget {
    node_id: String,
    tx: mpsc::Sender<ControlMsg>,
    started: bool,
    dead: bool,
    sent: u32,
    failed: u32,
}

/// DeviceLinkRegistry：所有公开方法供 Task 8 命令层与 Task 9 测试消费。
pub struct DeviceLinkRegistry {
    inner: Arc<Inner>,
}

impl DeviceLinkRegistry {
    /// 生产入口：固定设备身份 + n0 预设（默认中继 + DNS 地址发现）+ 指定中继模式。
    pub(crate) async fn start(
        secret: SecretKey,
        store: Store,
        sink: Arc<dyn LanEventSink>,
        relay: RelayMode,
    ) -> Result<Arc<Self>, String> {
        let relay_disabled = matches!(relay, RelayMode::Disabled);
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret)
            .alpns(vec![IPASTE_ALPN.to_vec()])
            .relay_mode(relay)
            .bind()
            .await
            .map_err(|e| format!("无法启动同步端点：{e}"))?;
        Self::from_endpoint(endpoint, store, sink, relay_disabled).await
    }

    /// 测试入口：最小预设 + 禁用中继 + 随机端口（hermetic，不触外网）。
    #[cfg(test)]
    pub(crate) async fn start_for_test(
        store: Store,
        sink: Arc<dyn LanEventSink>,
    ) -> Result<Arc<Self>, String> {
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::generate())
            .alpns(vec![IPASTE_ALPN.to_vec()])
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .map_err(|e| format!("无法启动测试端点：{e}"))?;
        Self::from_endpoint(endpoint, store, sink, true).await
    }

    async fn from_endpoint(
        endpoint: Endpoint,
        store: Store,
        sink: Arc<dyn LanEventSink>,
        relay_disabled: bool,
    ) -> Result<Arc<Self>, String> {
        let registry = Arc::new(Self {
            inner: Arc::new(Inner {
                endpoint,
                store,
                sink,
                invites: Mutex::new(InviteRegistry::new()),
                links: Mutex::new(HashMap::new()),
                pending_pair: Mutex::new(None),
                guard: PairGuard::new(),
                disconnected: Mutex::new(HashSet::new()),
                recent: Arc::new(RecentReceived::new()),
                auto_dropped: AtomicU64::new(0),
                relay_disabled,
                accept_task: Mutex::new(None),
                gen: AtomicU64::new(1),
            }),
        });
        // 入站分流循环
        let accept_registry = registry.clone();
        let accept_task = tokio::spawn(async move { accept_registry.accept_loop().await });
        *registry
            .inner
            .accept_task
            .lock()
            .expect("accept_task 锁中毒") = Some(accept_task);
        // 已配对且未撤销的设备各起一条重拨任务
        let devices = match registry.inner.store.list_paired_devices() {
            Ok(devices) => devices,
            Err(reason) => {
                registry.shutdown();
                return Err(reason);
            }
        };
        for device in devices {
            if device.revoked_at.is_none() {
                registry.spawn_link_task(device.node_id);
            }
        }
        registry.emit_device_list();
        Ok(registry)
    }

    // —— 事件出口 ——

    fn emit<E: serde::Serialize>(&self, event: &str, payload: E) {
        let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
        self.inner.sink.emit(event, &value);
    }

    fn emit_status(&self, node_id: &str, status: DeviceOnline) {
        self.emit(
            EVENT_DEVICE_STATUS_CHANGED,
            &DeviceStatusChanged { node_id: node_id.to_string(), status },
        );
    }

    fn emit_device_list(&self) {
        let devices = self.device_infos();
        self.emit(EVENT_DEVICE_LIST_CHANGED, &DeviceListChanged { devices });
    }

    /// link_task 专属的状态写：仅当登记仍是认领时的代次（gen 相同）且无活跃
    /// 会话（control_tx 为 None）才写，否则 no-op。
    ///
    /// 为什么必须 gen-aware：对端重拨落地时，入站 run_session 的收编分支会把
    /// 登记原子地换成新 gen + 新 control_tx（状态 Connected）。若无条件写状态
    ///（如会话结束后的 Offline），会把**存活中的入站会话**的状态字段砸成
    /// Offline——status 又被 has_live_session 当作在线判据时，link_task 误判
    /// 「无会话」而重拨，双向互踢以 5s 节奏永久震荡。gen 不匹配 = 登记已易主。
    fn set_status_if_owner(&self, node_id: &str, gen: u64, status: DeviceOnline) {
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            let Some(handle) = links.get_mut(node_id) else { return };
            if handle.gen != gen || handle.control_tx.is_some() {
                return; // 登记已易主（新会话收编）或有活跃会话：不覆写
            }
            if handle.status == status {
                return; // 状态未变：不重复 emit
            }
            handle.status = status;
        }
        self.emit_status(node_id, status);
    }

    fn next_gen(&self) -> u64 {
        self.inner.gen.fetch_add(1, Ordering::Relaxed)
    }

    // —— 入站分流与配对（spec §4.3）——

    async fn accept_loop(self: Arc<Self>) {
        // accept() 在 endpoint 关闭时返回 None → 循环自然结束
        while let Some(incoming) = self.inner.endpoint.accept().await {
            let Ok(accepting) = incoming.accept() else { continue };
            let registry = self.clone();
            tokio::spawn(async move {
                let Ok(conn) = accepting.await else { return };
                registry.handle_inbound(conn).await;
            });
        }
    }

    /// 入站会话门：已配对（信任且未撤销）且未被用户显式断开。
    fn inbound_allowed(&self, node_hex: &str) -> bool {
        let trusted = self.inner.store.is_trusted(node_hex).unwrap_or(false);
        trusted
            && !self
                .inner
                .disconnected
                .lock()
                .expect("disconnected 锁中毒")
                .contains(node_hex)
    }

    /// 陌生路径的撤销门：对端 node_id 在本地是「已撤销」的行 → 静默拒绝
    ///（不提示、不回帧——spec §3 撤销即失联）。必须放在邀请校验与用户确认
    /// 之前：否则撤销设备持有效票据会触发配对弹窗 + 可区分的 PairReject，
    /// 构成信任态预言机。重新配对需先在设备管理中删除记录。
    fn is_locally_revoked(&self, node_hex: &str) -> bool {
        self.inner
            .store
            .get_paired_device(node_hex)
            .ok()
            .flatten()
            .is_some_and(|device| device.revoked_at.is_some())
    }

    /// 清除「显式断开」标记（重新配对成功 / 撤销 / 删除时调用）。
    fn clear_disconnected(&self, node_id: &str) {
        self.inner
            .disconnected
            .lock()
            .expect("disconnected 锁中毒")
            .remove(node_id);
    }

    /// 单条入站连接：已配对 → 直接进会话；陌生 → 票据配对流程。
    async fn handle_inbound(self: Arc<Self>, conn: Connection) {
        let remote = conn.remote_id();
        let node_hex = hex_encode_32(remote.as_bytes());
        if self.inner.store.is_trusted(&node_hex).unwrap_or(false) {
            // 用户显式断开过：静默拒绝（不进会话也不进配对流程；
            // 恢复需重新配对或重启应用——重启清空内存态标记）
            if !self.inbound_allowed(&node_hex) {
                conn.close(VarInt::from_u32(0), b"disconnected");
                return;
            }
            // 已配对：对端是拨号方 → 对端开首条（会话）流，本地 accept_bi
            if let Ok((send, recv)) = conn.accept_bi().await {
                let dead_rx = Self::watch_conn_death(conn.clone());
                self.run_session(node_hex, recv, send, dead_rx).await;
            }
            conn.close(VarInt::from_u32(0), b"session-end");
            return;
        }
        // 陌生连接：首条双向流必须是 PairRequest，其余一切情况静默丢弃
        //（spec §4.2：无邀请的连接不产生任何提示，防提示轰炸/探测）。
        // 撤销门在最前（读流/校验邀请之前）：已撤销设备持有效票据再次拨入
        // 也按静默拒绝处理——不弹配对请求、不回任何帧（spec §3）。
        if self.is_locally_revoked(&node_hex) {
            return;
        }
        // 预认证读取限时（slow-loris 防护）：accept_bi 与首帧 PairRequest
        // 合计 60s 内不完成为静默关闭，配对任务不无限堆积。
        let preauth = tokio::time::timeout(STRANGER_PREAUTH_TIMEOUT, async {
            let (send, mut recv) = conn.accept_bi().await.ok()?;
            let mut reader = FrameReader::new(&mut recv);
            let (LanMessage::PairRequest { version, device_name: peer_name, invite_secret }, _) =
                reader.read_message().await.ok()?
            else {
                return None;
            };
            Some((send, version, peer_name, invite_secret))
        })
        .await
        .ok()
        .flatten();
        let Some((mut send, version, peer_name, invite_secret)) = preauth else {
            conn.close(VarInt::from_u32(0), b"pair-preauth-timeout");
            return; // 静默拒绝
        };
        if version != LAN_PROTOCOL_VERSION {
            reply_reject(&mut send, PairRejectReason::VersionMismatch).await;
            return;
        }
        // 防爆破（key = node_id hex）+ 邀请校验
        self.inner.guard.prune(std::time::Instant::now());
        let now = std::time::Instant::now();
        if self.inner.guard.is_blocked(&node_hex, now) {
            return; // 封禁期：静默
        }
        let verified = self
            .inner
            .invites
            .lock()
            .expect("invites 锁中毒")
            .verify_and_consume(&invite_secret);
        if !verified {
            let delay = self.inner.guard.record_failure(&node_hex, std::time::Instant::now());
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            return; // 邀请无效：静默（不回 PairReject，避免给探测者反馈）
        }
        self.inner.guard.record_success(&node_hex);
        // 用户确认（oneshot + 事件）
        let (decision_tx, decision_rx) = oneshot::channel();
        *self.inner.pending_pair.lock().expect("pending 锁中毒") = Some(PendingPair {
            device_name: peer_name.clone(),
            node_id: node_hex.clone(),
            decision_tx,
        });
        let fingerprint = fingerprint_hex(remote.as_bytes());
        self.emit(
            EVENT_PAIR_REQUEST,
            &PairRequested { device_name: peer_name.clone(), fingerprint },
        );
        // Ok(false) = 用户拒绝；Err = pending 槽被新请求覆盖（旧请求按拒绝处理，
        // 但不动槽——槽现在属于新请求）
        let accepted = match tokio::time::timeout(PAIR_CONFIRM_TIMEOUT, decision_rx).await {
            Ok(decision) => matches!(decision, Ok(true)),
            Err(_) => {
                // 120s 无人应答（弹窗被忽略）：按拒绝处理并清理 pending 槽。
                // 只在槽仍是本请求时清理——本请求的 decision_rx 已随超时 drop，
                // 槽内 decision_tx 呈 closed 态即为本请求残留；若已被新请求
                // 覆盖（tx 存活）则不动。
                let mut pending = self.inner.pending_pair.lock().expect("pending 锁中毒");
                if pending
                    .as_ref()
                    .is_some_and(|slot| slot.decision_tx.is_closed())
                {
                    *pending = None;
                }
                false
            }
        };
        if !accepted {
            reply_reject(&mut send, PairRejectReason::Declined).await;
            return;
        }
        // 注：PairRequested 事件不带请求 id，前端确认弹窗在前述 120s 自动拒绝后
        // 仍会停留在屏幕上，直到用户点击（点击时若 pending 已清空，
        // respond_pair 报「当前没有待确认的配对请求」）——配对本身已被拒。
        // 接受：互写信任表 + PairAccept + 会话流（本端为被拨方 → accept_bi）
        if let Err(reason) =
            self.inner.store.upsert_paired_device(&node_hex, &peer_name, None, &[])
        {
            eprintln!("[lan-sync] 配对落库失败：{reason}");
            reply_reject(&mut send, PairRejectReason::Unknown).await;
            return;
        }
        // 撤销过的行不复活（spec §3）：撤销后再次配对必须先删除记录
        if !self.inner.store.is_trusted(&node_hex).unwrap_or(false) {
            eprintln!("[lan-sync] 设备 {node_hex} 已被撤销，拒绝重新配对（需先删除记录）");
            reply_reject(&mut send, PairRejectReason::Unknown).await;
            return;
        }
        // 重新配对成功：解除此前的「显式断开」标记
        self.clear_disconnected(&node_hex);
        let me_name = device_name();
        let my_id = self.inner.endpoint.id();
        let mut writer = FrameWriter::new(&mut send);
        let accepted_msg = LanMessage::PairAccept {
            version: LAN_PROTOCOL_VERSION,
            device_name: me_name,
            fingerprint: fingerprint_hex(my_id.as_bytes()),
        };
        if writer.write_message(&accepted_msg, None).await.is_err() {
            return;
        }
        drop(send); // 关配对流（FIN），拨号方随即开第二条（会话）流
        // 等拨号方开第二条（会话）流（拨号方开流即发首发帧，见 send_stream_opener）
        let Ok((session_send, session_recv)) = conn.accept_bi().await else { return };
        let dead_rx = Self::watch_conn_death(conn.clone());
        self.run_session(node_hex, session_recv, session_send, dead_rx)
            .await;
        conn.close(VarInt::from_u32(0), b"session-end");
    }

    /// conn.closed() 监视任务：连接死亡时经 oneshot 通知会话循环。
    /// 返回的 Receiver 由会话循环持有；sender 存活于本任务直到 closed() 解除——
    /// **绝不提前 drop**（sender drop 即触发会话立即结束）。
    fn watch_conn_death(conn: Connection) -> oneshot::Receiver<()> {
        let (dead_tx, dead_rx) = oneshot::channel();
        tokio::spawn(async move {
            conn.closed().await;
            let _ = dead_tx.send(());
        });
        dead_rx
    }

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
                    .run_session(node_hex, session_recv, session_send, dead_rx)
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

    // —— 重拨任务与会话（spec §5）——

    /// 为已配对设备启动后台重拨任务（links 里已有该设备登记则不重复启动）。
    /// 占位与 spawn 在同一临界区内完成，防并发重复启动。
    fn spawn_link_task(self: &Arc<Self>, node_id: String) {
        let mut links = self.inner.links.lock().expect("links 锁中毒");
        if links.contains_key(&node_id) {
            return;
        }
        let registry = self.clone();
        let task_node = node_id.clone();
        // tokio::spawn 是同步调用，持锁期间调用不违反「无 await 持锁」纪律
        let task = tokio::spawn(async move { registry.link_task(task_node).await });
        links.insert(
            node_id,
            LinkHandle {
                gen: self.next_gen(),
                control_tx: None,
                status: DeviceOnline::Connecting,
                task: Some(task),
            },
        );
    }

    /// links 里该设备是否已有「会话在线」的登记（Connected 且控制通道存活）。
    /// 典型场景：对端拨来的入站会话收编了本端登记。
    /// 判据是 **control_tx 存活**（活跃会话的 sender 在登记里），不看 status——
    /// status 只是展示层快照，任何遗留/并发的非 gen-aware 写都可能让它短暂失真，
    /// 拿它当在线判据会重新打开互踢震荡的口子。
    fn has_live_session(&self, node_id: &str) -> bool {
        let links = self.inner.links.lock().expect("links 锁中毒");
        links.get(node_id).is_some_and(|handle| handle.control_tx.is_some())
    }

    /// 每设备后台任务：循环「拨号 → 会话 → 断开 → 退避重拨」（spec §5）。
    /// 每轮从 store 刷新信任态与地址线索（运行中可能被更新/撤销）。
    async fn link_task(self: Arc<Self>, node_id: String) {
        let mut attempt: usize = 0;
        // store 查不到（已删除）或已撤销 → 循环结束（撤销即断链）
        while let Some(device) = self.inner.store.get_paired_device(&node_id).ok().flatten() {
            if device.revoked_at.is_some() {
                break;
            }
            // 对端拨来的会话已在本端在线（收编后的入站登记）：跳过本端拨号。
            // 否则双向 link_task 互踢——A 重拨收编 B 的会话 → B 的会话死 →
            // B 重拨收编 A 的会话 → ……以 5s 一次的节奏永久连接抖动。
            // 对端会话死亡（control_tx 清空）后由下一轮循环自然接管重拨。
            if self.has_live_session(&node_id) {
                tokio::time::sleep(INBOUND_SESSION_POLL).await;
                continue; // 不增加退避计数：这不是拨号失败
            }
            // 认领当前登记（无活跃会话时的 gen）作为本轮状态写的所有权凭据。
            // 拨号期间（最长 15s）若有入站会话收编登记，gen 变化 → 状态写自动 no-op。
            let claim_gen = {
                let links = self.inner.links.lock().expect("links 锁中毒");
                links
                    .get(&node_id)
                    .filter(|handle| handle.control_tx.is_none())
                    .map(|handle| handle.gen)
            };
            if let Some(gen) = claim_gen {
                self.set_status_if_owner(&node_id, gen, DeviceOnline::Connecting);
            }
            match self
                .dial(&node_id, device.relay_url.as_deref(), &device.direct_addrs)
                .await
            {
                Ok((conn, send, recv)) => {
                    attempt = 0;
                    self.inner.store.touch_last_seen(&node_id).ok();
                    if let Some(gen) = claim_gen {
                        self.set_status_if_owner(&node_id, gen, DeviceOnline::Connected);
                    }
                    let dead_rx = Self::watch_conn_death(conn.clone());
                    // run_session 按值拿走 Arc：clone 一份给本次会话；
                    // 返回本会话的 gen，供会话结束后的 Offline 写做所有权校验
                    let session_gen = self
                        .clone()
                        .run_session(node_id.clone(), recv, send, dead_rx)
                        .await;
                    conn.close(VarInt::from_u32(0), b"session-end");
                    // 会话结束：只有登记仍是本会话的 gen（未被入站会话收编）才置
                    // Offline；被收编则 no-op——存活的入站会话不受影响，link_task
                    // 在循环顶的 guard 处停靠，不再重拨（Fix：互踢震荡）。
                    self.set_status_if_owner(&node_id, session_gen, DeviceOnline::Offline);
                }
                Err(reason) => {
                    eprintln!("[lan-sync] 拨号 {node_id} 失败：{reason}");
                    // 拨号失败同样回到 Offline（凭据是认领 gen；期间被收编则 no-op）
                    if let Some(gen) = claim_gen {
                        self.set_status_if_owner(&node_id, gen, DeviceOnline::Offline);
                    }
                }
            }
            tokio::time::sleep(reconnect_backoff(attempt)).await;
            attempt += 1;
        }
        // 任务退出：清掉自己的登记（仅当无活跃会话占用时）
        let mut remove = false;
        {
            let links = self.inner.links.lock().expect("links 锁中毒");
            if links
                .get(&node_id)
                .is_some_and(|handle| handle.control_tx.is_none())
            {
                remove = true;
            }
        }
        if remove {
            self.inner.links.lock().expect("links 锁中毒").remove(&node_id);
            self.emit_device_list();
        }
    }

    /// 组装 EndpointAddr（直连 + 中继）并拨号；成功后由调用方开会话流。
    /// 对端元数据回写跳过（iroh 连接信息取不到对端新地址，保留 store 里
    /// 票据/历史线索即可——brief 允许）。
    async fn dial(
        &self,
        node_id: &str,
        relay: Option<&str>,
        addrs: &[String],
    ) -> Result<(Connection, SendStream, RecvStream), String> {
        let Some(peer_id) = endpoint_id_from_hex(node_id) else {
            return Err(format!("设备标识无效：{node_id}"));
        };
        let mut transports: Vec<TransportAddr> = addrs
            .iter()
            .filter_map(|addr| addr.parse::<std::net::SocketAddr>().ok().map(TransportAddr::Ip))
            .collect();
        if let Some(relay) = relay {
            if let Ok(url) = relay.parse::<RelayUrl>() {
                transports.push(TransportAddr::Relay(url));
            }
        }
        if transports.is_empty() {
            return Err("没有可用的连接地址".to_string());
        }
        let addr = EndpointAddr::from_parts(peer_id, transports);
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, self.inner.endpoint.connect(addr, IPASTE_ALPN))
            .await
            .map_err(|_| "连接超时".to_string())?
            .map_err(|e| e.to_string())?;
        let (mut send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        // 拨号方首发帧：对端（已配对入站分支）的 accept_bi 才会解除挂起
        send_stream_opener(&mut send).await?;
        Ok((conn, send, recv))
    }

    /// 注册并运行一个会话（入站/配对/重拨共用入口）。收编语义：同设备已有旧
    /// 登记时，旧 control_tx 随替换被 drop → 旧会话干净关闭；重拨任务句柄继承。
    /// 返回本会话的 gen（调用方 link_task 以此做会话后状态写的所有权校验）。
    async fn run_session<R, W>(
        self: Arc<Self>,
        node_hex: String,
        read: R,
        write: W,
        dead: oneshot::Receiver<()>,
    ) -> u64
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (control_tx, control_rx) = mpsc::channel(16);
        let my_gen = self.next_gen();
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            use std::collections::hash_map::Entry;
            match links.entry(node_hex.clone()) {
                // 已有登记：收编（旧 control_tx 在赋值时 drop → 旧会话收到 None 干净关闭；
                // 旧重拨任务保留，不 abort）
                Entry::Occupied(mut occupied) => {
                    let handle = occupied.get_mut();
                    handle.gen = my_gen;
                    handle.control_tx = Some(control_tx);
                    handle.status = DeviceOnline::Connected;
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(LinkHandle {
                        gen: my_gen,
                        control_tx: Some(control_tx),
                        status: DeviceOnline::Connected,
                        task: None,
                    });
                }
            }
        }
        self.emit_status(&node_hex, DeviceOnline::Connected);
        self.emit_device_list();
        let peer_device_name = self
            .inner
            .store
            .get_paired_device(&node_hex)
            .ok()
            .flatten()
            .map(|device| device.device_name)
            .unwrap_or_else(|| node_hex.chars().take(8).collect());
        let ctx = SessionCtx {
            sink: self.inner.sink.clone(),
            store: self.inner.store.clone(),
            peer_node_id: node_hex.clone(),
            peer_device_name,
            // 本机身份（origin 自环防御基准）+ registry 级 recent 单例 +
            // auto 轻提示开关（每次会话建立时重读设置，改动即时生效于新会话）。
            local_node_id: hex_encode_32(self.inner.endpoint.id().as_bytes()),
            recent: self.inner.recent.clone(),
            auto_notify: self
                .inner
                .store
                .auto_push_settings()
                .map(|settings| settings.notify)
                .unwrap_or(false),
        };
        run_session_loop(read, write, ctx, control_rx, dead).await;
        // 会话结束：只清理仍属于自己的登记（gen 相同）；被新会话收编则不动。
        // task 存活（is_finished=false）才有重拨接管者；配对/入站建立的会话没有
        // 任务，或继承的任务已退出（如撤销导致 link_task break）——都走移除+重起。
        let mut remove_entry = false;
        let mut owned = false;
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            if let Some(handle) = links.get_mut(&node_hex) {
                if handle.gen == my_gen {
                    owned = true;
                    let task_alive = handle
                        .task
                        .as_ref()
                        .is_some_and(|task| !task.is_finished());
                    if task_alive {
                        // 有重拨任务接管：登记保留，状态置 Offline，任务继续循环
                        handle.control_tx = None;
                        handle.status = DeviceOnline::Offline;
                    } else {
                        remove_entry = true; // 无接管任务：移除登记，稍后按需重起
                    }
                }
            }
        }
        if remove_entry {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            // 复查 gen：两段锁之间可能已被新会话收编
            if links.get(&node_hex).is_some_and(|handle| handle.gen == my_gen) {
                links.remove(&node_hex);
            }
        }
        // Offline 状态事件只在登记仍属于本会话时发：被收编意味着同一设备的
        // 新会话已在线（收编登记时已发过 Connected），再发 Offline 是假事件。
        if owned {
            self.emit_status(&node_hex, DeviceOnline::Offline);
        }
        self.emit_device_list();
        // 无重拨任务的会话（配对建立）结束：若设备仍可信则起任务接管后续重拨
        if remove_entry && self.inner.store.is_trusted(&node_hex).unwrap_or(false) {
            self.spawn_link_task(node_hex);
        }
        my_gen
    }

    // —— 发送分发 ——

    /// 解析在线目标：None = 全部 Connected 链路；Some = 指定设备。
    /// 结果为空报「没有在线的目标设备」。锁内只 clone sender，不 await。
    fn online_targets(
        &self,
        target: Option<&str>,
    ) -> Result<Vec<(String, mpsc::Sender<ControlMsg>)>, String> {
        let links = self.inner.links.lock().expect("links 锁中毒");
        let mut out: Vec<(String, mpsc::Sender<ControlMsg>)> = Vec::new();
        match target {
            None => {
                for (node, handle) in links.iter() {
                    if handle.status == DeviceOnline::Connected {
                        if let Some(tx) = &handle.control_tx {
                            out.push((node.clone(), tx.clone()));
                        }
                    }
                }
            }
            Some(node_id) => {
                if let Some(handle) = links.get(node_id) {
                    if handle.status == DeviceOnline::Connected {
                        if let Some(tx) = &handle.control_tx {
                            out.push((node_id.to_string(), tx.clone()));
                        }
                    }
                }
            }
        }
        drop(links);
        if out.is_empty() {
            Err("没有在线的目标设备".to_string())
        } else {
            Ok(out)
        }
    }

    /// 发送单条剪贴板内容（无分组语义时 category_* 传 None）。
    /// 指定设备不在线 / 无任何在线设备时报错；个别目标会话已死时跳过并记日志。
    pub(crate) async fn send_raw(
        &self,
        target: Option<&str>,
        clip_type: &str,
        payload: &[u8],
        category_name: Option<&str>,
        category_color: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        let targets = self.online_targets(target)?;
        for (node_id, tx) in targets {
            let msg = ControlMsg::SendClip {
                clip_type: clip_type.to_string(),
                payload: payload.to_vec(),
                category_name: category_name.map(str::to_string),
                category_color: category_color.map(str::to_string),
                display_name: display_name.map(str::to_string),
                // 手动发送恒为非自动、无 origin（Spec 2 auto 路径由后续任务接线）
                auto: false,
                origin_node_id: None,
            };
            if tx.send(msg).await.is_err() {
                eprintln!("[lan-sync] 发送到 {node_id} 失败：会话已关闭");
            }
        }
        Ok(())
    }

    /// 整组发送某分组：`BatchStart` → 逐条 `SendClip`（携带分组名/颜色 + 重命名）→
    /// `BatchEnd`。条目逐条流式装配发送（v4 的 build_send_payload 共用，不再
    /// 预装配整个分组）；多目标时逐目标 emit `DeviceCategorySent`，返回
    /// (组名, 至少送达 1 个目标的条数, 其余计数)。
    pub(crate) async fn send_category(
        &self,
        target: Option<&str>,
        category_id: &str,
    ) -> Result<(String, u32, u32), String> {
        let online = self.online_targets(target)?;
        let conn = self.inner.store.connect()?;
        let category = self.inner.store.get_category_with_conn(&conn, category_id)?;
        let items = self
            .inner
            .store
            .list_category_items_for_category_with_conn(&conn, category_id)?;
        drop(conn);
        if items.is_empty() {
            return Err("该分组没有可发送的条目".to_string());
        }
        let item_count = items.len().min(u32::MAX as usize) as u32;
        let category_name = category.name.clone();
        let category_color = category.color.clone();
        // 逐条流式发送（不预装配全部 payload）：单条构建失败（如图片文件缺失）
        // 跳过并计数、不中断整组（v4 行为）。条目外层 / 目标内层——同一时刻
        // 内存只持一条 payload（外加各目标通道在途副本），避免 10k 条 × ~8MB
        // 的整组放大；每目标通道内的消息顺序仍是 BatchStart → 条目 → BatchEnd。
        let mut targets: Vec<CategorySendTarget> = Vec::with_capacity(online.len());
        for (node_id, tx) in online {
            let started = tx
                .send(ControlMsg::BatchStart {
                    category_name: category_name.clone(),
                    category_color: Some(category_color.clone()),
                    item_count,
                })
                .await
                .is_ok();
            // BatchStart 都送不达（会话已死）的目标：跳过且不发汇总事件（v4 行为）
            targets.push(CategorySendTarget {
                node_id,
                tx,
                started,
                dead: !started,
                sent: 0,
                failed: 0,
            });
        }
        let mut build_failed: u32 = 0;
        let mut delivered_any_count: u32 = 0; // 至少送达 1 个目标的条数（跨目标聚合）
        for item in &items {
            let payload = match build_send_payload(&item.clip_type, &item.text) {
                Ok(payload) => payload,
                Err(reason) => {
                    eprintln!("[lan-sync] 整组发送跳过条目 {}：{reason}", item.id);
                    build_failed += 1;
                    for target in &mut targets {
                        target.failed += 1; // 构建失败对所有目标计失败
                    }
                    continue;
                }
            };
            let mut delivered_this = false;
            for target in &mut targets {
                if target.dead {
                    target.failed += 1; // 会话已断：该目标剩余条目逐条计失败
                    continue;
                }
                let msg = ControlMsg::SendClip {
                    clip_type: item.clip_type.clone(),
                    payload: payload.clone(),
                    category_name: Some(category_name.clone()),
                    category_color: Some(category_color.clone()),
                    display_name: item.display_name.clone(),
                    // 整组发送为手动操作：非自动、无 origin
                    auto: false,
                    origin_node_id: None,
                };
                if target.tx.send(msg).await.is_ok() {
                    target.sent += 1;
                    delivered_this = true;
                } else {
                    // 通道关闭（会话已断）：该目标剩余条目必然失败，停发
                    target.dead = true;
                    target.failed += 1;
                }
            }
            if delivered_this {
                delivered_any_count += 1;
            }
        }
        for target in &mut targets {
            if !target.started {
                continue;
            }
            let _ = target.tx.send(ControlMsg::BatchEnd).await;
            self.emit(
                EVENT_DEVICE_CATEGORY_SENT,
                &DeviceCategorySent {
                    node_id: target.node_id.clone(),
                    category_name: category_name.clone(),
                    sent: target.sent,
                    failed: target.failed,
                },
            );
        }
        let sent = delivered_any_count;
        let failed = item_count.saturating_sub(sent);
        Ok((category_name, sent, failed))
    }

    /// 请求指定设备回推它当前的剪贴板内容。
    pub(crate) async fn request_clip(&self, node_id: &str) -> Result<(), String> {
        let targets = self.online_targets(Some(node_id))?;
        for (_, tx) in targets {
            let _ = tx.send(ControlMsg::RequestClip).await;
        }
        Ok(())
    }

    // —— 捕获即扇出（Spec 2 发送侧）——

    /// 捕获即扇出（spec §1）：master 开关 → recent 命中跳过（回环第一道）→
    /// payload 构建（超限跳过）→ 在线目标两段式过滤 → try_send（队列满丢弃
    /// 计数，绝不阻塞捕获）。Err 仅在 payload 构建失败时返回；其余抑制路径
    /// 一律静默 Ok（同步不得拖垮捕获路径）。
    pub(crate) async fn fan_out_auto(&self, clip: &ClipItem) -> Result<(), String> {
        // 闸门 1：master 总开关关闭——短路在 payload 构建之前（不读图片文件）。
        let settings = self.inner.store.auto_push_settings()?;
        if !settings.master {
            return Ok(());
        }
        // 闸门 2：recent 命中（回环窗口内刚从对端收到的内容）——同样短路在
        // payload 构建之前，防回推的同时省掉图片读盘。
        if self.inner.recent.contains(&clip.content_hash) {
            return Ok(());
        }
        // payload 规则与手动 send_raw 一致：image = 文件读出转 data url
        // （clip.text 为落盘路径），其余 = text 原文。图片超限在 build_send_payload
        // 内被拒（Err）；文本类超出 LAN_MAX_PAYLOAD 则跳过本次扇出（非错误）。
        let payload = build_send_payload(&clip.clip_type, &clip.text)?;
        if payload.len() > LAN_MAX_PAYLOAD {
            eprintln!(
                "[auto-push] payload 超出单帧上限（{} 字节），跳过本次自动推送",
                payload.len()
            );
            return Ok(());
        }
        // 两段式锁纪律：第一段在 links 锁内只收集 Connected 链路的 (node, 控制通道)，
        // 仅 clone sender——锁内无 SQLite/IO（无 await 纪律 + 最小化锁持有）。
        let candidates: Vec<(String, mpsc::Sender<ControlMsg>)> = {
            let links = self.inner.links.lock().expect("links 锁中毒");
            links
                .iter()
                .filter(|(_, handle)| handle.status == DeviceOnline::Connected)
                .filter_map(|(node, handle)| {
                    handle.control_tx.as_ref().map(|tx| (node.clone(), tx.clone()))
                })
                .collect()
        };
        // 第二段在锁外查每设备偏好（同步 SQLite），过滤交给纯函数 fan_out_targets。
        // store 无行（如设备刚被删除）按 TextOnly 兜底。
        let modes: Vec<(String, AutoSyncMode)> = candidates
            .iter()
            .map(|(node, _)| {
                let mode = self
                    .inner
                    .store
                    .get_paired_device(node)
                    .ok()
                    .flatten()
                    .map(|device| device.auto_sync_mode)
                    .unwrap_or(AutoSyncMode::TextOnly);
                (node.clone(), mode)
            })
            .collect();
        let allowed: HashSet<String> =
            fan_out_targets(&modes, &clip.clip_type).into_iter().collect();
        let my_id = hex_encode_32(self.inner.endpoint.id().as_bytes());
        for (node, tx) in candidates {
            if !allowed.contains(&node) {
                continue; // 该设备偏好不接收此类型：不发
            }
            let msg = ControlMsg::SendClip {
                clip_type: clip.clip_type.clone(),
                payload: payload.clone(),
                // 捕获路径无分组/重命名语义（brief 约定）
                category_name: None,
                category_color: None,
                display_name: None,
                auto: true,
                origin_node_id: Some(my_id.clone()),
            };
            // 队列满/会话已死：try_send_auto 内已计数并记日志，不重试、不等待。
            let _ = try_send_auto(&tx, msg, &self.inner.auto_dropped);
        }
        Ok(())
    }

    // —— 设备管理 ——

    /// store 行 + links 状态合成；撤销的恒 Offline（即使有残留登记）。
    pub(crate) fn device_infos(&self) -> Vec<DeviceInfo> {
        let devices = self.inner.store.list_paired_devices().unwrap_or_else(|reason| {
            eprintln!("[lan-sync] 读取设备列表失败：{reason}");
            Vec::new()
        });
        if devices.is_empty() {
            return Vec::new();
        }
        let links = self.inner.links.lock().expect("links 锁中毒");
        devices
            .into_iter()
            .map(|device| {
                let online = if device.revoked_at.is_some() {
                    DeviceOnline::Offline
                } else {
                    links
                        .get(&device.node_id)
                        .map(|handle| handle.status)
                        .unwrap_or(DeviceOnline::Offline)
                };
                DeviceInfo { device, online }
            })
            .collect()
    }

    /// 断开某设备的链路：登记移除（control_tx drop → 会话发 Disconnect 帧干净退出）
    /// + abort 重拨任务。
    fn kill_link(&self, node_id: &str) {
        let removed = {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            links.remove(node_id)
        };
        if let Some(handle) = removed {
            if let Some(task) = &handle.task {
                task.abort();
            }
        }
    }

    /// 撤销信任（软删）：store 撤销 + 断链 + emit 列表。
    pub(crate) fn revoke(&self, node_id: &str) {
        if let Err(reason) = self.inner.store.revoke_device(node_id) {
            eprintln!("[lan-sync] 撤销设备失败：{reason}");
        }
        self.kill_link(node_id);
        self.clear_disconnected(node_id); // 撤销行本身即拒绝入站，标记无意义
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 彻底删除记录：此后该设备拨号等同陌生设备。
    pub(crate) fn delete_device(&self, node_id: &str) {
        self.kill_link(node_id);
        self.clear_disconnected(node_id);
        if let Err(reason) = self.inner.store.delete_device(node_id) {
            eprintln!("[lan-sync] 删除设备失败：{reason}");
        }
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 仅改同步偏好（不断链）+ emit 列表。
    pub(crate) fn set_auto_sync(&self, node_id: &str, mode: AutoSyncMode) {
        if let Err(reason) = self.inner.store.set_auto_sync_mode(node_id, mode) {
            eprintln!("[lan-sync] 设置同步偏好失败：{reason}");
        }
        self.emit_device_list();
    }

    /// 用户主动断开某设备：杀会话 + 停重拨 + 记入内存态断开标记——此后对端
    /// 重拨一律静默拒绝，直到重新配对成功或重启应用（重启清空标记）。
    pub(crate) fn disconnect(&self, node_id: &str) {
        self.kill_link(node_id);
        self.inner
            .disconnected
            .lock()
            .expect("disconnected 锁中毒")
            .insert(node_id.to_string());
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 停止入站接受循环并断开全部链路。Endpoint 本体随最后的 Arc 引用释放关闭
    ///（其 close() 是异步的，留给 Task 8 的 lib 接线决定是否显式等待）。
    pub(crate) fn shutdown(&self) {
        if let Some(task) = self.inner.accept_task.lock().expect("accept_task 锁中毒").take() {
            task.abort();
        }
        let handles: Vec<LinkHandle> = {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            links.drain().map(|(_, handle)| handle).collect()
        };
        // control_tx 随 handle drop → 各会话收到 None 干净关闭；任务 abort 停止重拨
        for handle in handles {
            if let Some(task) = handle.task {
                task.abort();
            }
        }
        self.emit_device_list();
    }
}

/// 测试辅助（集成测试消费）：本端 EndpointId 的 64 字符 hex（即对端眼中的
/// node_id）。避免测试直接触私有字段。
#[cfg(test)]
impl DeviceLinkRegistry {
    pub(crate) fn inner_endpoint_id_hex_for_test(&self) -> String {
        hex_encode_32(self.inner.endpoint.id().as_bytes())
    }
}

/// 拨号方在会话流上的首发帧。iroh 的流语义：仅 `open_bi` 不足以让对端的
/// `accept_bi` 解除挂起——流上必须先有数据（iroh `Connection` 文档：「Data must
/// be sent on a stream before the respective accept call at the peer will yield
/// a RecvStream」）。拨号方开流后立即写一帧 Ping，接受方会话循环读到即回 Pong；
/// 否则配对/重拨的会话建立要空等 30s 心跳才完成。两处拨号路径（join 的会话流、
/// dial 的重拨流）共用。
async fn send_stream_opener(send: &mut SendStream) -> Result<(), String> {
    let mut writer = FrameWriter::new(send);
    writer
        .write_message(&LanMessage::Ping, None)
        .await
        .map_err(|e| e.to_string())
}

/// 配对流上回一帧 PairReject（尽力而为，失败即对端已断开）。
async fn reply_reject(send: &mut SendStream, reason: PairRejectReason) {
    let mut writer = FrameWriter::new(send);
    let _ = writer
        .write_message(&LanMessage::PairReject { reason }, None)
        .await;
}

/// 图片条目可发送的最大原始文件字节数：data url 前缀 + base64 放大（4/3）后
/// 不得超过 `LAN_MAX_PAYLOAD`，否则对端会在帧解析时拒收。
/// 命令层（commands.rs 的单条发送装配）与整组发送共用。
pub(crate) fn max_sendable_image_bytes() -> u64 {
    let expanded = (LAN_MAX_PAYLOAD - "data:image/png;base64,".len()) as u64;
    expanded / 4 * 3
}

/// auto 推送的投递原语：try_send 入队，队列满或会话已死时经 `dropped` 计数
/// 后丢弃并记日志——同步等待容量，是「扇出绝不阻塞捕获」的核心保证
///（fan_out_auto 消费）。返回 true = 已入队。
fn try_send_auto(
    tx: &mpsc::Sender<ControlMsg>,
    msg: ControlMsg,
    dropped: &AtomicU64,
) -> bool {
    match tx.try_send(msg) {
        Ok(()) => true,
        Err(_) => {
            let count = dropped.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("[auto-push] 目标队列满或会话已关闭，丢弃第 {count} 条自动推送");
            false
        }
    }
}

/// 把待发送的条目内容编码成同步 payload 字节（v4 lan_send_clip/lan_send_category
/// 原样迁移；命令层单条发送与整组发送共用）。
///
/// - 文本类条目：`text` 即原文，直接转 UTF-8 字节。
/// - 图片类条目：DB 里 `text` 存的是本地文件路径，读回字节并编码成自包含的
///   `data:image/png;base64,...`（对端机器上不存在该文件），接收侧
///   `captured_item_from_payload` 能解码。
pub(crate) fn build_send_payload(clip_type: &str, text: &str) -> Result<Vec<u8>, String> {
    if clip_type == "image" {
        // 读文件前先查大小：超限文件编码后必被对端拒收，整文件读入只浪费内存
        let file_len = std::fs::metadata(text)
            .map_err(|e| format!("读取图片文件失败：{e}"))?
            .len();
        if file_len > max_sendable_image_bytes() {
            return Err(format!("图片文件过大（{file_len} 字节），超出同步单帧上限"));
        }
        let bytes = std::fs::read(text).map_err(|e| format!("读取图片文件失败：{e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(format!("data:image/png;base64,{b64}").into_bytes())
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::temp_store;
    use serde_json::Value;

    /// 记录型事件出口：捕获 (事件名, payload) 供断言。
    #[derive(Clone, Default)]
    struct RecordingSink(Arc<std::sync::Mutex<Vec<(String, Value)>>>);

    impl LanEventSink for RecordingSink {
        fn emit(&self, event: &str, payload: &Value) {
            self.0.lock().unwrap().push((event.to_string(), payload.clone()));
        }
    }

    fn recorded(sink: &RecordingSink) -> Vec<(String, Value)> {
        sink.0.lock().unwrap().clone()
    }

    fn find_events(sink: &RecordingSink, event: &str) -> Vec<Value> {
        recorded(sink)
            .into_iter()
            .filter(|(name, _)| name == event)
            .map(|(_, payload)| payload)
            .collect()
    }

    fn hex32(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    async fn test_registry() -> (Arc<DeviceLinkRegistry>, RecordingSink) {
        let sink = RecordingSink::default();
        let registry =
            DeviceLinkRegistry::start_for_test(temp_store(), Arc::new(sink.clone()))
                .await
                .expect("test registry");
        (registry, sink)
    }

    /// 在 links 里伪造一条 Connected 登记（持有真实控制通道），返回接收端。
    fn fake_connected_link(
        registry: &DeviceLinkRegistry,
        node_id: &str,
    ) -> mpsc::Receiver<ControlMsg> {
        let (tx, rx) = mpsc::channel(16);
        registry
            .inner
            .links
            .lock()
            .unwrap()
            .insert(
                node_id.to_string(),
                LinkHandle {
                    gen: registry.next_gen(),
                    control_tx: Some(tx),
                    status: DeviceOnline::Connected,
                    task: None,
                },
            );
        rx
    }

    /// 退避序列：5,10,20,40,80,160，之后恒为 300。
    #[test]
    fn reconnect_backoff_progression() {
        assert_eq!(
            (0..6).map(reconnect_backoff).collect::<Vec<_>>(),
            vec![
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(20),
                Duration::from_secs(40),
                Duration::from_secs(80),
                Duration::from_secs(160),
            ]
        );
        assert_eq!(reconnect_backoff(6), Duration::from_secs(300));
        assert_eq!(reconnect_backoff(100), Duration::from_secs(300));
    }

    /// build_send_payload：文本条目直接转 UTF-8 字节。
    #[test]
    fn build_send_payload_text_returns_utf8_bytes() {
        let payload = build_send_payload("text", "hello-api-key").unwrap();
        assert_eq!(payload, b"hello-api-key");
    }

    /// build_send_payload：图片条目读回文件并编码成自包含 data url。
    #[test]
    fn build_send_payload_image_reads_file_and_encodes_data_url() {
        use crate::clipboard::image_bytes_from_data_url;

        // 建一个临时 png 文件模拟 DB 里图片条目的 text（文件路径）。
        let dir = std::env::temp_dir().join(format!("ipaste-send-payload-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("img.png");
        // 1x1 透明 png 的最小字节
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // png signature
        ];
        std::fs::write(&png_path, png_bytes).unwrap();

        let payload = build_send_payload("image", png_path.to_str().unwrap()).unwrap();
        let text = String::from_utf8(payload).unwrap();
        assert!(
            text.starts_with("data:image/png;base64,"),
            "图片 payload 应是 data url，实际：{text}"
        );
        // 解码回来的字节与原文件一致
        let decoded = image_bytes_from_data_url(&text).unwrap();
        assert_eq!(decoded, png_bytes);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// build_send_payload：文件缺失返回可读错误，而非 panic 或发空 payload。
    #[test]
    fn build_send_payload_image_missing_file_errors() {
        let result = build_send_payload("image", "/definitely/not/here/xyz.png");
        assert!(result.is_err());
    }

    /// build_send_payload：超限图片在读文件前拒绝，避免无意义的整文件读入
    /// 与注定失败的传输。
    #[test]
    fn build_send_payload_rejects_image_exceeding_frame_limit() {
        let dir = std::env::temp_dir().join(format!("ipaste-send-limit-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big_path = dir.join("big.png");
        std::fs::write(&big_path, vec![0u8; LAN_MAX_PAYLOAD]).unwrap();

        let error = build_send_payload("image", big_path.to_str().unwrap())
            .expect_err("超限图片应被拒绝");
        assert!(error.contains("过大"), "got: {error}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// build_send_payload：恰好编码后不超上限的文件可正常构建 payload。
    #[test]
    fn build_send_payload_allows_image_at_frame_limit() {
        let dir = std::env::temp_dir().join(format!("ipaste-send-bound-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edge.png");
        std::fs::write(&path, vec![0u8; max_sendable_image_bytes() as usize]).unwrap();

        let payload = build_send_payload("image", path.to_str().unwrap()).unwrap();
        assert!(payload.len() <= LAN_MAX_PAYLOAD);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// create_invite：票据可解码回本端 EndpointId，emit 的 payload 带 ticket 与
    /// 未来时间戳；cancel_invite 后 emit {None, None}。
    #[tokio::test]
    async fn create_and_cancel_invite_emit_state() {
        let (registry, sink) = test_registry().await;
        let ticket = registry.create_invite().await.expect("invite");
        assert!(ticket.starts_with("ipaste-pair-v1:"));
        let decoded = PairTicket::decode(&ticket).expect("decodable ticket");
        assert_eq!(decoded.endpoint_id, *registry.inner.endpoint.id().as_bytes());
        // RelayMode::Disabled（relay_disabled=true 跳过 online 等待）：票据无中继，
        // 只有直连地址——即 online() 超时兜底的 LAN-only 形态，必须依然可解析。
        assert_eq!(decoded.relay_url, None);
        assert!(!decoded.direct_addrs.is_empty(), "本端直连地址应进票据");

        let events = find_events(&sink, EVENT_PAIR_INVITE_STATE);
        assert_eq!(events.len(), 1, "create_invite 应 emit 一次邀请状态");
        assert_eq!(events[0]["ticket"].as_str(), Some(ticket.as_str()));
        let expires_at = events[0]["expiresAt"].as_u64().expect("expiresAt 为数字");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(
            expires_at > now_ms && expires_at <= now_ms + INVITE_TTL.as_millis() as u64 + 5_000,
            "expires_at 应在未来 10 分钟内"
        );

        registry.cancel_invite().expect("cancel");
        let events = find_events(&sink, EVENT_PAIR_INVITE_STATE);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["ticket"].as_str(), None);
        assert_eq!(events[1]["expiresAt"].as_u64(), None);
    }

    /// 无 pending 配对请求时 respond_pair 报错。
    #[tokio::test]
    async fn respond_pair_without_pending_errors() {
        let (registry, _sink) = test_registry().await;
        let err = registry.respond_pair(true).expect_err("无 pending 应报错");
        assert!(err.contains("当前没有待确认的配对请求"), "实际错误：{err}");
    }

    /// respond_pair：take 语义——决定送达 oneshot，第二次调用报错。
    #[tokio::test]
    async fn respond_pair_resolves_pending_once() {
        let (registry, _sink) = test_registry().await;
        let (decision_tx, decision_rx) = oneshot::channel();
        *registry.inner.pending_pair.lock().unwrap() = Some(PendingPair {
            device_name: "MBP".into(),
            node_id: hex32(1),
            decision_tx,
        });
        registry.respond_pair(true).expect("第一次应成功");
        assert_eq!(decision_rx.await, Ok(true));
        assert!(registry.respond_pair(true).is_err(), "pending 已消费，第二次报错");
    }

    /// device_infos：store 行 + links 状态合成；撤销恒 Offline。
    #[tokio::test]
    async fn device_infos_merges_link_status() {
        let (registry, _sink) = test_registry().await;
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(1), "MBP", None, &[])
            .unwrap();
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(2), "PC", None, &[])
            .unwrap();
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(3), "Old", None, &[])
            .unwrap();
        registry.inner.store.revoke_device(&hex32(3)).unwrap();

        let _rx = fake_connected_link(&registry, &hex32(1));
        let infos = registry.device_infos();
        assert_eq!(infos.len(), 3);
        let by_node: std::collections::HashMap<&str, DeviceOnline> = infos
            .iter()
            .map(|info| (info.device.node_id.as_str(), info.online))
            .collect();
        assert_eq!(by_node[hex32(1).as_str()], DeviceOnline::Connected, "有活跃登记 → Connected");
        assert_eq!(by_node[hex32(2).as_str()], DeviceOnline::Offline, "无登记 → Offline");
        assert_eq!(by_node[hex32(3).as_str()], DeviceOnline::Offline, "撤销恒 Offline");
    }

    /// send_raw：无在线目标（None / 指定不存在）都报「没有在线的目标设备」。
    #[tokio::test]
    async fn send_raw_without_online_target_errors() {
        let (registry, _sink) = test_registry().await;
        let err = registry
            .send_raw(None, "text", b"hi", None, None, None)
            .await
            .expect_err("无在线目标应报错");
        assert_eq!(err, "没有在线的目标设备");
        let err = registry
            .send_raw(Some(&hex32(9)), "text", b"hi", None, None, None)
            .await
            .expect_err("指定设备不在线应报错");
        assert_eq!(err, "没有在线的目标设备");
    }

    /// send_category：空分组报「该分组没有可发送的条目」。
    #[tokio::test]
    async fn send_category_empty_category_errors() {
        let (registry, _sink) = test_registry().await;
        let category = registry
            .inner
            .store
            .create_category("空分组".into(), "#9CA3AF".into())
            .unwrap();
        let _rx = fake_connected_link(&registry, &hex32(1));
        let err = registry
            .send_category(Some(&hex32(1)), &category.id)
            .await
            .expect_err("空分组应报错");
        assert_eq!(err, "该分组没有可发送的条目");
    }

    /// send_category：装配 + 分发 + 汇总事件（用伪造 link 通道验证控制指令序列）。
    #[tokio::test]
    async fn send_category_dispatches_batch_messages() {
        let (registry, sink) = test_registry().await;
        let store = &registry.inner.store;
        store
            .insert_received_category_item(
                "text".into(),
                "hash-1".into(),
                "preview".into(),
                "第一条".into(),
                "工作".into(),
                Some("#0D9488".into()),
                None,
                None,
            )
            .unwrap();
        store
            .insert_received_category_item(
                "text".into(),
                "hash-2".into(),
                "preview".into(),
                "第二条".into(),
                "工作".into(),
                Some("#0D9488".into()),
                Some("改名".into()),
                None,
            )
            .unwrap();
        // insert_received_category_item 按名称建分组；查回分组 id
        let conn = store.connect().unwrap();
        let cat_id: String = conn
            .query_row(
                "SELECT id FROM categories WHERE name = '工作'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);

        let mut rx = fake_connected_link(&registry, &hex32(1));
        let (name, sent, failed) = registry
            .send_category(Some(&hex32(1)), &cat_id)
            .await
            .expect("整组发送成功");
        assert_eq!(name, "工作");
        assert_eq!(sent, 2, "两条全部送达");
        assert_eq!(failed, 0);

        // 控制指令序列：BatchStart → SendClip × 2 → BatchEnd。
        // 两条 SendClip 的相对顺序由分组内 sort_order 决定（后插入的在最上），
        // 断言按内容集合而非固定顺序。
        let msgs: Vec<ControlMsg> = {
            let mut collected = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                collected.push(msg);
            }
            collected
        };
        assert_eq!(msgs.len(), 4, "BatchStart + 2×SendClip + BatchEnd");
        assert!(matches!(&msgs[0], ControlMsg::BatchStart { category_name, item_count, .. }
            if category_name == "工作" && *item_count == 2));
        let sent_payloads: Vec<(Vec<u8>, Option<String>)> = msgs[1..3]
            .iter()
            .filter_map(|msg| match msg {
                ControlMsg::SendClip { payload, display_name, category_name, category_color, .. } => {
                    assert_eq!(category_name.as_deref(), Some("工作"));
                    assert_eq!(category_color.as_deref(), Some("#0D9488"));
                    Some((payload.clone(), display_name.clone()))
                }
                _ => None,
            })
            .collect();
        let as_text = |entry: &(Vec<u8>, Option<String>)| {
            (String::from_utf8_lossy(&entry.0).to_string(), entry.1.clone())
        };
        let mut got: Vec<(String, Option<String>)> = sent_payloads.iter().map(as_text).collect();
        got.sort();
        let mut want = vec![
            ("第一条".to_string(), None),
            ("第二条".to_string(), Some("改名".to_string())),
        ];
        want.sort();
        assert_eq!(got, want);
        assert!(matches!(msgs[3], ControlMsg::BatchEnd));

        // 汇总事件：DeviceCategorySent 携带 node_id / 组名 / 计数
        let events = find_events(&sink, EVENT_DEVICE_CATEGORY_SENT);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["nodeId"].as_str(), Some(hex32(1).as_str()));
        assert_eq!(events[0]["categoryName"].as_str(), Some("工作"));
        assert_eq!(events[0]["sent"].as_u64(), Some(2));
        assert_eq!(events[0]["failed"].as_u64(), Some(0));
    }

    /// hex 往返：endpoint_id_from_hex(hex_encode_32(id)) == id；非法输入 None。
    /// 注意 EndpointId::from_bytes 会校验 ed25519 曲线点，全 0x5a 之类非法字节会
    /// 被拒——用真实生成的密钥测往返。
    #[test]
    fn endpoint_id_hex_roundtrip() {
        let id = SecretKey::generate().public();
        assert_eq!(
            endpoint_id_from_hex(&hex_encode_32(id.as_bytes())).map(|p| *p.as_bytes()),
            Some(*id.as_bytes())
        );
        assert!(endpoint_id_from_hex("zz").is_none());
        assert!(endpoint_id_from_hex(&"ab".repeat(31)).is_none());
        // 非法曲线点（全 0x5a）在 from_bytes 层被拒 → None
        assert!(endpoint_id_from_hex(&hex_encode_32(&[0x5a; 32])).is_none());
    }

    /// start_for_test 端点可建可用：EndpointId 是 32 字节公钥的稳定封装。
    #[tokio::test]
    async fn test_registry_binds_endpoint() {
        let (registry, _sink) = test_registry().await;
        let id = registry.inner.endpoint.id();
        assert_eq!(id.as_bytes().len(), 32);
        assert_eq!(hex_encode_32(id.as_bytes()).len(), 64);
        // shutdown 幂等且不 panic
        registry.shutdown();
        registry.shutdown();
    }

    /// 互踢震荡修复（Fix 1）：入站会话在线（Connected + 活跃控制通道）时，
    /// link_task 不得拨号——拨号路径的第一步就是 set_status(Connecting) 事件，
    /// 若未跳过会立刻观察到状态事件并随后 Offline（拨号指向不可达地址必失败）。
    #[tokio::test]
    async fn link_task_skips_dial_when_inbound_session_live() {
        let (registry, sink) = test_registry().await;
        let node = hex32(1);
        // 地址指向 TEST-NET（不可达）：若 link_task 误拨号，Connecting → 15s 内 Offline
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &["203.0.113.1:1".into()])
            .unwrap();
        let (tx, _rx) = mpsc::channel(16);
        registry.inner.links.lock().unwrap().insert(
            node.clone(),
            LinkHandle {
                gen: registry.next_gen(),
                control_tx: Some(tx),
                status: DeviceOnline::Connected,
                task: None,
            },
        );
        let task = tokio::spawn(registry.clone().link_task(node.clone()));
        // 留足调度余量：未跳过时 Connecting 事件在 spawn 后毫秒级出现
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert!(
            statuses.is_empty(),
            "入站会话在线时 link_task 不应拨号（不应出现任何状态事件）：{statuses:?}"
        );
        assert_eq!(
            registry.device_infos()[0].online,
            DeviceOnline::Connected,
            "在线登记不被 link_task 覆写"
        );
        // 会话结束后（drop 控制通道）跳过条件解除——此处仅验证跳过路径本身可退出
        task.abort();
    }

    /// 互踢震荡修复（Fix Round 2）：真实的 run_session 被入站会话收编后结束时，
    /// 不得覆写存活会话的状态、不得发假日 Offline 事件、link_task 停靠不重拨。
    /// 这是对「对端重拨落地 → 本端会话死 → 本端 link_task 醒来」完整时序的复刻：
    /// 旧实现的 set_status(Offline) 在此刻把入站会话的 Connected 砸成 Offline，
    /// 进而在循环顶骗过 guard 引发重拨互踢。
    #[tokio::test]
    async fn superseded_session_end_keeps_inbound_session_and_parks_link_task() {
        use tokio::io::duplex;
        let (registry, sink) = test_registry().await;
        let node = hex32(3);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &["203.0.113.1:1".into()])
            .unwrap();

        // 1) 真实 run_session 在 duplex 流上运行（link_task 的会话入口，非伪造登记）
        let (client, server) = duplex(4096);
        let (read_half, write_half) = tokio::io::split(server);
        let (_dead_tx, dead_rx) = oneshot::channel();
        let session_registry = registry.clone();
        let session_node = node.clone();
        let session = tokio::spawn(async move {
            session_registry
                .run_session(session_node, read_half, write_half, dead_rx)
                .await
        });
        let mut polls = 0;
        let my_gen = loop {
            let registered = {
                let links = registry.inner.links.lock().unwrap();
                links.get(&node).and_then(|handle| {
                    handle.control_tx.is_some().then_some(handle.gen)
                })
            };
            if let Some(gen) = registered {
                break gen;
            }
            polls += 1;
            assert!(polls < 100, "会话应在 2s 内注册");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(registry.device_infos()[0].online, DeviceOnline::Connected);

        // 2) 对端重拨落地：入站会话收编登记（新 gen + 活跃 control_tx + Connected）
        let (inbound_tx, _inbound_rx) = mpsc::channel(16);
        let inbound_gen = registry.next_gen();
        {
            let mut links = registry.inner.links.lock().unwrap();
            let handle = links.get_mut(&node).expect("登记存在");
            handle.gen = inbound_gen;
            handle.control_tx = Some(inbound_tx);
            handle.status = DeviceOnline::Connected;
        }

        // 3) 旧会话结束（收编时其唯一 sender 被 drop → 会话循环收到 None 退出；EOF 兜底）
        drop(client);
        let ended_gen = session.await.unwrap();
        assert_eq!(ended_gen, my_gen, "run_session 应返回本会话的 gen");

        // 4) 登记原封不动：仍是入站会话的 gen + 活跃通道 + Connected
        {
            let links = registry.inner.links.lock().unwrap();
            let handle = links.get(&node).expect("登记未被移除");
            assert_eq!(handle.gen, inbound_gen);
            assert!(handle.control_tx.is_some());
            assert_eq!(
                handle.status,
                DeviceOnline::Connected,
                "被收编的会话结束不得覆写存活会话的状态"
            );
        }

        // 5) link_task 会话后的 Offline 写（凭据 = 旧会话 gen）必须 no-op
        registry.set_status_if_owner(&node, ended_gen, DeviceOnline::Offline);
        assert_eq!(
            registry.inner.links.lock().unwrap().get(&node).unwrap().status,
            DeviceOnline::Connected
        );

        // 6) 无假日 Offline 事件：至此状态事件只有会话建立时的 Connected 一条
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert_eq!(statuses.len(), 1, "被收编会话的结束不应发 Offline 事件：{statuses:?}");
        assert_eq!(statuses[0]["status"].as_str(), Some("connected"));

        // 7) link_task 醒来后在 guard 停靠：不拨号、不再新增状态事件
        let task = tokio::spawn(registry.clone().link_task(node.clone()));
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert_eq!(statuses.len(), 1, "入站会话在线时 link_task 不得重拨：{statuses:?}");
        assert!(registry.has_live_session(&node));
        assert_eq!(registry.device_infos()[0].online, DeviceOnline::Connected);
        task.abort();
    }

    /// 持久断开（Fix 2）：disconnect() 后入站门关闭，重新配对/撤销/删除解除。
    /// 真实入站连接的拒绝行为由 handle_inbound 的同一道门（inbound_allowed）
    /// 保证，Task 9 的双端集成测试覆盖。
    #[tokio::test]
    async fn disconnect_blocks_inbound_until_repair() {
        let (registry, _sink) = test_registry().await;
        let node = hex32(2);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        assert!(registry.inbound_allowed(&node), "已配对未断开：允许入站会话");

        registry.disconnect(&node);
        assert!(
            !registry.inbound_allowed(&node),
            "显式断开后：已配对也静默拒绝入站会话"
        );
        assert!(registry.inner.disconnected.lock().unwrap().contains(&node));
        // store 行保留（断开 ≠ 撤销），设备列表仍可见
        assert_eq!(registry.device_infos().len(), 1);

        // 重新配对成功（join/accept 路径的清除点）→ 恢复
        registry.clear_disconnected(&node);
        assert!(registry.inbound_allowed(&node), "重新配对后恢复入站");

        // 撤销/删除也清标记（tidiness：撤销行本身即拒绝入站）
        registry.disconnect(&node);
        registry.revoke(&node);
        assert!(!registry.inner.disconnected.lock().unwrap().contains(&node));
    }

    /// 撤销门 helper（F2）：只有「行存在且 revoked_at 非空」才拦；
    /// 无记录/未撤销/已删除都不拦（删除后重新视为陌生设备）。
    #[tokio::test]
    async fn locally_revoked_guard_distinguishes_states() {
        let (registry, _sink) = test_registry().await;
        let node = hex32(5);
        assert!(!registry.is_locally_revoked(&node), "无记录：不拦（走陌生配对流程）");
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        assert!(!registry.is_locally_revoked(&node), "已配对未撤销：不拦（走已配对分支）");
        registry.inner.store.revoke_device(&node).unwrap();
        assert!(registry.is_locally_revoked(&node), "已撤销：静默拒绝");
        registry.inner.store.delete_device(&node).unwrap();
        assert!(!registry.is_locally_revoked(&node), "已删除：重新视为陌生设备");
    }

    /// join 前置撤销门（F2）：票据指向本地已撤销设备 → 不拨号直接失败，
    /// 错误提示先删除记录；Err 与 EVENT_PAIR_JOIN_FAILED 双通道（与其他
    /// join 失败一致）。
    #[tokio::test]
    async fn join_fails_fast_when_target_locally_revoked() {
        let (registry, sink) = test_registry().await;
        let node = hex32(6);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "旧设备", None, &[])
            .unwrap();
        registry.inner.store.revoke_device(&node).unwrap();
        // 票据 endpoint_id 只需 hex 对得上 store 行（不拨号，无需合法曲线点）
        let bytes = [0x06u8; 32];
        assert_eq!(hex_encode_32(&bytes), node);
        let ticket = PairTicket {
            version: 1,
            endpoint_id: bytes,
            relay_url: None,
            direct_addrs: vec![],
            invite_secret: [0u8; 16],
        };
        let err = registry
            .join(&ticket.encode())
            .await
            .expect_err("已撤销目标应立即失败");
        assert!(err.contains("已被撤销"), "实际错误：{err}");
        let events = find_events(&sink, EVENT_PAIR_JOIN_FAILED);
        assert_eq!(events.len(), 1, "失败应 emit join-failed 事件");
        assert!(events[0]["reason"].as_str().unwrap().contains("已被撤销"));
    }

    /// join 应答等待超时（F5）：对端校验通过但停在用户确认（无人应答）时，
    /// join 不得无限挂起——JOIN_REPLY_TIMEOUT（测试构建 2s）后返回
    /// 「等待对方响应超时」并 emit join-failed；host 侧的 120s 用户确认
    /// 限时远晚于 join 超时，保证先在拨号方触发。
    #[tokio::test]
    async fn join_reply_wait_times_out() {
        let (host, host_sink) = test_registry().await;
        let (joiner, sink) = test_registry().await;
        // host 生成有效邀请：对端 PairRequest 校验通过 → 走到用户确认等待
        //（连接保持打开、不回帧），拨号方只能靠自己的 30s 超时收场
        let secret = host.inner.invites.lock().unwrap().create();
        let addr = host.inner.endpoint.addr();
        let direct_addrs: Vec<String> = addr.ip_addrs().map(|a| a.to_string()).collect();
        assert!(!direct_addrs.is_empty(), "回环端点应有直连地址");
        let ticket = PairTicket {
            version: 1,
            endpoint_id: *host.inner.endpoint.id().as_bytes(),
            relay_url: None,
            direct_addrs,
            invite_secret: secret,
        };
        let err = joiner.join(&ticket.encode()).await.expect_err("对端不回帧时应超时失败");
        assert_eq!(err, "等待对方响应超时");
        let events = find_events(&sink, EVENT_PAIR_JOIN_FAILED);
        assert_eq!(events.len(), 1, "超时失败应 emit join-failed");
        assert_eq!(events[0]["reason"].as_str(), Some("等待对方响应超时"));
        // host 确实停在用户确认（而不是提前断开）：已弹出配对请求事件
        assert!(
            !find_events(&host_sink, EVENT_PAIR_REQUEST).is_empty(),
            "host 应已进入用户确认等待（连接保持打开）"
        );
    }

    /// try_send_auto：队列满时计数丢弃、不阻塞（容量 1 塞满后第二条失败 → 计数 +1）。
    #[test]
    fn try_send_auto_counts_drop_when_channel_full() {
        let (tx, _rx) = mpsc::channel(1);
        let dropped = AtomicU64::new(0);
        assert!(try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "首条应入队");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        // 容量已满：同步返回失败并计数，绝不等待容量（fire-and-forget 保证）
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "满队应丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "丢弃计数 +1");
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "继续满队仍丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 2, "计数持续累计");
    }

    /// try_send_auto：会话已死（接收端 drop）同样计数丢弃——与满队列同一处理。
    #[test]
    fn try_send_auto_counts_drop_when_session_closed() {
        let (tx, rx) = mpsc::channel(16);
        drop(rx);
        let dropped = AtomicU64::new(0);
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "死通道应丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "丢弃计数 +1");
    }

    /// 测试用 ClipItem 装配（fan_out_auto 的输入形态：text 字段为文本原文或
    /// 图片落盘路径——与捕获入库后的约定一致）。
    fn text_clip(clip_type: &str, hash: &str, text: &str) -> ClipItem {
        ClipItem {
            id: crate::util::new_id(),
            clip_type: clip_type.to_string(),
            content_hash: hash.to_string(),
            display_name: None,
            preview_text: text.chars().take(20).collect(),
            text: text.to_string(),
            source_app: None,
            last_captured_at: String::new(),
            favorite_count: 0,
            is_pinned: false,
        }
    }

    /// fan_out_auto 闸门：master 关 / recent 命中都在 payload 构建之前短路——
    /// 被抑制时即使图片路径不可读也不报错（build_send_payload 根本不会被调用）。
    #[tokio::test]
    async fn fan_out_auto_short_circuits_before_payload_build() {
        let (registry, _sink) = test_registry().await;
        let store = &registry.inner.store;
        // 图片条目的 text 指向不存在路径：若错误地先构建 payload 会返回 Err
        let bad_image = text_clip("image", "hash-bad", "/definitely/not/here/x.png");

        // master 关：静默 Ok，无投递
        store.update_auto_push_settings(false, false).unwrap();
        let mut rx = fake_connected_link(&registry, &hex32(1));
        registry
            .fan_out_auto(&bad_image)
            .await
            .expect("master 关应短路返回 Ok");
        assert!(rx.try_recv().is_err(), "master 关：无投递");

        // master 开 + recent 命中：同样短路在 payload 构建之前
        store.update_auto_push_settings(true, false).unwrap();
        registry.inner.recent.insert("hash-bad");
        registry
            .fan_out_auto(&bad_image)
            .await
            .expect("recent 命中应短路返回 Ok");
        assert!(rx.try_recv().is_err(), "recent 命中：无投递");
    }

    /// fan_out_auto 投递：在线目标按每设备偏好过滤；SendClip 携带 auto=true +
    /// origin（本端 node_id）；超限文本跳过扇出但不是错误；payload 构建失败是
    /// 唯一 Err 来源。
    #[tokio::test]
    async fn fan_out_auto_dispatches_with_per_device_filtering() {
        let (registry, _sink) = test_registry().await;
        let store = &registry.inner.store;
        store.update_auto_push_settings(true, false).unwrap();
        store.upsert_paired_device(&hex32(1), "MBP", None, &[]).unwrap();
        store.set_auto_sync_mode(&hex32(1), AutoSyncMode::All).unwrap();
        store.upsert_paired_device(&hex32(2), "PC", None, &[]).unwrap();
        store.set_auto_sync_mode(&hex32(2), AutoSyncMode::Off).unwrap();
        store.upsert_paired_device(&hex32(3), "Phone", None, &[]).unwrap(); // 默认 TextOnly
        let mut rx1 = fake_connected_link(&registry, &hex32(1));
        let mut rx2 = fake_connected_link(&registry, &hex32(2));
        let mut rx3 = fake_connected_link(&registry, &hex32(3));
        let mut rx4 = fake_connected_link(&registry, &hex32(9)); // store 无行：按 TextOnly 兜底

        let clip = text_clip("text", "hash-t1", "hello auto");
        registry.fan_out_auto(&clip).await.unwrap();

        let my_id = registry.inner_endpoint_id_hex_for_test();
        for (label, rx) in [("all", &mut rx1), ("text-only", &mut rx3), ("unknown", &mut rx4)] {
            match rx.try_recv() {
                Ok(ControlMsg::SendClip {
                    clip_type, payload, auto, origin_node_id, ..
                }) => {
                    assert_eq!(clip_type, "text");
                    assert_eq!(payload, b"hello auto");
                    assert!(auto, "{label}：应为 auto 推送");
                    assert_eq!(
                        origin_node_id.as_deref(),
                        Some(my_id.as_str()),
                        "{label}：origin 应为本端 node_id"
                    );
                }
                other => panic!("{label} 设备应收到 auto 推送：{other:?}"),
            }
        }
        assert!(rx2.try_recv().is_err(), "Off 偏好不得收到");

        // 图片条目：只有 All 设备收到；TextOnly / 未知（TextOnly 兜底）被过滤。
        // 用真实临时图片文件，避免 payload 构建失败干扰断言。
        let dir = std::env::temp_dir().join(format!("ipaste-fanout-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("img.png");
        std::fs::write(&png_path, [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        let image_clip = text_clip("image", "hash-img", png_path.to_str().unwrap());
        registry.fan_out_auto(&image_clip).await.unwrap();
        match rx1.try_recv() {
            Ok(ControlMsg::SendClip { payload, auto, .. }) => {
                assert!(auto, "图片 auto 推送");
                let text = String::from_utf8(payload).unwrap();
                assert!(
                    text.starts_with("data:image/png;base64,"),
                    "图片 payload 应为 data url：{text}"
                );
            }
            other => panic!("All 设备应收到图片 auto 推送：{other:?}"),
        }
        assert!(rx3.try_recv().is_err(), "TextOnly 不收图片");
        assert!(rx4.try_recv().is_err(), "未知设备（TextOnly 兜底）不收图片");
        std::fs::remove_dir_all(&dir).ok();

        // 超限文本（> LAN_MAX_PAYLOAD）：跳过整次扇出且返回 Ok
        let big = text_clip("text", "hash-big", &"x".repeat(LAN_MAX_PAYLOAD + 1));
        registry
            .fan_out_auto(&big)
            .await
            .expect("超限跳过不是错误");
        assert!(rx1.try_recv().is_err(), "超限：无投递");

        // payload 构建失败（图片文件缺失）是唯一 Err 来源
        let missing = text_clip("image", "hash-miss", "/definitely/not/here/y.png");
        assert!(
            registry.fan_out_auto(&missing).await.is_err(),
            "payload 构建失败应返回 Err"
        );
    }
}
