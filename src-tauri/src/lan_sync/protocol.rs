use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const LAN_TCP_BASE_PORT: u16 = 45130;
pub(crate) const LAN_MAX_PAYLOAD: usize = 8 * 1024 * 1024;
pub(crate) const LAN_PROTOCOL_VERSION: u32 = 3;
/// 单次分组批量传输最多接纳的条目数（用于接收端预排 sort_order 的上界）。
pub(crate) const LAN_BATCH_MAX_ITEMS: u32 = 10_000;
/// 配对码字母表：31 字符（A-Z 去掉 I/L/O = 23 字母 + 数字 2-9 = 8），无易混淆字符。
pub(crate) const PAIR_CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
pub(crate) const PAIR_CODE_LEN: usize = 8;

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
        /// 协议版本。老客户端无此字段 → 默认 1，host 校验版本不符直接拒绝。
        #[serde(default = "default_protocol_version")]
        version: u32,
        /// HMAC 派生码（配对码不以明文上线）。老帧/未知场景为 None。
        #[serde(default)]
        code_claim: Option<String>,
        device_name: String,
        /// base64 编码的 X25519 公钥（32 字节）。
        #[serde(default)]
        guest_pubkey: Option<String>,
    },
    PairAccepted {
        host_device_name: String,
        /// base64 编码的 host X25519 公钥。老 host 无此字段。
        #[serde(default)]
        host_pubkey: Option<String>,
        /// HMAC(session_key) 认证标签，guest 据此确认双方持有相同配对码。
        #[serde(default)]
        auth_tag: Option<String>,
    },
    PairRejected {
        #[serde(default)]
        reason: PairRejectReason,
    },
    ClipPush {
        clip_type: String,
        empty: bool,
        /// 所属分组名（按名称在接收端匹配/创建分组）。`None` 表示历史/无分组。
        /// 向后兼容：老版本发送的帧无此字段，反序列化得到 `None`，仍按历史处理。
        #[serde(default)]
        category_name: Option<String>,
        /// 分组颜色（随分组名一起传，新建分组时采用）。老版本帧缺省为 `None`。
        #[serde(default)]
        category_color: Option<String>,
        /// 条目的重命名显示名（用户手动重命名后的名称，`None` 表示未重命名）。
        /// 向后兼容：老版本发送的帧无此字段，反序列化得到 `None`。
        #[serde(default)]
        display_name: Option<String>,
    },
    /// 分组批量传输开始：接收端进入批量收集态，直到收到 `CategoryBatchEnd`。
    /// 协议 v3 两端之间使用（握手已保证双方版本一致，老版本不会遇到该帧）。
    CategoryBatchStart {
        /// 分组名（按名称在接收端匹配/创建分组）。
        category_name: String,
        /// 分组颜色（接收端新建分组时采用）。
        category_color: Option<String>,
        /// 预计条目数：接收端用它预排 sort_order 以保持发送顺序；0 表示未知。
        item_count: u32,
    },
    /// 分组批量传输结束：接收端清点结果并发出汇总事件。
    CategoryBatchEnd,
    ClipRequest,
    ClipResponse {
        clip_type: String,
        empty: bool,
        #[serde(default)]
        category_name: Option<String>,
        #[serde(default)]
        category_color: Option<String>,
        /// 与 ClipPush 一致；「拉取当前剪贴板」场景恒为 `None`。
        #[serde(default)]
        display_name: Option<String>,
    },
    Disconnect,
}

