pub(crate) mod protocol;
pub(crate) mod session;  // Task 3
pub(crate) mod server;   // Task 4
pub(crate) mod client;   // Task 5
pub(crate) mod commands; // Task 6
pub(crate) mod port;     // Task 2: 跨平台端口占用检测
pub(crate) mod pair_guard; // Task 2: 按 IP 防爆破
pub(crate) mod crypto;   // Task 3: 加密原语与加密会话帧

pub(crate) use port::PortConflict;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::lan_sync::pair_guard::PairGuard;

/// 事件出口抽象：生产环境转发到 Tauri 前端；测试用 Noop（不构造任何
/// 窗口运行时，避免把 tao/wry 的 GUI 原生代码链接进测试二进制）。
trait LanEventSink: Send + Sync + 'static {
    fn emit(&self, event: &str, payload: &serde_json::Value);
}

/// 生产事件出口：经真实 AppHandle emit 到前端。
struct TauriEventSink {
    app: AppHandle,
}

impl LanEventSink for TauriEventSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        let _ = self.app.emit(event, payload);
    }
}

/// 测试事件出口：空操作。
struct NoopEventSink;

impl LanEventSink for NoopEventSink {
    fn emit(&self, _event: &str, _payload: &serde_json::Value) {}
}

/// 测试事件出口：把 (event, payload) 记入共享 Vec，供集成测试断言 emit 内容
/// （如 join-failed 的具体提示文案——「对方版本过旧」曾被误报，需要按文案回归）。
pub(crate) struct CapturingEventSink {
    pub(crate) events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl LanEventSink for CapturingEventSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        self.events.lock().expect("capture sink poisoned").push((event.to_string(), payload.clone()));
    }
}

impl CapturingEventSink {
    /// 取所有 `lan-join-failed` 事件的 reason 文案。
    pub(crate) fn join_failed_reasons(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("capture sink poisoned")
            .iter()
            .filter(|(event, _)| event == EVENT_LAN_JOIN_FAILED)
            .map(|(_, payload)| {
                payload
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }
}
use crate::lan_sync::protocol::*;
use crate::models::*;
use crate::events::*;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) enum LanRole { Host, Guest }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) enum LanStatus { Idle, Hosting, WaitingPair, Connected }

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct LanSessionInfo {
    pub(crate) role: Option<LanRole>,
    pub(crate) status: LanStatus,
    pub(crate) code: Option<String>,
    pub(crate) listen_addr: Option<String>,
    pub(crate) peer_device_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub(crate) enum ClipSource {
    Current,
    Item { id: String },
    /// 分组（category）条目：id 为 `category_items.id`，category_id 为其所属分组。
    /// 发送时会附带分组名/颜色，接收端按名称匹配或新建分组。
    CategoryItem {
        id: String,
        #[serde(rename = "categoryId")]
        category_id: String,
    },
}

/// session loop 的控制指令
#[derive(Debug)]
pub(crate) enum ControlMsg {
    /// 开始分组批量发送：session loop 先写 `CategoryBatchStart` 帧，随后
    /// 逐条 `SendClip`，最后 `BatchEnd` 写 `CategoryBatchEnd` 帧。
    BatchStart {
        category_name: String,
        category_color: Option<String>,
        item_count: u32,
    },
    /// 结束分组批量发送。
    BatchEnd,
    SendClip {
        clip_type: String,
        payload: Vec<u8>,
        /// 分组名（按名称在接收端匹配/创建分组）；None = 历史/无分组。
        category_name: Option<String>,
        /// 分组颜色（随分组名一起传，新建分组时采用）。
        category_color: Option<String>,
        /// 条目的重命名显示名；None = 未重命名。
        display_name: Option<String>,
    },
    RequestClip,
    Disconnect,
}

#[derive(Default)]
struct LanInner {
    role: Option<LanRole>,
    status: LanStatus,
    code: Option<String>,
    listen_addr: Option<String>,
    peer_device_name: Option<String>,
    pair_decision_tx: Option<oneshot::Sender<bool>>,
    control_tx: Option<mpsc::Sender<ControlMsg>>,
    control_rx: Option<mpsc::Receiver<ControlMsg>>,
    /// 诊断用：当前 control channel 的编号（临时调试）。
    control_channel_id: Option<u64>,
    /// Host 的 accept 任务句柄；Task 6 的 disconnect 命令负责 abort 它以释放端口。
    /// `Option` 让 `#[derive(Default)]` 继续成立。
    host_tasks: Option<tokio::task::JoinHandle<()>>,
    /// Guest 的 join 任务句柄；`reset_to_idle` 时 abort——消灭「用户已断开、
    /// 残留握手任务继续跑完再弹 join 失败」的迟到错误事件。
    join_task: Option<tokio::task::JoinHandle<()>>,
}

/// 诊断用：为每个新建的控制通道分配递增编号（临时调试）。
static CONTROL_CHANNEL_COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) fn next_control_channel_id() -> u64 {
    CONTROL_CHANNEL_COUNTER.fetch_add(1, Ordering::Relaxed) + 1
}

