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
    // v4：先等 host 的挑战。按真实原因分流报错——此前所有非挑战结果一律
    // 误报「对方版本过旧」，导致「B 断开后 A 重连被 RST」也提示升级版本。
    // - 对端关闭/重置连接：host 恰好断开或退出，报连接错误；
    // - 超时：v3 老 host 只等不答（真正的版本过旧）或网络不通；
    // - 挑战版本不符：确定的版本过旧。
    let (host_device_name, host_pubkey) = match tokio::time::timeout(
        Duration::from_secs(CHALLENGE_WAIT_TIMEOUT_SECS),
        conn.read_message(),
    )
    .await
    {
        // 连接被对端关闭/重置（EOF / RST / 畸形帧）：不是版本问题。
        Ok(Err(reason)) => {
            manager.emit_join_failed(format!("对方已断开或无法建立连接：{reason}"));
            manager.reset_to_idle("连接已断开".to_string());
            return;
        }
        // 超时：v3 老 host 只等不答，也可能是对端网络不可达。
        Err(_elapsed) => {
            manager.emit_join_failed("等待对方响应超时：对方版本可能过旧，请确认双方均为最新版 iPaste".to_string());
            manager.reset_to_idle("等待挑战超时".to_string());
            return;
        }
        Ok(Ok((LanMessage::PairChallenge { version, host_device_name, host_pubkey }, _))) => {
            if version != LAN_PROTOCOL_VERSION {
                manager.emit_join_failed("对方版本过旧，请升级 iPaste 后重试".to_string());
                manager.reset_to_idle("版本不兼容".to_string());
                return;
            }
            (host_device_name, host_pubkey)
        }
        // 挑战阶段不应出现的帧。
        Ok(Ok(..)) => {
            manager.emit_join_failed("握手响应异常".to_string());
            manager.reset_to_idle("握手异常".to_string());
            return;
        }
    };
    let (host_device_name, host_public) = {
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
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    // 原子守门失败 = 已有 join/会话进行中（并发 join 微竞态）：静默退出，
    // 不 emit、不 reset，避免覆写正在进行中的会话状态。
    if !manager.try_set_joining(code.clone(), control_tx, control_rx) {
        return;
    }

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
