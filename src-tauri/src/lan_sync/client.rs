//! Guest 客户端：通过 UDP 广播发现 host 或直连 IP，握手后进入会话循环。
//!
//! 见 `task-5-brief.md`。本模块在 Task 5 取消注释启用。

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::net::UdpSocket;

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
    let host_device_name = device_name();
    let msg = LanMessage::Handshake {
        code: code.to_string(),
        device_name: host_device_name,
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
        LanMessage::PairRejected => {
            manager.emit_join_failed("匹配码错误或被拒绝".to_string());
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

/// 广播发现模式：绑定 LAN_UDP_PORT 监听 host 广播，3s 内匹配 codeHash，
/// 命中后用 **广播包源 IP**（即 host 出口 IP）+ tcpPort 拨号 TCP。
pub(crate) async fn join_by_broadcast(
    manager: Arc<LanSessionManager>,
    store: Store,
    code: String,
) {
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_joining(code.clone(), control_tx, control_rx);
    let target_hash = code_hash(&code);

    let found = tokio::time::timeout(Duration::from_secs(3), async {
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", LAN_UDP_PORT)).await else {
            return None::<(String, u16)>;
        };
        let mut buf = [0u8; 256];
        loop {
            let Ok((n, src)) = sock.recv_from(&mut buf).await else {
                continue;
            };
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf[..n]) {
                let h = val.get("codeHash").and_then(|v| v.as_str()).unwrap_or("");
                let port = val.get("tcpPort").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                if h == target_hash && port > 0 {
                    // UDP 广播 datagram 的源地址即发送方 IP，直接复用。
                    return Some((src.ip().to_string(), port));
                }
            }
        }
    })
    .await;

    let (ip, port) = match found {
        Ok(Some((ip, port))) => (ip, port),
        _ => {
            manager.emit_join_failed("未发现设备，请改用手动 IP".to_string());
            manager.reset_to_idle("未发现".to_string());
            return;
        }
    };
    let addr = format!("{}:{}", ip, port);
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            manager.emit_join_failed(format!("无法连接：{e}"));
            manager.reset_to_idle("连接失败".to_string());
            return;
        }
    };
    handshake(stream, &manager, &store, &code).await;
}
