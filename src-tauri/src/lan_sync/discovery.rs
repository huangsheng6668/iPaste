//! 局域网发现：host 邀请存活期间每 2s UDP 广播；guest 常驻监听维护附近设备表。
//! 广播包不含 PIN、不含票据秘密；地址只留在后端缓存，永不透给前端。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::events::{NearbyDevicesChanged, EVENT_NEARBY_DEVICES};
use crate::lan_sync::LanEventSink;
use crate::models::NearbyDevice;
use crate::util::LockRecover;

pub(crate) const DISCOVERY_PORT: u16 = 42691;
pub(crate) const DISCOVERY_MAGIC: &str = "ipaste-discovery-v1";
pub(crate) const BROADCAST_INTERVAL: Duration = Duration::from_secs(2);
pub(crate) const NEARBY_TTL: Duration = Duration::from_secs(8);
pub(crate) const MAX_NEARBY: usize = 16;
const MAX_PACKET_BYTES: usize = 2048;
const MAX_ADDRS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveryPacket {
    pub(crate) magic: String,
    pub(crate) device_name: String,
    pub(crate) endpoint_id: String,
    pub(crate) addrs: Vec<String>,
    pub(crate) relay: Option<String>,
}

impl DiscoveryPacket {
    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

/// 严格校验后再入表：伪造包最多刷列表，不能塞进非法地址触发后续拨号。
pub(crate) fn decode_packet(bytes: &[u8]) -> Option<DiscoveryPacket> {
    if bytes.len() > MAX_PACKET_BYTES {
        return None;
    }
    let packet: DiscoveryPacket = serde_json::from_slice(bytes).ok()?;
    if packet.magic != DISCOVERY_MAGIC
        || packet.endpoint_id.len() != 64
        || !packet.endpoint_id.bytes().all(|b| b.is_ascii_hexdigit())
        || packet.addrs.len() > MAX_ADDRS
        || packet
            .addrs
            .iter()
            .any(|a| a.parse::<SocketAddr>().is_err())
        || packet.relay.as_deref().is_some_and(|r| r.len() > 512)
    {
        return None;
    }
    Some(packet)
}

#[derive(Debug, Clone)]
pub(crate) struct NearbyEntry {
    pub(crate) device_name: String,
    pub(crate) endpoint_id: String,
    pub(crate) addrs: Vec<String>,
    pub(crate) relay: Option<String>,
    pub(crate) last_seen: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct NearbyTable {
    entries: Vec<NearbyEntry>,
    self_id: String,
}

impl NearbyTable {
    pub(crate) fn new(self_id: String) -> Self {
        Self {
            entries: Vec::new(),
            self_id,
        }
    }

    /// 返回「展示列表是否变化」，决定要不要 emit（同名同 id 的刷新不发事件）。
    pub(crate) fn upsert(&mut self, packet: DiscoveryPacket, now: Instant) -> bool {
        if packet.endpoint_id == self.self_id {
            return false;
        }
        let before = self.visible_signature();
        self.prune(now);
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.endpoint_id == packet.endpoint_id)
        {
            entry.device_name = packet.device_name;
            entry.addrs = packet.addrs;
            entry.relay = packet.relay;
            entry.last_seen = now;
        } else {
            self.entries.push(NearbyEntry {
                device_name: packet.device_name,
                endpoint_id: packet.endpoint_id,
                addrs: packet.addrs,
                relay: packet.relay,
                last_seen: now,
            });
        }
        self.enforce_cap();
        before != self.visible_signature()
    }

    pub(crate) fn prune(&mut self, now: Instant) -> bool {
        let before = self.visible_signature();
        self.entries
            .retain(|e| now.duration_since(e.last_seen) < NEARBY_TTL);
        before != self.visible_signature()
    }

    fn enforce_cap(&mut self) {
        if self.entries.len() <= MAX_NEARBY {
            return;
        }
        self.entries.sort_by_key(|e| e.last_seen);
        let excess = self.entries.len() - MAX_NEARBY;
        self.entries.drain(..excess);
    }

    /// 拥有所有权的签名：借用 &self 的返回值会与后续可变借用冲突。
    fn visible_signature(&self) -> Vec<(String, String)> {
        let mut out: Vec<_> = self
            .entries
            .iter()
            .map(|e| (e.endpoint_id.clone(), e.device_name.clone()))
            .collect();
        out.sort_unstable();
        out
    }

