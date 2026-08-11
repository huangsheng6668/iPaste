use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const LAN_TCP_BASE_PORT: u16 = 45130;
pub(crate) const LAN_TCP_PORT_ATTEMPTS: usize = 6;
pub(crate) const LAN_UDP_PORT: u16 = 45131;
pub(crate) const LAN_MAX_PAYLOAD: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum LanMessage {
    Handshake {
        code: String,
        #[serde(rename = "deviceName")]
        device_name: String,
        #[serde(default)]
        auto: bool,
    },
    PairAccepted {
        #[serde(rename = "hostDeviceName")]
        host_device_name: String,
    },
    PairRejected,
    ClipPush {
        #[serde(rename = "clipType")]
        clip_type: String,
        empty: bool,
    },
    ClipRequest,
    ClipResponse {
        #[serde(rename = "clipType")]
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
        // 模拟老版本 Guest 不发 auto 字段的 JSON
        let json = r#"{"kind":"handshake","code":"c","deviceName":"d"}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        let msg = decode_frame(&bytes).unwrap();
        match msg {
            LanMessage::Handshake { auto, .. } => assert!(!auto, "legacy handshake must default auto=false"),
            _ => panic!("wrong variant"),
        }
    }
}
