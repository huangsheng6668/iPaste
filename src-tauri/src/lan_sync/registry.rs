//! DeviceLinkRegistry：iroh Endpoint 生命周期 + 每设备连接管理（spec §5）。
//!
//! 职责：
//! - 持有 iroh `Endpoint`（协议 v5 的唯一传输），入站连接按**首帧**分流
//!   （`PairRequest` → 票据配对门（不看信任态，v0.9.2 A2）；其余 → 会话流，
//!   信任与「显式断开」门由 run_session 的登记前复验把关（v0.9.2 A1））。
//! - 已配对设备各一条后台重拨任务（link_task）：拨号 → 会话 → 断开 → 指数退避重拨。
//! - 邀请/加入配对（`create_invite`/`join`/`respond_pair`）。
//! - 按设备分发发送指令（`send_raw`/`send_category`/`request_clip`）与设备管理
//!   （`revoke`/`delete_device`/`set_auto_sync`/`disconnect`）。
//!
//! 模块拆分：本文件保留端点生命周期、入站接受循环（accept_loop/handle_inbound）、
//! 共享状态类型（Inner/LinkHandle）与跨域辅助（hex 编解码、payload 装配、
//! 拨号首发帧）；域逻辑在子模块——`pairing`（邀请/加入/用户确认）、`link`
//! （重拨任务与会话登记）、`sender`（发送分发与 auto 扇出）、`device_admin`
//! （设备管理）、`tests`（原内联测试整体外移）。
//!
//! 锁纪律：`links`/`invites`/`pending_pair`/`accept_task` 是 std `Mutex`，只在
//! 无 `.await` 的短临界区内持有；跨 await 的共享一律 clone 出来再操作。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use iroh::endpoint::presets;
use iroh::endpoint::{Connection, SendStream, VarInt};
use iroh::{Endpoint, EndpointId, RelayMode, SecretKey};
use tokio::sync::{mpsc, oneshot};

use crate::events::{
    DeviceListChanged, DeviceStatusChanged, PairRequested, EVENT_DEVICE_LIST_CHANGED,
    EVENT_DEVICE_STATUS_CHANGED, EVENT_PAIR_REQUEST,
};
use crate::lan_sync::autopush::RecentReceived;
use crate::lan_sync::frame::{read_message_with_raw, FrameWriter, PrefixedFrame};
use crate::lan_sync::pair_guard::PairGuard;
use crate::lan_sync::protocol::{
    LanMessage, PairRejectReason, IPASTE_ALPN, LAN_MAX_PAYLOAD, LAN_PROTOCOL_VERSION,
};
use crate::lan_sync::session::fingerprint_hex;
use crate::lan_sync::ticket::InviteRegistry;
use crate::lan_sync::{device_name, ControlMsg, LanEventSink};
use crate::models::{AppendCopyState, DeviceOnline};
use crate::store::Store;

mod device_admin;
mod link;
mod pairing;
mod sender;

#[cfg(test)]
mod tests;

/// 拨号/加入的超时：中继路径下 QUIC 握手可能较慢，给足 15s。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// 陌生连接预认证（accept_bi + 首帧 PairRequest）的限时：迟迟不发首帧的
/// 连接到期静默关闭，防 slow-loris 式无限挂住配对任务（任务堆积）。
const STRANGER_PREAUTH_TIMEOUT: Duration = Duration::from_secs(60);

