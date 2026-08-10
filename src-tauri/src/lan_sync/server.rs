//! Host 服务端：绑定 TCP 监听 + UDP 广播 + accept 循环 + 配对确认。
//!
//! 见 `task-4-brief.md`。本模块在 Task 4 取消注释启用，Task 5/6 才会真正调用 `start_host`。

use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::*;
use crate::store::Store;

/// 启动 Host：绑定 TCP + UDP 广播 + accept 循环。
///
/// 成功返回 `listen_addr`（`ip:port`）。listener / 广播 / accept 三个任务在后台 spawn，
/// 失败时由各任务自身通过 `manager.reset_to_idle` 清理状态。
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

    // 2. 注册到 manager（control_rx 也存进去，握手通过后由 handle_guest 取出）
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_hosting(code.clone(), listen_addr.clone(), control_tx, control_rx);

    let hash = code_hash(&code);

    // 3. UDP 广播任务：周期性向 255.255.255.255:LAN_UDP_PORT 发送 {codeHash, tcpPort}。
    //    捕获 JoinHandle 以便 Task 6 的 disconnect 命令 abort。
    let broadcast_handle = {
        let manager = manager.clone();
        let payload = serde_json::json!({ "codeHash": hash, "tcpPort": tcp_port }).to_string();
        let bcast_addr: std::net::SocketAddr = format!("255.255.255.255:{}", LAN_UDP_PORT)
            .parse()
            .expect("broadcast 地址字面量必然可解析");
        tokio::spawn(async move {
            let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else { return };
            let _ = sock.set_broadcast(true);
            loop {
                if manager.status_is_idle_or_connected_break() {
                    break;
                }
                let _ = sock.send_to(payload.as_bytes(), bcast_addr).await;
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
        })
    };

    // 4. accept 循环：对每个新连接 spawn `handle_guest`，1v1 由 `handle_guest` 内的
    //    `try_begin_pairing()` 原子门保证（不再在 accept 循环里预 check —— 那会造成
    //    check 与 set_waiting_pair 之间的 TOCTOU 竞态）。
    let accept_handle = {
        let manager = manager.clone();
        let app = app.clone();
        let store = store.clone();
        let code = code.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let manager = manager.clone();
                let app = app.clone();
                let store = store.clone();
                let code = code.clone();
                tokio::spawn(async move {
                    handle_guest(stream, manager, app, store, code).await;
                });
            }
        })
    };

    // 5. 把两个任务句柄存进 manager，Task 6 的 disconnect 命令会 abort 它们以释放端口。
    manager.set_host_tasks(broadcast_handle, accept_handle);

    Ok(listen_addr)
}

/// 处理一个 guest 连接：读 Handshake → 校验 code → 询问用户 → 进入 session loop 或拒绝。
async fn handle_guest(
    stream: TcpStream,
    manager: Arc<LanSessionManager>,
    app: AppHandle,
    store: Store,
    expected_code: String,
) {
    let mut conn = Connection::new(stream);

    // 读取握手；读取失败静默丢弃
    let (msg, _payload) = match conn.read_message().await {
        Ok(v) => v,
        Err(_) => return,
    };
    // 注意：字段重命名为 guest_device_name，避免遮蔽模块级 `device_name()` 函数。
    let LanMessage::Handshake { code, device_name: guest_device_name } = msg else { return };

    // code 校验（与 code_hash 一致，trim 后比较）
    if code.trim() != expected_code.trim() {
        let _ = conn.write_message(&LanMessage::PairRejected, None).await;
        return;
    }

    // 原子配对门：Hosting → WaitingPair + 预留 oneshot，一次 lock 完成（修 TOCTOU）。
    // 已有配对进行中 / 已连接时直接拒绝，不破坏现有状态。
    let Some(rx) = manager.try_begin_pairing() else {
        let _ = conn.write_message(&LanMessage::PairRejected, None).await;
        return;
    };

    // 询问前端用户是否接受配对
    let guest_id = code_hash(&guest_device_name);
    let _ = app.emit(
        "ipaste://lan-pair-request",
        LanPairRequest {
            guest_id,
            device_name: guest_device_name.clone(),
        },
    );

    let accepted = match rx.await {
        Ok(v) => v,
        Err(_) => false, // sender 被 drop 视作拒绝
    };
    if !accepted {
        let _ = conn.write_message(&LanMessage::PairRejected, None).await;
        // 回到 Hosting（持久 host 会话），不停掉整个 host —— 下一个 guest 仍可接入。
        manager.resume_hosting();
        return;
    }

    // 接受：写 PairAccepted，转入 session loop
    let host_name = device_name();
    if conn
        .write_message(&LanMessage::PairAccepted { host_device_name: host_name }, None)
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
    run_session_loop(raw, manager, store, guest_device_name, control_rx).await;
}

/// 在 `LAN_TCP_BASE_PORT .. +LAN_TCP_PORT_ATTEMPTS` 范围内尝试绑定，返回首个成功的
/// `std::net::TcpListener`。
///
/// 保持同步、不触碰 tokio reactor —— 这样 `bind_tcp` 在纯 `#[test]` 中也可调用；
/// 调用方（`start_host`，async 上下文）再自行 `TcpListener::from_std` 切非阻塞。
fn bind_tcp() -> Result<(std::net::TcpListener, u16), String> {
    let mut last_err = None;
    for i in 0..LAN_TCP_PORT_ATTEMPTS {
        let port = LAN_TCP_BASE_PORT + i as u16;
        match std::net::TcpListener::bind(("0.0.0.0", port)) {
            Ok(listener) => return Ok((listener, port)),
            Err(e) => last_err = Some(e),
        }
    }
    Err(format!(
        "无法绑定局域网同步端口：{}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
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
        assert!((LAN_TCP_BASE_PORT..LAN_TCP_BASE_PORT + LAN_TCP_PORT_ATTEMPTS as u16).contains(&port));
        drop(listener);
    }

    #[test]
    fn local_ip_with_port_has_port() {
        let addr = local_ip_with_port(12345);
        assert!(addr.ends_with(":12345"));
    }
}
