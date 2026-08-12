use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const LAN_TCP_BASE_PORT: u16 = 45130;
pub(crate) const LAN_MAX_PAYLOAD: usize = 64 * 1024 * 1024;

/// `PairRejected` 的具体原因。向后兼容：老版本 host 发的 `PairRejected` 无此字段，
/// guest 侧 `#[serde(default)]` 得到 `Unknown`，仍按旧行为提示。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PairRejectReason {
    /// 匹配码错误。
    WrongCode,
    /// host 不在 Hosting 态（已在会话中 / 正在配对 / 会话已结束）。
    HostBusy,
    /// host 用户在配对弹窗点了拒绝。
    Declined,
    /// 老版本 / 未知原因。
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum LanMessage {
    Handshake {
        code: String,
        device_name: String,
    },
    PairAccepted {
        host_device_name: String,
    },
    PairRejected {
        #[serde(default)]
        reason: PairRejectReason,
    },
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
        let msg = LanMessage::Handshake { code: "abc".into(), device_name: "MBP".into() };
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
    fn pair_rejected_roundtrips_with_reason() {
        let msg = LanMessage::PairRejected { reason: PairRejectReason::HostBusy };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    /// 老版本 host 发的 PairRejected 不带 reason 字段；新版本 guest 必须兼容，
    /// reason 默认为 Unknown（保证 v0.3.18 新版 ↔ 旧版混合时不会反序列化失败）。
    #[test]
    fn pair_rejected_legacy_without_reason_defaults_unknown() {
        let json = r#"{"kind":"pairRejected"}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        match decode_frame(&bytes).unwrap() {
            LanMessage::PairRejected { reason } => {
                assert_eq!(reason, PairRejectReason::Unknown);
            }
            _ => panic!("wrong variant"),
        }
    }
}