/// host 侧等待用户确认配对的限时：超时按拒绝处理（回 PairReject{Declined}），
/// 防陌生人持有效票据把确认弹窗永久钉死；正常用户 120s 足够操作。
const PAIR_CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

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
    pending_pair: Mutex<Option<pairing::PendingPair>>,
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
    /// 追加复制会话状态（与 AppState/clipboard watcher 共享同一实例）：
    /// 会话活跃时 auto 接收跳过剪贴板写（session.rs 消费）。直接引用
    /// models::AppendCopyState——同 crate pub(crate)，无需更轻的共享标志。
    append_state: Arc<Mutex<AppendCopyState>>,
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
    /// 整组批量（BatchStart…BatchEnd）正在经该链路发送：send_category 在
    /// 入队 BatchStart 前置位、BatchEnd 入队后清除；fan_out_auto 在 links 锁内
    /// 据此跳过忙碌链路——auto 帧绝不插进批量中段（对端会把中段帧折叠进
    /// 打开的分组，污染用户分拣的分组内容）。
    batch_busy: bool,
    /// 该设备重拨任务（link_task）的句柄；配对/入站建立的会话为 `None`
    /// （会话结束后由 registry 重新起任务接管重拨）。abort(&self) 不需要所有权。
    task: Option<tokio::task::JoinHandle<()>>,
}

/// DeviceLinkRegistry：所有公开方法供 Task 8 命令层与 Task 9 测试消费。
pub struct DeviceLinkRegistry {
    inner: Arc<Inner>,
}

