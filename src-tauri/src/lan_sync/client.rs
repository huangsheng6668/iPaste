//! Guest 客户端：直连对方 `ip:port` 握手后进入会话循环。

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::*;
use crate::store::Store;

/// 与 host 握手：等 PairChallenge → 发持码证明 → 校验 PairAccepted → 进 session loop。
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

    use crate::lan_sync::crypto::{
        derive_session_key, generate_pair_keys, guest_proof, host_transcript_tag, transcript_hash,
        SecureConnection,
    };

    let mut conn = Connection::new(stream);
    // v4：先等 host 的挑战。等不到 = 对方是 v3 老版本（只等不答）或不可达。
    let (host_device_name, host_public) = match tokio::time::timeout(
        Duration::from_secs(CHALLENGE_WAIT_TIMEOUT_SECS),
        conn.read_message(),
    )
    .await
    {
        Ok(Ok((LanMessage::PairChallenge { version, host_device_name, host_pubkey }, _)))
            if version == LAN_PROTOCOL_VERSION =>
        {
            match base64::engine::general_purpose::STANDARD
                .decode(&host_pubkey.unwrap_or_default())
                .map_err(|_| ())
                .and_then(|v| <[u8; 32]>::try_from(v).map_err(|_| ()))
            {
                Ok(bytes) => (host_device_name, PublicKey::from(bytes)),
                Err(_) => {
                    manager.emit_join_failed("握手响应异常".to_string());
                    manager.reset_to_idle("握手异常".to_string());
                    return;
                }
            }
        }
        _ => {
            if cfg!(test) { eprintln!("[client] challenge wait timed out or malformed"); }
            manager.emit_join_failed("对方版本过旧，请升级 iPaste 后重试".to_string());
            manager.reset_to_idle("版本不兼容".to_string());
            return;
        }
    };

    let local_name = device_name();
    let (guest_secret, guest_public) = generate_pair_keys();
    let key = derive_session_key(&guest_secret, &host_public, code);
    let transcript = transcript_hash(
        LAN_PROTOCOL_VERSION,
        &host_device_name,
        &host_public,
        &local_name,
        &guest_public,
    );
    let msg = LanMessage::Handshake {
        version: LAN_PROTOCOL_VERSION,
        device_name: local_name,
        guest_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes())),
        guest_proof: Some(guest_proof(&key, &transcript)),
    };
    if conn.write_message(&msg, None).await.is_err() {
        manager.emit_join_failed("连接已断开".to_string());
        manager.reset_to_idle("连接已断开".to_string());
        return;
    }
    let (reply, _payload) = match conn.read_message().await {
        Ok(v) => v,
        Err(e) => {
            if cfg!(test) { eprintln!("[client] reply read failed: {e}"); }
            manager.emit_join_failed(e);
            manager.reset_to_idle("连接已断开".to_string());
            return;
        }
    };
    match reply {
        LanMessage::PairAccepted { host_device_name, auth_tag } => {
            let Some(tag) = auth_tag else {
                manager.emit_join_failed("握手响应异常".to_string());
                manager.reset_to_idle("握手异常".to_string());
                return;
            };
            // 转录绑定的 host 认证：证明对方持码，且挑战里的设备名/公钥未被替换。
            if host_transcript_tag(&key, &transcript) != tag {
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
    let channel_id = crate::lan_sync::next_control_channel_id();
    if cfg!(test) { eprintln!("[client] creating control channel #{channel_id}"); }
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_joining(code.clone(), control_tx, control_rx, channel_id);

    let stream = match tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(addr.trim()),
    )
    .await
    {
        Ok(Ok(s)) => s,
        _ => {
            if cfg!(test) { eprintln!("[client] connect failed/timed out"); }
            manager.emit_join_failed("无法连接到对方".to_string());
            manager.reset_to_idle("连接失败".to_string());
            return;
        }
    };
    if cfg!(test) { eprintln!("[client] connected, starting handshake"); }
    handshake(stream, &manager, &store, &code).await;
}