    /// 前端可见项：只有名字 + id；按 id 排序保证列表不跳。
    pub(crate) fn list(&self) -> Vec<NearbyDevice> {
        let mut devices: Vec<_> = self
            .entries
            .iter()
            .map(|e| NearbyDevice {
                device_name: e.device_name.clone(),
                endpoint_id: e.endpoint_id.clone(),
            })
            .collect();
        devices.sort_by(|a, b| a.endpoint_id.cmp(&b.endpoint_id));
        devices
    }

    /// 拨号地址（含 TTL 校验），仅后端 join_nearby 使用。
    pub(crate) fn peer(
        &mut self,
        endpoint_id: &str,
        now: Instant,
    ) -> Option<(Vec<String>, Option<String>)> {
        self.prune(now);
        self.entries
            .iter()
            .find(|e| e.endpoint_id == endpoint_id)
            .map(|e| (e.addrs.clone(), e.relay.clone()))
    }

    #[cfg(test)]
    pub(crate) fn insert_for_test(&mut self, packet: DiscoveryPacket, now: Instant) {
        self.upsert(packet, now);
    }
}

pub(crate) struct Discovery {
    pub(crate) table: Mutex<NearbyTable>,
}

impl Discovery {
    pub(crate) fn new(self_id: String) -> Self {
        Self {
            table: Mutex::new(NearbyTable::new(self_id)),
        }
    }
}

pub(crate) fn emit_nearby(sink: &dyn LanEventSink, discovery: &Discovery) {
    let devices = discovery.table.lock_recover("nearby").list();
    let value =
        serde_json::to_value(NearbyDevicesChanged { devices }).unwrap_or(serde_json::Value::Null);
    sink.emit(EVENT_NEARBY_DEVICES, &value);
}

/// host 侧：邀请存活期间每 2s 广播一次固定快照；still_active 为 false 即停
///（取消 / 过期 / 核销 / 被新邀请覆盖）。端口异常只记 warning，不打断配对。
pub(crate) fn spawn_broadcast_loop<F>(
    packet: DiscoveryPacket,
    still_active: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> bool + Send + 'static,
{
    tokio::spawn(async move {
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!(
                    "[lan-sync] 发现广播 socket 创建失败（PIN 仍可用，票据路径不受影响）：{error}"
                );
                return;
            }
        };
        if let Err(error) = socket.set_broadcast(true) {
            eprintln!("[lan-sync] 无法开启广播选项（PIN 仍可用）：{error}");
            return;
        }
        let bytes = packet.encode();
        let target = format!("255.255.255.255:{DISCOVERY_PORT}");
        loop {
            if !still_active() {
                break;
            }
            if let Err(error) = socket.send_to(&bytes, &target).await {
                eprintln!("[lan-sync] 发现广播发送失败（已忽略）：{error}");
            }
            tokio::time::sleep(BROADCAST_INTERVAL).await;
        }
    })
}