impl DeviceLinkRegistry {
    /// 生产入口：固定设备身份 + n0 预设（默认中继 + DNS 地址发现）+ 指定中继模式。
    /// `append_state` 与 AppState/clipboard watcher 共享（追加会话期间 auto 接收
    /// 跳过剪贴板写的判据，见 session.rs）。
    pub(crate) async fn start(
        secret: SecretKey,
        store: Store,
        sink: Arc<dyn LanEventSink>,
        relay: RelayMode,
        append_state: Arc<Mutex<AppendCopyState>>,
    ) -> Result<Arc<Self>, String> {
        let relay_disabled = matches!(relay, RelayMode::Disabled);
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret)
            .alpns(vec![IPASTE_ALPN.to_vec()])
            .relay_mode(relay)
            .bind()
            .await
            .map_err(|e| format!("无法启动同步端点：{e}"))?;
        Self::from_endpoint(endpoint, store, sink, relay_disabled, append_state).await
    }

    /// 测试入口：最小预设 + 禁用中继 + 随机端口（hermetic，不触外网）。
    /// 追加复制状态用独立默认实例（禁用态）：测试默认不受该闸门影响。
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
        Self::from_endpoint(
            endpoint,
            store,
            sink,
            true,
            Arc::new(Mutex::new(AppendCopyState::default())),
        )
        .await
    }

    async fn from_endpoint(
        endpoint: Endpoint,
        store: Store,
        sink: Arc<dyn LanEventSink>,
        relay_disabled: bool,
        append_state: Arc<Mutex<AppendCopyState>>,
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
                append_state,
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

    /// 会话登记的准入复验（v0.9.2 A1，替代原入站门 `inbound_allowed`）：
    /// None = 允许成为会话；Some(reason) = 拒绝登记，reason 为连接关闭的
    /// 应用层原因。覆盖两类不得成为会话的对端：
    /// - store 行不可信（已撤销/已删除）→ `b"revoked"`；
    /// - 被用户显式断开（内存态标记，重新配对或重启恢复）→ `b"disconnected"`。
    fn session_admission_denied(&self, node_hex: &str) -> Option<&'static [u8]> {
        if !self.inner.store.is_trusted(node_hex).unwrap_or(false) {
            return Some(b"revoked");
        }
        if self
            .inner
            .disconnected
            .lock()
            .expect("disconnected 锁中毒")
            .contains(node_hex)
        {
            return Some(b"disconnected");
        }
        None
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

    /// 单条入站连接：按**首帧**分流（v0.9.2 A2），不再按本端信任态分流。
    ///
    /// - 首帧 `PairRequest` → 票据配对门（无论对端陌生、仍被信任或曾被显式
    ///   断开）。修复两个不可达路径：(i) 仍信任的对端持 PairRequest 拨入曾被
    ///   当作会话处理、首帧被会话循环吞掉 → 拨号方挂 30s 得「对方响应异常」；
    ///   (ii) 被显式断开的对端曾被直接 close(b"disconnected")，无恢复路径
    ///   （文档承诺的「重新配对恢复」形同虚设）——现在有效邀请 + 接受即清除
    ///   断开标记并重建会话；无有效邀请则沿用配对门的静默/过期处理。
    /// - 其余任何首帧（拨号方 link_task/join 的首发 Ping、未知帧）→ 会话路径：
    ///   已读的首帧字节经 `PrefixedFrame` 无损回放给会话循环（Ping 照常回
    ///   Pong），信任与断开门统一由 run_session 的登记前复验（A1）把关——
    ///   未配对/已撤销/被显式断开的拨入在登记临界区内被拒，不产生幽灵会话。
    async fn handle_inbound(self: Arc<Self>, conn: Connection) {
        let remote = conn.remote_id();
        let node_hex = hex_encode_32(remote.as_bytes());
        // 预认证读取限时（slow-loris 防护）：accept_bi 与首帧读取合计 60s 内不
        // 完成即静默关闭，任务不无限堆积。首帧的线格式字节一并保留（回放用）。
        let first = tokio::time::timeout(STRANGER_PREAUTH_TIMEOUT, async {
            let (send, mut recv) = conn.accept_bi().await.ok()?;
            let (msg, raw) = read_message_with_raw(&mut recv).await.ok()?;
            Some((send, recv, msg, raw))
        })
        .await
        .ok()
        .flatten();
        let Some((send, recv, msg, raw)) = first else {
            conn.close(VarInt::from_u32(0), b"pair-preauth-timeout");
            return; // 静默拒绝：超时/坏帧/连接死亡
        };
        let LanMessage::PairRequest { version, device_name: peer_name, invite_secret } = msg
        else {
            // —— 会话路径（首帧非 PairRequest）——
            let dead_rx = Self::watch_conn_death(conn.clone());
            let reader = PrefixedFrame::new(raw, recv);
            self.run_session(Some(conn.clone()), node_hex, reader, send, dead_rx).await;
            conn.close(VarInt::from_u32(0), b"session-end");
            return;
        };
        // —— 配对门（spec §4.2：无邀请的连接不产生任何提示，防提示轰炸/探测）——
        // 撤销门在最前（邀请校验与用户确认之前）：已撤销设备持有效票据再次拨入
        // 也按静默拒绝处理——不弹配对请求、不回任何帧（spec §3，重新配对需先
        // 删除记录，撤销行不得经配对复活）。
        if self.is_locally_revoked(&node_hex) {
            return;
        }
        let mut send = send;
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
        *self.inner.pending_pair.lock().expect("pending 锁中毒") = Some(pairing::PendingPair {
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
        // 重新配对成功：解除此前的「显式断开」标记（A2：对端曾处于 disconnected
        // 集合也能走到这里，清标记后 run_session 的准入复验放行，链路恢复）
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
        drop(recv); // 配对流的读半至此不再消费（首帧 PairRequest 已处理完毕）
        // 等拨号方开第二条（会话）流（拨号方开流即发首发帧，见 send_stream_opener）
        let Ok((session_send, session_recv)) = conn.accept_bi().await else { return };
        let dead_rx = Self::watch_conn_death(conn.clone());
        self.run_session(Some(conn.clone()), node_hex, session_recv, session_send, dead_rx)
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
}

/// 测试辅助（集成测试消费）：本端 EndpointId 的 64 字符 hex（即对端眼中的
/// node_id）。避免测试直接触私有字段。
#[cfg(test)]
impl DeviceLinkRegistry {
    pub(crate) fn inner_endpoint_id_hex_for_test(&self) -> String {
        hex_encode_32(self.inner.endpoint.id().as_bytes())
    }

    /// 集成测试观察口：registry 级 recent 滑窗是否已登记该哈希（auto 接收路径
    /// 插入、发送侧 fan_out_auto 消费——auto/手动两路径的可观测分界，Spec 2）。
    pub(crate) fn recent_contains(&self, hash: &str) -> bool {
        self.inner.recent.contains(hash)
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

