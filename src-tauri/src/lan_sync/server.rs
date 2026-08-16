//! Host 服务端：绑定 TCP 监听 + accept 循环 + 配对确认。
//!
//! accept 后先下发 PairChallenge（v4 host 先发言），再按回包分流：Handshake →
//! 配对流程；其余静默关闭。

use std::net::IpAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::lan_sync::crypto::generate_pair_keys;
use crate::lan_sync::protocol::{
    LAN_PROTOCOL_VERSION, LAN_TCP_BASE_PORT, LanMessage, PairRejectReason, code_hash,
};
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::{ControlMsg, LanSessionManager, device_name};
use crate::store::Store;

/// 握手/配对阶段的最大并发连接数，超出即丢弃新连接（防 slowloris 型资源耗尽）。
pub(crate) const MAX_CONCURRENT_HANDSHAKES: usize = 8;
/// 首条握手消息的读取超时。
pub(crate) const HANDSHAKE_TIMEOUT_SECS: u64 = 10;

/// 启动 Host：绑定固定端口 `LAN_TCP_BASE_PORT` + accept 循环。
///
/// 成功返回 `listen_addr`（`ip:port`）。accept 任务在后台 spawn，
/// 失败时由任务自身通过 `manager.reset_to_idle` 清理状态。
pub(crate) async fn start_host(
    manager: Arc<LanSessionManager>,
    store: Store,
    code: String,
) -> Result<String, String> {
    start_host_on(manager, store, code, LAN_TCP_BASE_PORT).await
}

/// `start_host` 的测试入口：可指定端口（0 = 随机端口），供集成测试
/// 在真实 accept 循环上跑通「握手 → 配对 → 会话 → 断开」全链路。
pub(crate) async fn start_host_on(
    manager: Arc<LanSessionManager>,
    store: Store,
    code: String,
    port: u16,
) -> Result<String, String> {
    // 1. 选可用 TCP 端口（同步绑定，再转为 tokio listener）
    let (std_listener, tcp_port) = bind_tcp(port)?;
    let listener = TcpListener::from_std(std_listener)
        .map_err(|e| format!("无法切换非阻塞模式：{}", e))?;
    let listen_addr = local_ip_with_port(tcp_port);

    // 2. 注册到 manager（control_rx 也存进去，握手通过后由 handle_guest_with_challenge 取出）
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_hosting(code.clone(), listen_addr.clone(), control_tx, control_rx);

    // 3. accept 循环：并发限额 + 握手读取超时 + 按 IP 清理防爆破记录
    let sem = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_HANDSHAKES));
    let accept_handle = {
        let manager = manager.clone();
        let store = store.clone();
        let expected_code = code.clone();
        let sem = sem.clone();
        tokio::spawn(async move {
            #[allow(clippy::never_loop)]
            loop {
                let Ok((stream, peer)) = listener.accept().await else { continue };
                let ip = peer.ip();
                // 并发已满：直接丢弃新连接（防资源耗尽）
                let Ok(permit) = sem.clone().try_acquire_owned() else {
                    drop(stream);
                    continue;
                };
                let manager = manager.clone();
                let store = store.clone();
                let expected_code = expected_code.clone();
                manager.pair_guard().prune(std::time::Instant::now());
                tokio::spawn(async move {
                    let _permit = permit; // 任务结束自动释放额度
                    let mut conn = Connection::new(stream);
                    // v4：host 先发言——生成临时密钥对并立即下发挑战。
                    use base64::Engine as _;
                    let (host_secret, host_public) = generate_pair_keys();
                    let host_name = device_name();
                    let challenge = LanMessage::PairChallenge {
                        version: LAN_PROTOCOL_VERSION,
                        host_device_name: host_name.clone(),
                        host_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(host_public.as_bytes())),
                    };
                    if conn.write_message(&challenge, None).await.is_err() {
                        return;
                    }
                    // 握手读取超时，防 slowloris 型慢连接占用任务与内存（沿用 v3 值）
                    let read = tokio::time::timeout(
                        std::time::Duration::from_secs(HANDSHAKE_TIMEOUT_SECS),
                        conn.read_message(),
                    )
                    .await;
                    let Ok(Ok((msg, _))) = read else {
                        return;
                    };
                    match msg {
                        LanMessage::Handshake { version, device_name: guest_name, guest_pubkey, guest_proof } => {
                            // v3 老客户端 / 畸形帧：版本不符或缺公钥/证明 → 拒绝（老客户端收到
                            // PairRejected 后提示「匹配码错误或被拒绝」）。
                            if version != LAN_PROTOCOL_VERSION || guest_pubkey.is_none() || guest_proof.is_none() {
                                let _ = conn
                                    .write_message(&LanMessage::PairRejected { reason: PairRejectReason::Unknown }, None)
                                    .await;
                                return;
                            }
                            handle_guest_with_challenge(
                                conn,
                                &manager,
                                &store,
                                &expected_code,
                                host_secret,
                                host_public,
                                host_name,
                                guest_name,
                                guest_pubkey.unwrap(),
                                guest_proof.unwrap(),
                                ip,
                            )
                            .await;
                        }
                        _ => { /* 未知消息，静默关闭 */ }
                    }
                });
                if cfg!(test) { break; } // 测试：单次 accept 后退出，避免下一轮 accept 干扰诊断
            }
        })
    };

    // 4. 把 accept 任务句柄存进 manager，Task 6 的 disconnect 命令会 abort 它以释放端口。
    manager.set_host_task(accept_handle);

    Ok(listen_addr)
}