/// guest 侧：应用启动即常驻监听固定端口；列表变化才 emit，1s tick 做 TTL 清理。
/// 端口被占 / 防火墙拦 UDP 时仅记 warning，附近列表为空，票据路径不受影响。
pub(crate) fn spawn_listener(
    discovery: Arc<Discovery>,
    sink: Arc<dyn LanEventSink>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let socket = match UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!(
                    "[lan-sync] 发现监听端口 {DISCOVERY_PORT} 不可用（附近设备列表将为空）：{error}"
                );
                return;
            }
        };
        let mut buffer = [0u8; MAX_PACKET_BYTES];
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                incoming = socket.recv_from(&mut buffer) => {
                    let Ok((len, _)) = incoming else { continue };
                    let Some(packet) = decode_packet(&buffer[..len]) else { continue };
                    let changed = discovery
                        .table
                        .lock_recover("nearby")
                        .upsert(packet, Instant::now());
                    if changed {
                        emit_nearby(sink.as_ref(), discovery.as_ref());
                    }
                }
                _ = ticker.tick() => {
                    let changed = discovery.table.lock_recover("nearby").prune(Instant::now());
                    if changed {
                        emit_nearby(sink.as_ref(), discovery.as_ref());
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(id: &str) -> DiscoveryPacket {
        DiscoveryPacket {
            magic: DISCOVERY_MAGIC.to_string(),
            device_name: "MacBook Pro".to_string(),
            endpoint_id: id.to_string(),
            addrs: vec!["192.168.1.5:52123".to_string()],
            relay: Some("https://relay.example.com".to_string()),
        }
    }

    fn hex_id(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    #[test]
    fn packet_roundtrips() {
        assert_eq!(
            decode_packet(&packet(&hex_id(7)).encode()).unwrap(),
            packet(&hex_id(7))
        );
    }

    #[test]
    fn decode_rejects_bad_packets() {
        assert!(decode_packet(b"not json").is_none());
        assert!(decode_packet(&vec![0u8; MAX_PACKET_BYTES + 1]).is_none());
        let mut wrong_magic = packet(&hex_id(1));
        wrong_magic.magic = "evil".into();
        assert!(decode_packet(&wrong_magic.encode()).is_none());
        let mut bad_id = packet(&hex_id(1));
        bad_id.endpoint_id = "zz".into();
        assert!(decode_packet(&bad_id.encode()).is_none());
        let mut bad_addr = packet(&hex_id(1));
        bad_addr.addrs = vec!["not-a-socket".into()];
        assert!(decode_packet(&bad_addr.encode()).is_none());
        let mut too_many = packet(&hex_id(1));
        too_many.addrs = (0..MAX_ADDRS + 1)
            .map(|_| "192.168.1.5:52123".to_string())
            .collect();
        assert!(decode_packet(&too_many.encode()).is_none());
    }

    #[test]
    fn table_ignores_self_broadcast() {
        let mut table = NearbyTable::new(hex_id(1));
        let now = Instant::now();
        assert!(!table.upsert(packet(&hex_id(1)), now));
        assert!(table.list().is_empty());
    }

    #[test]
    fn refresh_of_same_device_does_not_signal_change() {
        let mut table = NearbyTable::new(hex_id(9));
        let now = Instant::now();
        assert!(table.upsert(packet(&hex_id(1)), now));
        assert!(!table.upsert(packet(&hex_id(1)), now + Duration::from_secs(1)));
        assert_eq!(table.list().len(), 1);
    }

    #[test]
    fn table_prunes_after_ttl_and_signals_change() {
        let mut table = NearbyTable::new(hex_id(9));
        let now = Instant::now();
        table.upsert(
            packet(&hex_id(1)),
            now - NEARBY_TTL - Duration::from_secs(1),
        );
        assert_eq!(table.list().len(), 1);
        assert!(table.prune(now));
        assert!(table.list().is_empty());
    }

    #[test]
    fn table_caps_at_16_by_oldest_last_seen() {
        let mut table = NearbyTable::new(hex_id(9));
        let now = Instant::now();
        // 时间戳必须全部落在 8s TTL 内，否则 prune 会先把旧条目按 TTL 清掉，
        // 测不到 MAX_NEARBY 的容量淘汰。
        for i in 0..20u8 {
            table.upsert(
                packet(&hex_id(i)),
                now - Duration::from_millis(20 - i as u64),
            );
        }
        let list = table.list();
        assert_eq!(list.len(), MAX_NEARBY);
        assert!(
            list.iter().all(|d| d.endpoint_id != hex_id(0)),
            "最旧条目应被淘汰"
        );
    }

    #[test]
    fn list_is_sorted_and_peer_returns_addrs_with_ttl_check() {
        let mut table = NearbyTable::new(hex_id(9));
        let now = Instant::now();
        table.upsert(packet(&hex_id(3)), now);
        table.upsert(packet(&hex_id(1)), now);
        let list = table.list();
        assert_eq!(list[0].endpoint_id, hex_id(1));
        assert_eq!(list[0].device_name, "MacBook Pro");
        let (addrs, relay) = table.peer(&hex_id(1), now).unwrap();
        assert_eq!(addrs, vec!["192.168.1.5:52123".to_string()]);
        assert_eq!(relay.as_deref(), Some("https://relay.example.com"));
        assert!(table
            .peer(&hex_id(1), now + NEARBY_TTL + Duration::from_secs(1))
            .is_none());
    }
}
