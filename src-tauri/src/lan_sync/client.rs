//! Guest 客户端：直连对方 `ip:port` 握手后进入会话循环。

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::*;
use crate::store::Store;

/// 与 host 握手：发 Handshake → 读 PairAccepted/PairRejected → 进 session loop。
///
/// 失败分支统一 `emit_join_failed` + `reset_to_idle`，不留半态。
async fn handshake(
    stream: TcpStream,
    manager: &Arc<LanSessionManager>,
    store: &Store,
    code: &str,
) {
    let mut conn = Connection::new(stream);
    // 本机设备名（区别于握手响应里 host 回传的 host_device_name）
    let local_name = device_name();
    let msg = LanMessage::Handshake {
        code: code.to_string(),
        device_name: local_name,
    };
    if conn.write_message(&msg, None).await.is_err() {
        manager.emit_join_failed("连接已断开".to_string());
        manager.reset_to_idle("连接已断开".to_string());
        return;
    }
    let (reply, _payload) = match conn.read_message().await {
        Ok(v) => v,
        Err(e) => {
            manager.emit_join_failed(e);
            manager.reset_to_idle("连接已断开".to_string());
            return;
        }
    };
    let peer_name = match reply {
        LanMessage::PairAccepted { host_device_name } => host_device_name,
        LanMessage::PairRejected { reason } => {
            // 按真实原因提示，避免 host 忙/拒绝也一律报"码错"误导用户。
            let msg = match reason {
                PairRejectReason::WrongCode => "匹配码错误",
                PairRejectReason::HostBusy => "对方正忙（已在会话中或正在配对）",
                PairRejectReason::Declined => "对方拒绝了加入请求",
                PairRejectReason::Unknown => "匹配码错误或被拒绝",
            };
            manager.emit_join_failed(msg.to_string());
            manager.reset_to_idle("被拒绝".to_string());
            return;
        }
        _ => {
            manager.emit_join_failed("握手响应异常".to_string());
            manager.reset_to_idle("握手异常".to_string());
            return;
        }
    };
    let raw = conn.into_stream();
    let Some(control_rx) = manager.take_control_rx() else {
        manager.reset_to_idle("内部状态错误".to_string());
        return;
    };
    run_session_loop(raw, manager.clone(), store.clone(), peer_name, control_rx).await;
}

/// 直连模式：按用户提供的 `ip:port` 拨号 TCP，超时 5s，失败 emit lan-join-failed。
pub(crate) async fn join_by_address(
    manager: Arc<LanSessionManager>,
    store: Store,
    addr: String,
    code: String,
) {
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_joining(code.clone(), control_tx, control_rx);

    let stream = match tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(addr.trim()),
    )
    .await
    {
        Ok(Ok(s)) => s,
        _ => {
            manager.emit_join_failed("无法连接到对方".to_string());
            manager.reset_to_idle("连接失败".to_string());
            return;
        }
    };
    handshake(stream, &manager, &store, &code).await;
}
