pub(crate) mod protocol;
pub(crate) mod session;  // Task 3
// pub(crate) mod server;   // Task 4
// pub(crate) mod client;   // Task 5
// pub(crate) mod commands; // Task 6

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

    pub(crate) fn set_hosting(&self, code: String, listen_addr: String, control_tx: mpsc::Sender<ControlMsg>) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.role = Some(LanRole::Host);
        inner.status = LanStatus::Hosting;
        inner.code = Some(code);
        inner.listen_addr = Some(listen_addr);
        inner.peer_device_name = None;
        inner.control_tx = Some(control_tx);
        inner.pair_decision_tx = None;
    }

    pub(crate) fn set_waiting_pair(&self, tx: oneshot::Sender<bool>) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.pair_decision_tx = Some(tx);
        inner.status = LanStatus::WaitingPair;
    }

    pub(crate) fn set_joining(&self, code: String, control_tx: mpsc::Sender<ControlMsg>) {
        let mut inner = self.inner.lock().expect("lan inner poisoned");
        inner.role = Some(LanRole::Guest);
        inner.status = LanStatus::WaitingPair;
        inner.code = Some(code);
        inner.control_tx = Some(control_tx);
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