/// 处理已带挑战下发的连接：校验 proof（= 码校验）→ 询问用户 → 进入 session loop 或拒绝。
async fn handle_guest_with_challenge(
    mut conn: Connection,
    manager: &Arc<LanSessionManager>,
    store: &Store,
    expected_code: &str,
    host_secret: x25519_dalek::StaticSecret,
    host_public: x25519_dalek::PublicKey,
    host_name: String,
    guest_name: String,
    guest_pubkey_b64: String,
    guest_proof_value: String,
    ip: IpAddr,
) {
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{
        derive_session_key, guest_proof, host_transcript_tag, transcript_hash, SecureConnection,
    };

    // 防爆破：封禁期直接丢弃。
    if manager.pair_guard().is_blocked(ip, std::time::Instant::now()) {
        return;
    }
    let guest_public = {
        let bytes: [u8; 32] = match base64::engine::general_purpose::STANDARD
            .decode(&guest_pubkey_b64)
            .map_err(|_| ())
            .and_then(|v| v.try_into().map_err(|_| ()))
        {
            Ok(b) => b,
            // 预认证阶段（proof 尚未校验）：坏 base64 只可能是恶意或损坏的对端，
            // 仅断开该连接即可，绝不能 reset 整个 host 监听（防可用性 DoS）。
            Err(_) => return,
        };
        PublicKey::from(bytes)
    };
    // 派生会话密钥 + 转录。proof 是「DH + 配对码」的函数：校验它即校验配对码，
    // 且不在线上暴露任何可离线穷举的码函数（v3 code_claim 的替代）。
    let key = derive_session_key(&host_secret, &guest_public, expected_code);
    let transcript = transcript_hash(
        LAN_PROTOCOL_VERSION,
        &host_name,
        &host_public,
        &guest_name,
        &guest_public,
    );
    if guest_proof(&key, &transcript) != guest_proof_value {
        let delay = manager.pair_guard().record_failure(ip, std::time::Instant::now());
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let _ = conn
            .write_message(&LanMessage::PairRejected { reason: PairRejectReason::WrongCode }, None)
            .await;
        return;
    }
    manager.pair_guard().record_success(ip);

    // 原子配对门：Hosting → WaitingPair + 预留 oneshot，一次 lock 完成（沿用 v3 修复）。
    let Some(rx) = manager.try_begin_pairing() else {
        manager.emit_guest_rejected(guest_name.clone(), manager.snapshot().status);
        let _ = conn
            .write_message(&LanMessage::PairRejected { reason: PairRejectReason::HostBusy }, None)
            .await;
        return;
    };

    // 询问前端用户是否接受配对
    let guest_id = code_hash(&guest_name);
    manager.emit_pair_request(guest_id, guest_name.clone());

    let accepted = match rx.await {
        Ok(v) => v,
        Err(_) => false, // sender 被 drop 视作拒绝
    };
    if !accepted {
        let _ = conn
            .write_message(&LanMessage::PairRejected { reason: PairRejectReason::Declined }, None)
            .await;
        manager.resume_hosting();
        return;
    }

    // 接受：回转录绑定的认证标签（证明持码 + 握手指纹）。
    if conn
        .write_message(
            &LanMessage::PairAccepted {
                host_device_name: host_name.clone(),
                auth_tag: Some(host_transcript_tag(&key, &transcript)),
            },
            None,
        )
        .await
        .is_err()
    {
        manager.reset_to_idle("连接已断开".to_string());
        return;
    }

    let raw = conn.into_stream();
    let Some(control_rx) = manager.take_control_rx() else {
        manager.reset_to_idle("内部状态错误".to_string());
        return;
    };
    run_session_loop(SecureConnection::new(raw, key), manager.clone(), store.clone(), guest_name, control_rx).await;
}

/// 绑定指定端口（0 = 随机端口）；被占即返回端口占用错误，
/// 由 `lan_create_session` 检测占用进程后弹窗让用户处理。
///
/// 保持同步、不触碰 tokio reactor —— 这样 `bind_tcp` 在纯 `#[test]` 中也可调用；
/// 调用方（`start_host`，async 上下文）再自行 `TcpListener::from_std` 切非阻塞。
fn bind_tcp(port: u16) -> Result<(std::net::TcpListener, u16), String> {
    match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(listener) => {
            // tokio 的 from_std 不会代为切换非阻塞；macOS kqueue 上注册阻塞 fd
            // 会直接 panic（tokio#7172），Windows 的 IOCP 不检查所以此前未暴露。
            listener
                .set_nonblocking(true)
                .map_err(|error| format!("无法切换非阻塞模式：{error}"))?;
            let actual = listener.local_addr().map_err(|error| error.to_string())?.port();
            Ok((listener, actual))
        }
        Err(error) => Err(format!("端口 {port} 被占用：{error}")),
    }
}

/// 拼接本机主要出口 IP 与端口，作为对端可拨的 listen_addr。
pub(crate) fn local_ip_with_port(port: u16) -> String {
    let ip = local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    format!("{}:{}", ip, port)
}

/// 用 UDP "connect" 技巧取得本机主要出口 IP（不真正发包）。
fn local_ip() -> Option<String> {
    use std::net::UdpSocket as StdUdp;
    let sock = StdUdp::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_tcp_returns_listener() {
        let (listener, port) = bind_tcp(LAN_TCP_BASE_PORT).unwrap();
        assert_eq!(port, LAN_TCP_BASE_PORT);
        drop(listener);
    }

    #[test]
    fn bind_tcp_port_zero_picks_ephemeral_port() {
        let (listener, port) = bind_tcp(0).unwrap();
        assert_ne!(port, 0, "port 0 should be replaced by an ephemeral port");
        drop(listener);
    }

    #[test]
    fn local_ip_with_port_has_port() {
        let addr = local_ip_with_port(12345);
        assert!(addr.ends_with(":12345"));
    }
}