/// 生成 8 位随机配对码（约 39 bit 熵：log2(31^8) ≈ 39.6）。
///
/// 用拒绝采样（rejection sampling）消除取模偏差：字母表 31 字符，而
/// `256 % 31 = 8 ≠ 0`，若直接 `% 31` 则前 8 个字符会被多命中。故对每字节只接受
/// `[0, 248)`（`floor(256/31)*31 = 248`，`248 % 31 == 0` 分布均匀），落在
/// `[248, 256)` 则丢弃重抽。
pub(crate) fn generate_pair_code() -> String {
    let alphabet_len = PAIR_CODE_ALPHABET.len(); // 31
    // 接受区间上界（不含）：`floor(256 / alphabet_len) * alphabet_len`。
    // 31 时为 248，接受 [0, 248)，丢弃 [248, 256)（共 8 个值），消除取模偏差。
    let accept_limit = (256 / alphabet_len) * alphabet_len; // 248
    (0..PAIR_CODE_LEN)
        .map(|_| {
            let mut buf = [0u8; 1];
            loop {
                getrandom::getrandom(&mut buf).expect("os rng unavailable");
                if (buf[0] as usize) < accept_limit {
                    return PAIR_CODE_ALPHABET[buf[0] as usize % alphabet_len] as char;
                }
            }
        })
        .collect()
}

/// 归一化配对码：None/空串 → 随机生成；Some → trim 后校验长度 6..=16。
pub(crate) fn normalize_pair_code(input: Option<String>) -> Result<String, String> {
    match input.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
        None => Ok(generate_pair_code()),
        Some(code) => {
            let len = code.chars().count();
            if !(6..=16).contains(&len) {
                return Err("匹配码需为 6-16 位字符".to_string());
            }
            Ok(code)
        }
    }
}