impl Default for LanStatus { fn default() -> Self { LanStatus::Idle } }

/// 会话状态机。事件经 `sink` 转发到前端：生产环境为真实 AppHandle，
/// 测试（`new_for_test`）为空操作——不引用任何窗口运行时，测试二进制
/// 不会链接 tao/wry 的 GUI 原生代码。
pub struct LanSessionManager {
    sink: Arc<dyn LanEventSink>,
    inner: Mutex<LanInner>,
    pair_guard: PairGuard,
}

impl LanSessionManager {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self {
            sink: Arc::new(TauriEventSink { app }),
            inner: Mutex::new(LanInner::default()),
            pair_guard: PairGuard::new(),
        }
    }

    /// 测试专用构造：不绑定 AppHandle（纯测试无法构造真实窗口运行时），
    /// emit 均为空操作。
    pub(crate) fn new_for_test() -> Self {
        Self {
            sink: Arc::new(NoopEventSink),
            inner: Mutex::new(LanInner::default()),
            pair_guard: PairGuard::new(),
        }
    }

    /// 测试专用构造：事件写入共享捕获器，供断言 emit 的提示文案。
    pub(crate) fn new_capturing_for_test() -> (Arc<Self>, Arc<CapturingEventSink>) {
        let sink = Arc::new(CapturingEventSink { events: Mutex::new(Vec::new()) });
        (
            Arc::new(Self {
                sink: sink.clone(),
                inner: Mutex::new(LanInner::default()),
                pair_guard: PairGuard::new(),
            }),
            sink,
        )
    }

    /// 统一的事件出口（测试时为空操作）。
    fn emit<E: Serialize>(&self, event: &str, payload: E) {
        let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
        self.sink.emit(event, &value);
    }

    pub(crate) fn pair_guard(&self) -> &PairGuard {
        &self.pair_guard
    }

    pub(crate) fn snapshot(&self) -> LanSessionInfo {
        let inner = self.inner.lock().expect("lan inner poisoned");
        LanSessionInfo {
            role: inner.role,
            status: inner.status,
            code: inner.code.clone(),
            listen_addr: inner.listen_addr.clone(),
            peer_device_name: inner.peer_device_name.clone(),
        }
    }

    pub(crate) fn set_hosting(
        &self,
        code: String,
        listen_addr: String,
        control_tx: mpsc::Sender<ControlMsg>,
        control_rx: mpsc::Receiver<ControlMsg>,
        channel_id: u64,
    ) {
        if cfg!(test) { eprintln!("[mgr] set_hosting storing control channel #{channel_id}"); }
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.role = Some(LanRole::Host);
        inner.status = LanStatus::Hosting;
        inner.code = Some(code);
        inner.listen_addr = Some(listen_addr);
        inner.peer_device_name = None;
        inner.control_tx = Some(control_tx);
        inner.control_rx = Some(control_rx);
        inner.control_channel_id = Some(channel_id);
        inner.pair_decision_tx = None;
    }

    /// 原子配对门：在单次 lock 内完成「Hosting 检查 + 转 WaitingPair + 预留 oneshot」，
    /// 杜绝 accept 循环的 TOCTOU 竞态（两个 guest 同时通过 check → 第二个覆盖第一个
    /// sender，第一个 guest 被静默丢弃）。成功返回 receiver（供 host 等待用户决定），
    /// 非 Hosting 态返回 None —— 调用方据此拒绝该 guest。
    pub(crate) fn try_begin_pairing(&self) -> Option<oneshot::Receiver<bool>> {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        if !matches!(inner.status, LanStatus::Hosting) {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        inner.pair_decision_tx = Some(tx);
        inner.status = LanStatus::WaitingPair;
        Some(rx)
    }

    /// 原子加入门：仅 Idle 态允许进入 join 流程。单次 lock 内完成「Idle 检查 +
    /// 置 WaitingPair(guest) + 登记控制通道」，杜绝并发 join（双击/并发命令）
    /// 互相覆写控制通道的竞态。非 Idle 返回 false，调用方应静默退出。
    pub(crate) fn try_set_joining(
        &self,
        code: String,
        control_tx: mpsc::Sender<ControlMsg>,
        control_rx: mpsc::Receiver<ControlMsg>,
        channel_id: u64,
    ) -> bool {
        if cfg!(test) { eprintln!("[mgr] try_set_joining storing control channel #{channel_id}"); }
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        if !matches!(inner.status, LanStatus::Idle) {
            return false;
        }
        inner.role = Some(LanRole::Guest);
        inner.status = LanStatus::WaitingPair;
        inner.code = Some(code);
        inner.control_tx = Some(control_tx);
        inner.control_rx = Some(control_rx);
        inner.control_channel_id = Some(channel_id);
        inner.pair_decision_tx = None;
        true
    }

    pub(crate) fn set_connected(&self, peer_device_name: String) {
        let role = {
            let mut inner = self.inner.lock().expect("lan inner poisoned");
            inner.status = LanStatus::Connected;
            inner.peer_device_name = Some(peer_device_name.clone());
            inner.role.unwrap_or(LanRole::Guest)
        };
        self.emit(EVENT_LAN_SESSION_READY, LanSessionReady {
            peer_device_name,
            role,
        });
    }

    pub(crate) fn take_pair_decision_tx(&self) -> Option<oneshot::Sender<bool>> {
        self.inner.lock().expect("lan inner poisoned").pair_decision_tx.take()
    }

    pub(crate) fn control_tx(&self) -> Option<mpsc::Sender<ControlMsg>> {
        let inner = self.inner.lock().expect("lan inner poisoned");
        let tx = inner.control_tx.clone();
        if cfg!(test) { eprintln!("[mgr] control_tx cloned, present={}, channel_id={:?}", tx.is_some(), inner.control_channel_id); }
        tx
    }

    /// 取出 control_rx，交给首个建立的会话循环。
    pub(crate) fn take_control_rx(&self) -> Option<mpsc::Receiver<ControlMsg>> {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        let rx = inner.control_rx.take();
        if cfg!(test) { eprintln!("[mgr] take_control_rx called, present={}, channel_id={:?}", rx.is_some(), inner.control_channel_id); }
        rx
    }

    /// 记录 host 的 accept 任务句柄，供 Task 6 的 disconnect 命令 abort 以释放端口。
    /// 调用方（`start_host`）在 `tokio::spawn` 返回后立即调用；此处不 abort。
    pub(crate) fn set_host_task(
        &self,
        accept: tokio::task::JoinHandle<()>,
    ) {
        self.inner.lock().expect("lan inner poisoned").host_tasks = Some(accept);
    }

    /// Task 6 的 disconnect 命令在 Hosting/WaitingPair 态下调用：abort accept
    /// 任务以释放 TCP 端口。Connected 态不需要调用（session loop 会自清理）。
    /// Guest 在 WaitingPair 态调用为 no-op（其 `host_tasks` 为 None）。
    pub(crate) fn abort_host_tasks(&self) {
        if let Some(accept) =
            self.inner.lock().expect("lan inner poisoned").host_tasks.take()
        {
            accept.abort();
        }
    }

    /// 登记 guest 的 join 任务句柄（`lan_join_by_address` spawn 后调用），
    /// 供 `reset_to_idle` / 断开时 abort。前任句柄仍存活时不顶替——那只会
    /// 出现在并发 join 的微竞态里（`try_set_joining` 已挡掉绝大多数），
    /// 前任才是真正持有会话状态的任务。
    pub(crate) fn set_join_task(&self, join: tokio::task::JoinHandle<()>) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        if let Some(prev) = &inner.join_task {
            if !prev.is_finished() {
                return;
            }
        }
        inner.join_task = Some(join);
    }

    /// Task 6 的 create_session 守门：已有进行中的会话时拒绝新建。
    /// WaitingPair 也算进行中（host 正在询问用户 / guest 正在等回应）。
    pub(crate) fn status_is_connected_or_hosting(&self) -> bool {
        matches!(
            self.inner.lock().expect("lan inner poisoned").status,
            LanStatus::Connected | LanStatus::Hosting | LanStatus::WaitingPair
        )
    }

    /// 拒绝 guest 后回到 Hosting（持久 host 会话设计）：清掉残留的 pair 状态，
    /// 继续接受新 guest。**不 emit 任何事件** —— host 会话本身未中断，
    /// 只是拒绝了一个 guest（区别于 `reset_to_idle` 会停掉整个 host）。
    pub(crate) fn resume_hosting(&self) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.status = LanStatus::Hosting;
        inner.pair_decision_tx = None;
        inner.peer_device_name = None;
    }

    pub(crate) fn reset_to_idle(&self, reason: String) {
        {
            let mut inner = self.inner.lock().expect("lan inner poisoned");
            inner.role = None;
            inner.status = LanStatus::Idle;
            inner.code = None;
            inner.listen_addr = None;
            inner.peer_device_name = None;
            inner.pair_decision_tx = None;
            inner.control_tx = None;
            inner.control_rx = None;
            // Abort accept task. Safe: the session loop runs on an
            // independently-spawned task (handle_guest_with_challenge), not a
            // structured child of the accept loop, so aborting the accept task
            // does not panic or unwind the caller. JoinHandle::abort schedules
            // cancellation at the next .await.
            // (lan_disconnect's Hosting|WaitingPair arm calls abort_host_tasks()
            // separately — double-abort is harmless since take() yields None the
            // second time.)
            if let Some(accept) = inner.host_tasks.take() {
                accept.abort();
            }
            // Abort in-flight guest join task. Self-abort is safe: every join-task
            // code path that calls reset_to_idle returns immediately afterwards
            // without another .await (join failure arms `return`; session end
            // breaks out of the loop and only calls the non-async
            // `read_task.abort()`), so the cancellation flag never fires on the
            // task itself. External callers (lan_disconnect) get the task
            // cancelled at its next await point — killing the zombie handshake
            // that used to emit a spurious lan-join-failed AFTER the user
            // already disconnected.
            if let Some(join) = inner.join_task.take() {
                join.abort();
            }
        }
        self.emit(EVENT_LAN_DISCONNECTED, LanDisconnected { reason });
    }

    /// host 收到 guest 的配对请求：通知前端弹确认框。
    pub(crate) fn emit_pair_request(&self, guest_id: String, device_name: String) {
        self.emit(
            EVENT_LAN_PAIR_REQUEST,
            LanPairRequest { guest_id, device_name },
        );
    }

    /// host 因非 Hosting 态拒绝 guest：通知前端展示诊断提示。
    pub(crate) fn emit_guest_rejected(&self, guest_device_name: String, host_status: LanStatus) {
        self.emit(
            EVENT_LAN_GUEST_REJECTED,
            LanGuestRejected { guest_device_name, host_status },
        );
    }

    pub(crate) fn emit_clip_received(&self, clip_type: String, category_name: Option<String>) {
        self.emit(
            EVENT_LAN_CLIP_RECEIVED,
            LanClipReceived { clip_type, category_name },
        );
    }

    pub(crate) fn emit_join_failed(&self, reason: String) {
        self.emit(EVENT_LAN_JOIN_FAILED, LanJoinFailed { reason });
    }

    /// 接收侧解析/落库失败时调用：emit 诊断事件 + 打印日志，避免静默丢弃。
    pub(crate) fn emit_clip_receive_failed(&self, reason: String) {
        eprintln!("[lan-sync] 接收条目失败：{reason}");
        self.emit(
            EVENT_LAN_CLIP_RECEIVE_FAILED,
            LanClipReceiveFailed { reason },
        );
    }

    /// 发送端整组发送完成：emit 汇总事件（前端用于提示）。
    pub(crate) fn emit_category_sent(&self, category_name: String, sent: u32, failed: u32) {
        self.emit(
            EVENT_LAN_CATEGORY_SENT,
            LanCategorySent { category_name, sent, failed },
        );
    }

    /// 接收端整组接收完成：emit 汇总事件（前端据此刷新一次列表并提示）。
    pub(crate) fn emit_category_received(&self, category_name: String, count: u32, failed: u32) {
        self.emit(
            EVENT_LAN_CATEGORY_RECEIVED,
            LanCategoryReceived { category_name, count, failed },
        );
    }
}

