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

/// 枚举本机所有真实 IPv4 接口地址（滤除 loopback 与 169.254/16 link-local）。
///
/// 取代旧的 UDP-connect-8.8.8.8 单 IP 推断：那种方法在多网卡（Windows 上常见
/// Wi-Fi + WSL/Hyper-V/Docker/VMware 虚拟网卡）或无默认路由场景下只能取到一个、
/// 甚至取不到 IP，导致 `tcp_scan` 推断的子网不是 peer 所在子网，从而扫不到 host。
/// 枚举真实接口能覆盖本机所有本地子网。
fn local_ipv4_addrs() -> Vec<std::net::Ipv4Addr> {
    let Ok(entries) = local_ip_address::list_afinet_netifas() else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|(_, ip)| match ip {
            std::net::IpAddr::V4(v4) => Some(v4),
            std::net::IpAddr::V6(_) => None,
        })
        .filter(|ip| !ip.is_loopback() && !is_link_local(*ip))
        .collect()
}

/// 169.254.0.0/16（APIPA / link-local）—— DHCP 失败自动分配，扫描它无意义。
fn is_link_local(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 169 && o[1] == 254
}

/// 取 /24 子网前缀（"a.b.c"）。iPaste 的 LAN 场景默认 /24 子网。
fn subnet_prefix(ip: std::net::Ipv4Addr) -> String {
    let o = ip.octets();
    format!("{}.{}.{}", o[0], o[1], o[2])
}

/// 由一组本机 IPv4 地址推导要去扫描的 /24 子网前缀列表（去重，已排除 loopback /
/// link-local）。抽成纯函数便于单测覆盖多网卡去重与过滤逻辑。
fn scan_subnets(addrs: impl IntoIterator<Item = std::net::Ipv4Addr>) -> Vec<String> {
    use std::collections::HashSet;
    addrs
        .into_iter()
        .filter(|ip| !ip.is_loopback() && !is_link_local(*ip))
        .map(subnet_prefix)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
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
    handshake(stream, &manager, &store, &code, false).await;
}

/// 纯 TCP 子网扫描：枚举本机每个 IPv4 接口的 /24 子网（去重），并发探测
/// .1..254 × [45130]，连上发 Discover，收到 DiscoverResponse 即识别为 iPaste Host。
///
/// 多网卡时必须扫每个本地子网——只扫默认路由那张卡会漏掉其它子网里的 host
/// （这正是"手动填 IP 能连、扫描却扫不到"的根因）。
pub(crate) async fn tcp_scan() -> Vec<LanDevice> {
    let subnets = scan_subnets(local_ipv4_addrs());
    if subnets.is_empty() {
        return Vec::new();
    }
    let ports = [LAN_TCP_BASE_PORT];

    let semaphore = Arc::new(Semaphore::new(16));
    let mut tasks = Vec::new();
    for subnet in subnets {
        for i in 1..=254u16 {
            let semaphore = semaphore.clone();
            let subnet = subnet.clone();
            tasks.push(tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                for port in ports {
                    let host_ip = format!("{subnet}.{i}");
                    let addr = format!("{host_ip}:{port}");
                    let Ok(Ok(stream)) = tokio::time::timeout(
                        Duration::from_millis(150),
                        TcpStream::connect(addr.as_str()),
                    )
                    .await
                    else {
                        continue;
                    };
                    if let Some(device) = probe_discover(stream, host_ip, port).await {
                        return Some(device);
                    }
                }
                None
            }));
        }
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
    use super::{is_link_local, probe_discover, scan_subnets, subnet_prefix};
    use crate::lan_sync::protocol::LanMessage;
    use crate::lan_sync::session::Connection;
    use std::collections::HashSet;
    use std::net::Ipv4Addr;
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

    #[test]
    fn subnet_prefix_takes_first_three_octets() {
        assert_eq!(subnet_prefix(Ipv4Addr::new(192, 168, 1, 42)), "192.168.1");
        assert_eq!(subnet_prefix(Ipv4Addr::new(10, 0, 0, 1)), "10.0.0");
    }

    #[test]
    fn is_link_local_detects_apipa() {
        assert!(is_link_local(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!is_link_local(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_link_local(Ipv4Addr::new(127, 0, 0, 1)));
    }

    /// 多网卡场景：Wi-Fi 192.168.1.x + 虚拟网卡 192.168.99.x + 同子网重复 +
    /// loopback + APIPA。期望只扫两个真实子网，且去重、排除无效地址。
    #[test]
    fn scan_subnets_dedupes_and_filters_multi_adapter() {
        let addrs = vec![
            Ipv4Addr::new(192, 168, 1, 5),    // Wi-Fi
            Ipv4Addr::new(192, 168, 1, 6),    // 同子网，应去重
            Ipv4Addr::new(192, 168, 99, 1),   // 虚拟网卡，另一子网
            Ipv4Addr::new(127, 0, 0, 1),      // loopback，排除
            Ipv4Addr::new(169, 254, 10, 20),  // APIPA，排除
        ];
        let got: HashSet<String> = scan_subnets(addrs).into_iter().collect();
        let want: HashSet<String> = ["192.168.1".to_string(), "192.168.99".to_string()]
            .into_iter()
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn scan_subnets_empty_when_only_loopback() {
        assert!(scan_subnets(vec![Ipv4Addr::new(127, 0, 0, 1)]).is_empty());
        assert!(scan_subnets(Vec::<Ipv4Addr>::new()).is_empty());
    }
}
