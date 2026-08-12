//! Guest 客户端：通过 TCP 子网扫描发现 host 或直连 IP，握手后进入会话循环。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::{mpsc, Semaphore};

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::*;
use crate::store::Store;

/// 推断本机主要出口 IPv4 地址（UDP "connect" 8.8.8.8 技巧，不真正发包），
/// 供 `tcp_scan` 推断子网。失败返回 None。
///
/// 注：`server.rs` 的 `local_ip()` 是私有 `fn` 且返回 `Option<String>`，
/// 这里不复用它；本 helper 直接返回 `Option<Ipv4Addr>`，省去调用点 parse。
fn local_ipv4_addr() -> Option<std::net::Ipv4Addr> {
    use std::net::UdpSocket as StdUdp;
    let sock = StdUdp::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    match sock.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) => Some(v4),
        std::net::IpAddr::V6(_) => None,
    }
}

/// 与 host 握手：发 Handshake → 读 PairAccepted/PairRejected → 进 session loop。
///
/// 失败分支统一 `emit_join_failed` + `reset_to_idle`，不留半态。
async fn handshake(
    stream: TcpStream,
    manager: &Arc<LanSessionManager>,
    store: &Store,
    code: &str,
    auto: bool,
) {
    let mut conn = Connection::new(stream);
    // 本机设备名（区别于握手响应里 host 回传的 host_device_name）
    let local_name = device_name();
    let msg = LanMessage::Handshake {
        code: code.to_string(),
        device_name: local_name,
        auto,
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
    handshake(stream, &manager, &store, &code, false).await;
}

/// 纯 TCP 子网扫描：并发探测 1..=254 × [45130]，
/// 连上发 Discover，收到 DiscoverResponse 即识别为 iPaste Host。
pub(crate) async fn tcp_scan() -> Vec<LanDevice> {
    let Some(ip) = local_ipv4_addr() else {
        return Vec::new();
    };
    let parts = ip.to_string().split('.').map(|s| s.to_string()).collect::<Vec<_>>();
    if parts.len() != 4 {
        return Vec::new();
    }
    let subnet = format!("{}.{}.{}", parts[0], parts[1], parts[2]);
    let ports = [LAN_TCP_BASE_PORT];

    let semaphore = Arc::new(Semaphore::new(16));
    let mut tasks = Vec::new();
    for i in 1..=254u16 {
        let semaphore = semaphore.clone();
        let subnet = subnet.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            for port in ports {
                let addr = format!("{subnet}.{i}:{port}");
                let Ok(Ok(stream)) = tokio::time::timeout(
                    Duration::from_millis(150),
                    TcpStream::connect(addr.as_str()),
                )
                .await
                else {
                    continue;
                };
                if let Some(device) = probe_discover(stream, format!("{subnet}.{i}"), port).await {
                    return Some(device);
                }
            }
            None
        }));
    }

    let mut found: HashMap<String, LanDevice> = HashMap::new();
    for task in tasks {
        if let Ok(Some(device)) = task.await {
            found.entry(device.addr.clone()).or_insert(device);
        }
    }
    found.into_values().collect()
}

/// 对已连接 stream 发 Discover，读 DiscoverResponse；失败/非 Host 返回 None。
async fn probe_discover(stream: TcpStream, ip: String, port: u16) -> Option<LanDevice> {
    let mut conn = Connection::new(stream);
    if conn.write_message(&LanMessage::Discover, None).await.is_err() {
        return None;
    }
    let (msg, _) = tokio::time::timeout(Duration::from_millis(2000), conn.read_message())
        .await
        .ok()?
        .ok()?;
    match msg {
        LanMessage::DiscoverResponse { device_name, tcp_port } => {
            let effective_port = if tcp_port > 0 { tcp_port } else { port };
            let addr = format!("{ip}:{effective_port}");
            Some(LanDevice { device_name, addr })
        }
        _ => None,
    }
}

/// 自动扫描后直连：与 `join_by_address` 类似，但握手发送 `auto: true` 且 code 为空，
/// 触发 host 端的自动接受分支（host 仍在 Hosting 态即自动放行）。
pub(crate) async fn join_scanned(manager: Arc<LanSessionManager>, store: Store, addr: String) {
    let (control_tx, control_rx) = mpsc::channel::<ControlMsg>(16);
    manager.set_joining(String::new(), control_tx, control_rx);
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
    handshake(stream, &manager, &store, "", true).await;
}

#[cfg(test)]
mod tests {
    use super::probe_discover;
    use crate::lan_sync::protocol::LanMessage;
    use crate::lan_sync::session::Connection;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn tcp_scan_finds_discoverable_host() {
        // mock Host：监听一个端口，读 Discover → 回 DiscoverResponse
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let (msg, _) = conn.read_message().await.unwrap();
            assert!(matches!(msg, LanMessage::Discover));
            conn.write_message(
                &LanMessage::DiscoverResponse {
                    device_name: "MockHost".into(),
                    tcp_port: mock_port,
                },
                None,
            )
            .await
            .unwrap();
        });

        // 直接测 probe_discover：连 mock 端口发 Discover 收响应
        let stream = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            TcpStream::connect(("127.0.0.1", mock_port)),
        )
        .await
        .unwrap()
        .unwrap();
        let device = probe_discover(stream, "127.0.0.1".to_string(), mock_port)
            .await
            .expect("probe should find MockHost");
        assert_eq!(device.device_name, "MockHost");
        assert!(device.addr.ends_with(&format!(":{mock_port}")));
    }
}