pub(crate) fn device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "iPaste-device".to_string())
}

/// 让 commands 从 `AppHandle` 取到共享的 `Arc<LanSessionManager>`。
/// lib.rs 的 setup 里 `.manage(Arc::new(LanSessionManager::new(...)))` 注入。
pub trait LanManagerExt {
    fn lan_manager(&self) -> std::sync::Arc<LanSessionManager>;
}

impl LanManagerExt for tauri::AppHandle {
    fn lan_manager(&self) -> std::sync::Arc<LanSessionManager> {
        self.state::<std::sync::Arc<LanSessionManager>>()
            .inner()
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_source_deserializes_current() {
        let json = r#"{"kind":"current"}"#;
        let src: ClipSource = serde_json::from_str(json).unwrap();
        assert!(matches!(src, ClipSource::Current));
    }

    #[test]
    fn clip_source_deserializes_item() {
        let json = r#"{"kind":"item","id":"abc"}"#;
        let src: ClipSource = serde_json::from_str(json).unwrap();
        match src { ClipSource::Item { id } => assert_eq!(id, "abc"), _ => panic!("wrong variant") }
    }

    #[test]
    fn clip_source_deserializes_category_item() {
        // 前端 ipasteApi 走 camelCase（与 TS 类型一致）。
        let json = r#"{"kind":"categoryItem","id":"i1","categoryId":"c1"}"#;
        let src: ClipSource = serde_json::from_str(json).expect("camelCase variant must deserialize");
        match src {
            ClipSource::CategoryItem { id, category_id } => {
                assert_eq!(id, "i1");
                assert_eq!(category_id, "c1");
            }
            _ => panic!("wrong variant for {json}"),
        }
    }
}

#[cfg(test)]
mod integration_tests;
