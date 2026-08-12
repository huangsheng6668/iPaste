pub(crate) mod protocol;
pub(crate) mod session;  // Task 3
pub(crate) mod server;   // Task 4
pub(crate) mod client;   // Task 5
pub(crate) mod commands; // Task 6
pub(crate) mod port;     // Task 2: 跨平台端口占用检测

pub(crate) use port::PortConflict;

use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::lan_sync::protocol::*;
use crate::models::*;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LanRole { Host, Guest }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LanStatus { Idle, Hosting, WaitingPair, Connected }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanSessionInfo {
    pub(crate) role: Option<LanRole>,
    pub(crate) status: LanStatus,
    pub(crate) code: Option<String>,
    pub(crate) listen_addr: Option<String>,
    pub(crate) peer_device_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum ClipSource {
    Current,
    Item { id: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanPairRequest { pub(crate) guest_id: String, pub(crate) device_name: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanSessionReady { pub(crate) peer_device_name: String, pub(crate) role: LanRole }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanDisconnected { pub(crate) reason: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanClipReceived { pub(crate) clip_type: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanJoinFailed { pub(crate) reason: String }

/// host 因非 Hosting 态拒绝 guest 时发出（host 侧事件），携带当时 host 的状态，
/// 用于前端提示"有设备尝试加入但当前正忙"以及定位扫描加入被拒的根因。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanGuestRejected {
    pub(crate) guest_device_name: String,
    pub(crate) host_status: LanStatus,
}

/// 扫描发现的局域网设备（供 Guest 自动扫描列表展示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LanDevice {
    pub(crate) device_name: String,
    pub(crate) addr: String,
}

/// session loop 的控制指令
#[derive(Debug)]
pub(crate) enum ControlMsg {
    SendClip { clip_type: String, payload: Vec<u8> },
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
    /// Host 的 accept 任务句柄；Task 6 的 disconnect 命令负责 abort 它以释放端口。
    /// `Option` 让 `#[derive(Default)]` 继续成立。
    host_tasks: Option<tokio::task::JoinHandle<()>>,
}

impl Default for LanStatus { fn default() -> Self { LanStatus::Idle } }

pub struct LanSessionManager {
    app: AppHandle,
    inner: Mutex<LanInner>,
}

impl LanSessionManager {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app, inner: Mutex::new(LanInner::default()) }
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
    ) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.role = Some(LanRole::Host);
        inner.status = LanStatus::Hosting;
        inner.code = Some(code);
        inner.listen_addr = Some(listen_addr);
        inner.peer_device_name = None;
        inner.control_tx = Some(control_tx);
        inner.control_rx = Some(control_rx);
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

    pub(crate) fn set_joining(
        &self,
        code: String,
        control_tx: mpsc::Sender<ControlMsg>,
        control_rx: mpsc::Receiver<ControlMsg>,
    ) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.role = Some(LanRole::Guest);
        inner.status = LanStatus::WaitingPair;
        inner.code = Some(code);
        inner.control_tx = Some(control_tx);
        inner.control_rx = Some(control_rx);
        inner.pair_decision_tx = None;
    }

    pub(crate) fn set_connected(&self, peer_device_name: String) {
        let role = {
            let mut inner = self.inner.lock().expect("lan inner poisoned");
            inner.status = LanStatus::Connected;
            inner.peer_device_name = Some(peer_device_name.clone());
            inner.role.unwrap_or(LanRole::Guest)
        };
        let _ = self.app.emit("ipaste://lan-session-ready", LanSessionReady {
            peer_device_name,
            role,
        });
    }

    pub(crate) fn take_pair_decision_tx(&self) -> Option<oneshot::Sender<bool>> {
        self.inner.lock().expect("lan inner poisoned").pair_decision_tx.take()
    }

    pub(crate) fn control_tx(&self) -> Option<mpsc::Sender<ControlMsg>> {
        self.inner.lock().expect("lan inner poisoned").control_tx.clone()
    }

    /// 取出 control_rx，交给首个建立的会话循环。
    pub(crate) fn take_control_rx(&self) -> Option<mpsc::Receiver<ControlMsg>> {
        self.inner.lock().expect("lan inner poisoned").control_rx.take()
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
            // independently-spawned task (handle_guest_with_handshake), not a
            // structured child of the accept loop, so aborting the accept task
            // does not panic or unwind the caller. JoinHandle::abort schedules
            // cancellation at the next .await.
            // (lan_disconnect's Hosting|WaitingPair arm calls abort_host_tasks()
            // separately — double-abort is harmless since take() yields None the
            // second time.)
            if let Some(accept) = inner.host_tasks.take() {
                accept.abort();
            }
        }
        let _ = self.app.emit("ipaste://lan-disconnected", LanDisconnected { reason });
    }

    pub(crate) fn emit_clip_received(&self, clip_type: String) {
        let _ = self.app.emit("ipaste://lan-clip-received", LanClipReceived { clip_type });
    }

    pub(crate) fn emit_join_failed(&self, reason: String) {
        let _ = self.app.emit("ipaste://lan-join-failed", LanJoinFailed { reason });
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
}

#[cfg(test)]
mod integration_tests;