fn default_protocol_version() -> u32 {
    1
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
    fn handshake_v2_roundtrips() {
        let msg = LanMessage::Handshake {
            version: LAN_PROTOCOL_VERSION,
            code_claim: Some("aabbccdd".into()),
            device_name: "MBP".into(),
            guest_pubkey: Some("QUJD".into()),
        };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    #[test]
    fn pair_accepted_v2_roundtrips() {
        let msg = LanMessage::PairAccepted {
            host_device_name: "host".into(),
            host_pubkey: Some("REVG".into()),
            auth_tag: Some("ffeeddcc".into()),
        };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    /// 老版本（v1）Handshake 帧：只有 code/device_name，缺 version/code_claim/guest_pubkey。
    /// 新 host 必须能反序列化（字段默认 None / version 默认 1），再按协议拒绝。
    #[test]
    fn legacy_handshake_deserializes_with_defaults() {
        let json = r#"{"kind":"handshake","code":"ROOM","device_name":"old"}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        match decode_frame(&bytes).unwrap() {
            LanMessage::Handshake { version, code_claim, guest_pubkey, device_name } => {
                assert_eq!(version, 1);
                assert_eq!(code_claim, None);
                assert_eq!(guest_pubkey, None);
                assert_eq!(device_name, "old");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    /// 老版本 PairAccepted 帧：只有 host_device_name。guest 侧据此提示对方版本过旧。
    #[test]
    fn legacy_pair_accepted_deserializes_with_defaults() {
        let json = r#"{"kind":"pairAccepted","host_device_name":"old-host"}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        match decode_frame(&bytes).unwrap() {
            LanMessage::PairAccepted { host_device_name, host_pubkey, auth_tag } => {
                assert_eq!(host_device_name, "old-host");
                assert_eq!(host_pubkey, None);
                assert_eq!(auth_tag, None);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn clip_push_with_empty_roundtrips() {
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: true,
            category_name: None,
            category_color: None,
            display_name: None,
        };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    #[test]
    fn clip_push_with_category_roundtrips() {
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: Some("工作".into()),
            category_color: Some("#0D9488".into()),
            display_name: Some("重命名后的条目".into()),
        };
        let bytes = encode_frame(&msg).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), msg);
    }

    /// 分组批量传输的 Start/End 帧往返：字段完整、无 payload。
    #[test]
    fn category_batch_start_end_roundtrips() {
        let start = LanMessage::CategoryBatchStart {
            category_name: "工作".into(),
            category_color: Some("#0D9488".into()),
            item_count: 12,
        };
        let bytes = encode_frame(&start).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), start);

        let end = LanMessage::CategoryBatchEnd;
        let bytes = encode_frame(&end).unwrap();
        assert_eq!(decode_frame(&bytes).unwrap(), end);
    }

    /// 老版本帧不含 category_name/category_color 字段；新版本必须兼容，
    /// 缺省字段反序列化为 None（保证新版 ↔ 旧版混合不会反序列化失败）。
    #[test]
    fn clip_push_legacy_without_category_defaults_none() {
        // 老版本发的 JSON：仅有 kind/clip_type/empty（无分组字段）。
        // 注意：该 enum 为 internally-tagged，serde 字段名为 snake_case（rename_all
        // 不作用于 internally-tagged 内容的字段），仅 variant tag 走 camelCase。
        let json = r#"{"kind":"clipPush","clip_type":"text","empty":false}"#;
        let mut bytes = (json.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        match decode_frame(&bytes).unwrap() {
            LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name } => {
                assert_eq!(clip_type, "text");
                assert!(!empty);
                assert_eq!(category_name, None);
                assert_eq!(category_color, None);
                assert_eq!(display_name, None, "老帧缺省 display_name 应为 None");
            }
            _ => panic!("wrong variant"),
        }
    }

    /// 协议 v3 新字段 display_name 有值时的往返。
    #[test]
    fn clip_push_with_display_name_roundtrips() {
        let msg = LanMessage::ClipPush {
            clip_type: "link".into(),
            empty: false,
            category_name: None,
            category_color: None,
            display_name: Some("API 文档".into()),
        };
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

    #[test]
    fn pair_code_is_eight_chars_from_alphabet() {
        let code = generate_pair_code();
        assert_eq!(code.chars().count(), PAIR_CODE_LEN);
        assert!(code
            .bytes()
            .all(|b| PAIR_CODE_ALPHABET.contains(&b)));
    }

    #[test]
    fn pair_code_avoids_confusable_chars() {
        for c in ['I', 'L', 'O', '0', '1'] {
            assert!(!PAIR_CODE_ALPHABET.contains(&(c as u8)));
        }
    }

    #[test]
    fn pair_code_alphabet_is_thirty_one_chars() {
        // 31 字符（A-Z 去 I/L/O = 23 + 数字 2-9 = 8），无重复。
        assert_eq!(PAIR_CODE_ALPHABET.len(), 31);
        let set: std::collections::HashSet<u8> = PAIR_CODE_ALPHABET.iter().copied().collect();
        assert_eq!(set.len(), 31, "字母表不能有重复字符");
    }

    #[test]
    fn normalize_pair_code_generates_when_none_or_empty() {
        for input in [None, Some("".into()), Some("   ".into())] {
            let code = normalize_pair_code(input).unwrap();
            assert_eq!(code.chars().count(), PAIR_CODE_LEN);
        }
    }

    #[test]
    fn normalize_pair_code_trims_and_validates_length() {
        assert_eq!(normalize_pair_code(Some("  AB12CD  ".into())).unwrap(), "AB12CD");
        assert!(normalize_pair_code(Some("ABCDE".into())).is_err()); // 5 位太短
        assert!(normalize_pair_code(Some("A".repeat(17)).into()).is_err()); // 17 位太长
        assert_eq!(normalize_pair_code(Some("A".repeat(16)).into()).unwrap(), "A".repeat(16));
    }

    /// 防回归：payload 上限必须保持 8MB（剪贴板图片 data url 足够，过大会放大内存 DoS）。
    #[test]
    fn payload_limit_is_eight_megabytes() {
        assert_eq!(LAN_MAX_PAYLOAD, 8 * 1024 * 1024);
    }

    /// 批量传输接纳上限：用于接收端 sort_order 预排上界，防止恶意 item_count 放大偏移。
    #[test]
    fn batch_max_items_is_bounded() {
        assert!(LAN_BATCH_MAX_ITEMS >= 100);
        assert!(LAN_BATCH_MAX_ITEMS <= 1_000_000);
    }
}
