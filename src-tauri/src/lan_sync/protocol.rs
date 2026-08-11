use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::Ipv4Addr;

pub(crate) const LAN_TCP_BASE_PORT: u16 = 45130;
pub(crate) const LAN_TCP_PORT_ATTEMPTS: usize = 6;
pub(crate) const LAN_UDP_PORT: u16 = 45131;
pub(crate) const LAN_MAX_PAYLOAD: usize = 64 * 1024 * 1024;
/// 设备发现组播地址（IANA 管理范围 239.255.0.0/16；端口复用 LAN_UDP_PORT）
pub(crate) const LAN_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 42, 99);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum LanMessage {
    Handshake {
        code: String,
        device_name: String,
        #[serde(default)]
        auto: bool,
    },
    PairAccepted {
        host_device_name: String,
    },
    PairRejected,
    ClipPush {
        clip_type: String,
        empty: bool,
    },
    ClipRequest,
    ClipResponse {
        clip_type: String,
        empty: bool,
    },
    Disconnect,
}

pub(crate) fn code_hash(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.trim().as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{:02x}", b)).collect::<String>()
}

pub(crate) fn encode_frame(msg: &LanMessage) -> Result<Vec<u8>, String> {
    let header = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    if header.len() > LAN_MAX_PAYLOAD {
        return Err("帧头过大".to_string());
    }
    let mut out = Vec::with_capacity(4 + header.len());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    Ok(out)
}

pub(crate) fn decode_frame(bytes: &[u8]) -> Result<LanMessage, String> {
    if bytes.len() < 4 {
        return Err("帧不完整".to_string());
    }
    let header_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if header_len > LAN_MAX_PAYLOAD {
        return Err("帧头超过上限".to_string());
    }
    if bytes.len() < 4 + header_len {
        return Err("帧不完整".to_string());
    }
    serde_json::from_slice(&bytes[4..4 + header_len]).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_hash_is_stable_and_truncated() {
        let h = code_hash("room42");
        assert_eq!(h.len(), 16);
        assert_eq!(h, code_hash("room42"));
        assert_ne!(h, code_hash("room43"));
    }

    #[test]
    fn handshake_roundtrips() {
        let msg = LanMessage::Handshake { code: "abc".into(), device_name: "MBP".into(), auto: false };
        let bytes = encode_frame(&msg).unwrap();
        let decoded = decode_frame(&bytes).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn clip_push_with_empty_roundtrips() {
        let msg = LanMessage::ClipPush { clip_type: "text".into(), empty: true };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    #[test]
    fn reject_truncated_frame() {
        assert!(decode_frame(&[1, 2, 3]).is_err());
    }

    #[test]
    fn reject_oversized_header_length() {
        let mut bytes = (LAN_MAX_PAYLOAD as u32 + 1).to_le_bytes().to_vec();
        bytes.push(0);
        assert!(decode_frame(&bytes).is_err());
    }

    #[test]
    fn handshake_auto_true_roundtrips() {
        let msg = LanMessage::Handshake {
            code: "c".into(),
            device_name: "d".into(),
            auto: true,
        };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    #[test]
    fn handshake_legacy_without_auto_defaults_false() {
        // 模拟 0.3.14 老版本 Guest 不发 auto 字段的 JSON。
        // 注意：字段名必须是 snake_case（device_name）——这是 0.3.14 实际的线上格式。
        // enum 上的 rename_all="camelCase" 只对 variant 名（kind tag）生效，不作用于
        // struct variant 内部字段。
        let json = r#"{"kind":"handshake","code":"c","device_name":"d"}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        let msg = decode_frame(&bytes).unwrap();
        match msg {
            LanMessage::Handshake { auto, .. } => assert!(!auto, "legacy handshake must default auto=false"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn lan_multicast_addr_is_management_range() {
        // 239.255.0.0/16 是 IANA 管理范围组播段（局域网自由使用）
        assert_eq!(LAN_MULTICAST_ADDR, Ipv4Addr::new(239, 255, 42, 99));
        assert!(LAN_MULTICAST_ADDR.is_multicast());
    }
}
