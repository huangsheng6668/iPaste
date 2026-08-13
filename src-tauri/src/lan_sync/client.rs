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
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{code_claim, derive_session_key, generate_pair_keys, host_auth_tag, SecureConnection};

    let mut conn = Connection::new(stream);
    let local_name = device_name();
    let (guest_secret, guest_public) = generate_pair_keys();
    let msg = LanMessage::Handshake {
        version: LAN_PROTOCOL_VERSION,
        code_claim: Some(code_claim(code)),
        device_name: local_name,
        guest_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes())),
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
    match reply {
        LanMessage::PairAccepted { host_device_name, host_pubkey, auth_tag } => {
            // 老 host（无公钥/tag）→ 提示升级；其余握手数据异常同样拒绝。
            let (Some(host_pubkey_b64), Some(tag)) = (host_pubkey, auth_tag) else {
                manager.emit_join_failed("对方版本过旧，请升级 iPaste 后重试".to_string());
                manager.reset_to_idle("版本不兼容".to_string());
                return;
            };
            let host_public = {
                let bytes: [u8; 32] = match base64::engine::general_purpose::STANDARD
                    .decode(&host_pubkey_b64)
                    .map_err(|_| ())
                    .and_then(|v| v.try_into().map_err(|_| ()))
                {
                    Ok(b) => b,
                    Err(_) => {
                        manager.emit_join_failed("握手响应异常".to_string());
                        manager.reset_to_idle("握手异常".to_string());
                        return;
                    }
                };
                PublicKey::from(bytes)
            };
            let key = derive_session_key(&guest_secret, &host_public, code);
            if host_auth_tag(&key) != tag {
                manager.emit_join_failed("无法与对方建立安全连接".to_string());
                manager.reset_to_idle("握手校验失败".to_string());
                return;
            }
            let raw = conn.into_stream();
            let Some(control_rx) = manager.take_control_rx() else {
                manager.reset_to_idle("内部状态错误".to_string());
                return;
            };
            run_session_loop(SecureConnection::new(raw, key), manager.clone(), store.clone(), host_device_name, control_rx).await;
        }
        LanMessage::PairRejected { reason } => {
            let msg = match reason {
                PairRejectReason::WrongCode => "匹配码错误",
                PairRejectReason::HostBusy => "对方正忙（已在会话中或正在配对）",
                PairRejectReason::Declined => "对方拒绝了加入请求",
                PairRejectReason::Unknown => "匹配码错误或被拒绝",
            };
            manager.emit_join_failed(msg.to_string());
            manager.reset_to_idle("被拒绝".to_string());
        }
        _ => {
            manager.emit_join_failed("握手响应异常".to_string());
            manager.reset_to_idle("握手异常".to_string());
        }
    }
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
