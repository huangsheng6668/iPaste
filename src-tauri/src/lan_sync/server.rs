//! Host 服务端：绑定 TCP 监听 + accept 循环 + 配对确认。
//!
//! accept 后按首条消息分流：Handshake → 配对流程；其余静默关闭。

use std::net::IpAddr;
use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::*;
use crate::store::Store;

/// 启动 Host：绑定 TCP + accept 循环。
///
/// 成功返回 `listen_addr`（`ip:port`）。accept 任务在后台 spawn，
/// 失败时由任务自身通过 `manager.reset_to_idle` 清理状态。
pub(crate) async fn start_host(
    app: AppHandle,
    manager: Arc<LanSessionManager>,
    store: Store,
    code: String,
) -> Result<String, String> {
    // 1. 选可用 TCP 端口（同步绑定，再转为 tokio listener）
    let (std_listener, tcp_port) = bind_tcp()?;
    let listener = TcpListener::from_std(std_listener)
        .map_err(|e| format!("无法切换非阻塞模式：{}", e))?;
    let listen_addr = local_ip_with_port(tcp_port);

    // 2. 注册到 manager（control_rx 也存进去，握手通过后由 handle_guest_with_handshake 取出）
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_hosting(code.clone(), listen_addr.clone(), control_tx, control_rx);

    // 3. accept 循环：对每个新连接读首条消息分流。Handshake → 进入配对流程。
    let accept_handle = {
        let manager = manager.clone();
        let app = app.clone();
        let store = store.clone();
        let expected_code = code.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = listener.accept().await else { continue };
                let ip = peer.ip();
                let manager = manager.clone();
                let app = app.clone();
                let store = store.clone();
                let expected_code = expected_code.clone();
                manager.pair_guard().prune(std::time::Instant::now());
                tokio::spawn(async move {
                    let mut conn = Connection::new(stream);
                    let Ok((msg, _)) = conn.read_message().await else {
                        return;
                    };
                    match msg {
                        LanMessage::Handshake { version, code_claim, device_name: guest_name, guest_pubkey } => {
                            // 老客户端 / 畸形帧：无 claim 或公钥 → 拒绝（老客户端收到
                            // PairRejected 后提示"匹配码错误或被拒绝"）。
                            if version != LAN_PROTOCOL_VERSION
                                || code_claim.is_none()
                                || guest_pubkey.is_none()
                            {
                                let _ = conn
                                    .write_message(&LanMessage::PairRejected { reason: PairRejectReason::Unknown }, None)
                                    .await;
                                return;
                            }
                            handle_guest_with_handshake(
                                conn,
                                &manager,
                                &app,
                                &store,
                                &expected_code,
                                code_claim.unwrap(),
                                guest_name,
                                guest_pubkey.unwrap(),
                                ip,
                            )
                            .await;
                        }
                        _ => { /* 未知消息，静默关闭 */ }
                    }
                });
            }
        })
    };

    // 4. 把 accept 任务句柄存进 manager，Task 6 的 disconnect 命令会 abort 它以释放端口。
    manager.set_host_task(accept_handle);

    Ok(listen_addr)
}

/// 处理 Handshake 已读的连接：校验 code → 询问用户 → 进入 session loop 或拒绝。
///
/// 调用方（accept 循环 handler）已读取首条消息并解构为 Handshake 字段，
/// 传入已构造好的 `Connection`。函数体从原 `handle_guest` 的 code 校验开始，
/// 逻辑不变。
async fn handle_guest_with_handshake(
    mut conn: Connection,
    manager: &Arc<LanSessionManager>,
    app: &AppHandle,
    store: &Store,
    expected_code: &str,
    claim: String,
    guest_name: String,
    guest_pubkey_b64: String,
    ip: IpAddr,
) {
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{code_claim, derive_session_key, generate_pair_keys, host_auth_tag, SecureConnection};

    // 防爆破：封禁期直接丢弃；claim 错记失败并做指数退避；正确则清计数。
    if manager.pair_guard().is_blocked(ip, std::time::Instant::now()) {
        return;
    }
    if claim != code_claim(expected_code) {
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

    // 原子配对门：Hosting → WaitingPair + 预留 oneshot，一次 lock 完成（修 TOCTOU）。
    // 已有配对进行中 / 已连接时直接拒绝，不破坏现有状态。
    let Some(rx) = manager.try_begin_pairing() else {
        // host 不在 Hosting 态（已在会话中 / 正在配对）。这是"扫描加入却被报码错"
        // 的真因——emit host 侧诊断事件暴露当前状态，供前端提示 + 定位 Bug B。
        let _ = app.emit(
            "ipaste://lan-guest-rejected",
            LanGuestRejected {
                guest_device_name: guest_name.clone(),
                host_status: manager.snapshot().status,
            },
        );
        let _ = conn
            .write_message(&LanMessage::PairRejected { reason: PairRejectReason::HostBusy }, None)
            .await;
        return;
    };

    // 询问前端用户是否接受配对
    let guest_id = code_hash(&guest_name);
    let _ = app.emit(
        "ipaste://lan-pair-request",
        LanPairRequest {
            guest_id,
            device_name: guest_name.clone(),
        },
    );

    let accepted = match rx.await {
        Ok(v) => v,
        Err(_) => false, // sender 被 drop 视作拒绝
    };
    if !accepted {
        let _ = conn
            .write_message(&LanMessage::PairRejected { reason: PairRejectReason::Declined }, None)
            .await;
        // 回到 Hosting（持久 host 会话），不停掉整个 host —— 下一个 guest 仍可接入。
        manager.resume_hosting();
        return;
    }

    // 接受：解析 guest 公钥 → 派生会话密钥 → 回 PairAccepted（带公钥 + 认证标签）
    let guest_public = {
        let bytes: [u8; 32] = match base64::engine::general_purpose::STANDARD
            .decode(&guest_pubkey_b64)
            .map_err(|_| ())
            .and_then(|v| v.try_into().map_err(|_| ()))
        {
            Ok(b) => b,
            Err(_) => {
                manager.reset_to_idle("握手数据无效".to_string());
                return;
            }
        };
        PublicKey::from(bytes)
    };
    let (host_secret, host_public) = generate_pair_keys();
    let key = derive_session_key(&host_secret, &guest_public, expected_code);
    let host_name = device_name();
    if conn
        .write_message(
            &LanMessage::PairAccepted {
                host_device_name: host_name,
                host_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(host_public.as_bytes())),
                auth_tag: Some(host_auth_tag(&key)),
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

/// 只绑定固定端口 `LAN_TCP_BASE_PORT`（45130）；被占即返回端口占用错误，
/// 由 `lan_create_session` 检测占用进程后弹窗让用户处理。
///
/// 保持同步、不触碰 tokio reactor —— 这样 `bind_tcp` 在纯 `#[test]` 中也可调用；
/// 调用方（`start_host`，async 上下文）再自行 `TcpListener::from_std` 切非阻塞。
fn bind_tcp() -> Result<(std::net::TcpListener, u16), String> {
    match std::net::TcpListener::bind(("0.0.0.0", LAN_TCP_BASE_PORT)) {
        Ok(listener) => Ok((listener, LAN_TCP_BASE_PORT)),
        Err(error) => Err(format!(
            "端口 {} 被占用：{error}",
            LAN_TCP_BASE_PORT
        )),
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
        let (listener, port) = bind_tcp().unwrap();
        assert_eq!(port, LAN_TCP_BASE_PORT);
        drop(listener);
    }

    #[test]
    fn local_ip_with_port_has_port() {
        let addr = local_ip_with_port(12345);
        assert!(addr.ends_with(":12345"));
    }
}
