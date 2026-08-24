//! 协议 v5 消息定义（线格式：明文 JSON header，传输加密由 iroh QUIC TLS 承担）。
//!
//! 帧布局见 frame.rs：`[u32 header_len LE][JSON][u32 payload_len LE][payload]`。

use serde::{Deserialize, Serialize};

pub(crate) const LAN_PROTOCOL_VERSION: u32 = 5;
pub(crate) const LAN_MAX_PAYLOAD: usize = 8 * 1024 * 1024;
pub(crate) const LAN_BATCH_MAX_ITEMS: u32 = 10_000;
pub(crate) const IPASTE_ALPN: &[u8] = b"ipaste/sync/5";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PairRejectReason {
    /// 邀请无效/过期/已用。
    InviteInvalid,
    /// 对端用户在确认弹窗点了拒绝。
    Declined,
    /// 协议版本不符。
    VersionMismatch,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum LanMessage {
    /// 配对请求：仅在「陌生连接的首条双向流」出现。invite_secret 为 16B hex。
    PairRequest {
        #[serde(default = "default_protocol_version")]
        version: u32,
        device_name: String,
        invite_secret: String,
    },
    /// 配对接受：回本端设备名与指纹短码（对端 UI 展示）。
    PairAccept {
        version: u32,
        device_name: String,
        fingerprint: String,
    },
    PairReject { #[serde(default)] reason: PairRejectReason },
    Ping,
    Pong,
    ClipPush {
        clip_type: String,
        empty: bool,
        #[serde(default)]
        category_name: Option<String>,
        #[serde(default)]
        category_color: Option<String>,
        #[serde(default)]
        display_name: Option<String>,
        /// true = 捕获即自动同步（Spec 2 消费；接收端静默入库不弹提示）。
        #[serde(default)]
        auto: bool,
        /// 发起方 EndpointId hex（Spec 2 回环抑制）。
        #[serde(default)]
        origin_node_id: Option<String>,
    },
    CategoryBatchStart { category_name: String, category_color: Option<String>, item_count: u32 },
    CategoryBatchEnd,
    ClipRequest,
    ClipResponse {
        clip_type: String,
        empty: bool,
        #[serde(default)]
        category_name: Option<String>,
        #[serde(default)]
        category_color: Option<String>,
        #[serde(default)]
        display_name: Option<String>,
    },
    Disconnect,
}

fn default_protocol_version() -> u32 {
    1 // 缺省 1：让老版本帧可反序列化，随后按 VersionMismatch 拒绝
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: LanMessage) {
        let json = serde_json::to_string(&msg).unwrap();
        let back: LanMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn pair_request_roundtrips() {
        roundtrip(LanMessage::PairRequest {
            version: LAN_PROTOCOL_VERSION,
            device_name: "MBP".into(),
            invite_secret: "aabbccddeeff00112233445566778899".into(),
        });
    }

    #[test]
    fn pair_accept_roundtrips() {
        roundtrip(LanMessage::PairAccept {
            version: LAN_PROTOCOL_VERSION,
            device_name: "Host".into(),
            fingerprint: "7f3a91c2".into(),
        });
    }

    #[test]
    fn clip_push_auto_fields_default_false_none() {
        // 老帧（v5 早期/异常路径缺字段）缺省 auto=false、origin=None
        let json = r#"{"kind":"clipPush","clip_type":"text","empty":false}"#;
        let msg: LanMessage = serde_json::from_str(json).unwrap();
        match msg {
            LanMessage::ClipPush { auto, origin_node_id, .. } => {
                assert!(!auto);
                assert_eq!(origin_node_id, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn clip_push_with_auto_and_origin_roundtrips() {
        roundtrip(LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: None,
            category_color: None,
            display_name: None,
            auto: true,
            origin_node_id: Some("ab".repeat(32)),
        });
    }

    #[test]
    fn legacy_version_defaults_to_one() {
        let json = r#"{"kind":"pairRequest","device_name":"old","invite_secret":"x"}"#;
        let msg: LanMessage = serde_json::from_str(json).unwrap();
        match msg {
            LanMessage::PairRequest { version, .. } => assert_eq!(version, 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn payload_limit_unchanged() {
        assert_eq!(LAN_MAX_PAYLOAD, 8 * 1024 * 1024);
        assert_eq!(LAN_PROTOCOL_VERSION, 5);
    }
}
