//! 配对票据：`ipaste-pair-v1:` + base64url(version ‖ endpoint_id ‖ relay ‖ 直连地址 ‖ 邀请密钥)。
//!
//! 二进制布局（小端）：
//! [version u8=1][endpoint_id 32B][relay_flag u8][relay_len u16 + bytes]
//! [addr_count u8][addr_len u16 + bytes]×N[invite_secret 16B]
//! 直连地址让「局域网无外网」也能拨号（中继不可达时）。

use std::time::{Duration, Instant};

use base64::Engine as _;

pub(crate) const TICKET_PREFIX: &str = "ipaste-pair-v1:";
pub(crate) const INVITE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PairTicket {
    pub version: u8,
    pub endpoint_id: [u8; 32],
    pub relay_url: Option<String>,
    pub direct_addrs: Vec<String>,
    pub invite_secret: [u8; 16],
}

impl PairTicket {
    pub(crate) fn encode(&self) -> String {
        let mut out = Vec::with_capacity(64 + self.relay_url.as_ref().map_or(0, |r| r.len()));
        out.push(self.version);
        out.extend_from_slice(&self.endpoint_id);
        match &self.relay_url {
            Some(relay) => {
                out.push(1);
                out.extend_from_slice(&(relay.len() as u16).to_le_bytes());
                out.extend_from_slice(relay.as_bytes());
            }
            None => out.push(0),
        }
        out.push(self.direct_addrs.len().min(u8::MAX as usize) as u8);
        for addr in self.direct_addrs.iter().take(u8::MAX as usize) {
            out.extend_from_slice(&(addr.len() as u16).to_le_bytes());
            out.extend_from_slice(addr.as_bytes());
        }
        out.extend_from_slice(&self.invite_secret);
        format!(
            "{TICKET_PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(out)
        )
    }

    pub(crate) fn decode(input: &str) -> Result<Self, String> {
        let body = input
            .trim()
            .strip_prefix(TICKET_PREFIX)
            .ok_or("不是有效的 iPaste 配对票据")?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| "票据编码损坏")?;
        let mut cursor = Cursor { bytes: &bytes, pos: 0 };
        let version = cursor.u8()?;
        if version != 1 {
            return Err(format!("票据版本 {version} 不受支持，请双方升级到最新版 iPaste"));
        }
        let endpoint_id = cursor.array32()?;
        let relay_url = match cursor.u8()? {
            0 => None,
            1 => Some(cursor.string()?),
            _ => return Err("票据编码损坏".to_string()),
        };
        let addr_count = cursor.u8()? as usize;
        let mut direct_addrs = Vec::with_capacity(addr_count);
        for _ in 0..addr_count {
            direct_addrs.push(cursor.string()?);
        }
        let invite_secret = cursor.array16()?;
        if !cursor.is_empty() {
            return Err("票据包含多余数据".to_string());
        }
        Ok(Self { version, endpoint_id, relay_url, direct_addrs, invite_secret })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.pos).ok_or("票据不完整")?;
        self.pos += 1;
        Ok(b)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let slice = self.bytes.get(self.pos..self.pos + N).ok_or("票据不完整")?;
        self.pos += N;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }
    fn array32(&mut self) -> Result<[u8; 32], String> { self.array::<32>() }
    fn array16(&mut self) -> Result<[u8; 16], String> { self.array::<16>() }
    fn string(&mut self) -> Result<String, String> {
        let len = u16::from_le_bytes(self.array::<2>()?) as usize;
        if len > 512 {
            return Err("票据字段超长".to_string());
        }
        let slice = self.bytes.get(self.pos..self.pos + len).ok_or("票据不完整")?;
        self.pos += len;
        String::from_utf8(slice.to_vec()).map_err(|_| "票据编码损坏".to_string())
    }
    fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

/// 内存态邀请登记：一次性 + TTL + 可作废。app 重启即全部作废（不持久化）。
pub(crate) struct InviteRegistry {
    active: Option<Invite>,
}

struct Invite {
    secret: [u8; 16],
    expires_at: Instant,
}

impl InviteRegistry {
    pub(crate) fn new() -> Self {
        Self { active: None }
    }

    /// 生成新邀请（覆盖旧邀请——同一时刻只有一个有效邀请，简化心智模型）。
    pub(crate) fn create(&mut self) -> [u8; 16] {
        let mut secret = [0u8; 16];
        getrandom::getrandom(&mut secret).expect("os rng unavailable");
        self.active = Some(Invite { secret, expires_at: Instant::now() + INVITE_TTL });
        secret
    }

    pub(crate) fn cancel(&mut self) {
        self.active = None;
    }

    /// 校验并核销：**校验成功即焚；校验失败邀请保留**（spec §4.2：只有成功
    /// 才消耗一次性邀请）。失败即焚会让局域网攻击者轮换 NodeId 发垃圾密钥
    /// 即可烧掉全部邀请、阻断正常配对（DoS）。过期视为已死（不回填）。
    /// 常数时间比较，防时序侧信道。
    pub(crate) fn verify_and_consume(&mut self, secret_hex: &str) -> bool {
        let Some(invite) = self.active.take() else { return false };
        if Instant::now() > invite.expires_at {
            return false; // 已过期：邀请作废，无需回填
        }
        let Ok(bytes) = hex_decode_16(secret_hex) else {
            self.active = Some(invite); // 输入非法定长 hex：误输入/探测，保留邀请
            return false;
        };
        if constant_time_eq(&bytes, &invite.secret) {
            true // 匹配：保持取走状态（一次性核销）
        } else {
            self.active = Some(invite); // 密钥不匹配：回填，等待真正的配对方
            false
        }
    }

    /// 测试辅助：把当前邀请的过期时间改到过去（不引入时钟抽象，YAGNI）。
    #[cfg(test)]
    fn expire_now(&mut self) {
        if let Some(invite) = &mut self.active {
            invite.expires_at = Instant::now() - Duration::from_secs(1);
        }
    }
}

fn hex_decode_16(input: &str) -> Result<[u8; 16], String> {
    if input.len() != 32 || !input.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("hex 长度不符".to_string());
    }
    let mut out = [0u8; 16];
    for (i, chunk) in input.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16).ok_or("bad hex")?;
        let lo = (chunk[1] as char).to_digit(16).ok_or("bad hex")?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Ok(out)
}

fn constant_time_eq(a: &[u8; 16], b: &[u8; 16]) -> bool {
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PairTicket {
        PairTicket {
            version: 1,
            endpoint_id: [7u8; 32],
            relay_url: Some("https://relay.example.com".into()),
            direct_addrs: vec!["192.168.1.5:52123".into(), "10.0.0.2:52123".into()],
            invite_secret: [9u8; 16],
        }
    }

    #[test]
    fn ticket_roundtrips() {
        let encoded = sample().encode();
        assert!(encoded.starts_with("ipaste-pair-v1:"));
        assert_eq!(PairTicket::decode(&encoded).unwrap(), sample());
    }

    #[test]
    fn ticket_without_relay_and_addrs_roundtrips() {
        let t = PairTicket {
            version: 1,
            endpoint_id: [1; 32],
            relay_url: None,
            direct_addrs: vec![],
            invite_secret: [2; 16],
        };
        assert_eq!(PairTicket::decode(&t.encode()).unwrap(), t);
    }

    #[test]
    fn decode_rejects_bad_inputs() {
        assert!(PairTicket::decode("nonsense").is_err());
        assert!(PairTicket::decode("ipaste-pair-v1:!!!").is_err());
        // 篡改 version 字节：base64url 首字符承载 version=1，改为 2 的编码
        let mut t = sample();
        t.version = 2;
        let bad = t.encode();
        assert!(PairTicket::decode(&bad).unwrap_err().contains("版本"));
    }

    #[test]
    fn invite_is_one_shot_and_expires() {
        let mut reg = InviteRegistry::new();
        let secret = reg.create();
        let hex_str: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        assert!(reg.verify_and_consume(&hex_str), "首次校验成功");
        assert!(!reg.verify_and_consume(&hex_str), "一次性：二次失败");
    }

    #[test]
    fn invite_cancel_invalidates() {
        let mut reg = InviteRegistry::new();
        let secret = reg.create();
        reg.cancel();
        let hex_str: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!reg.verify_and_consume(&hex_str));
    }

    #[test]
    fn invite_wrong_secret_fails() {
        let mut reg = InviteRegistry::new();
        let _secret = reg.create();
        assert!(!reg.verify_and_consume(&"00".repeat(16)));
    }

    /// 失败不焚毁（F1 回归）：错误密钥/坏 hex 之后，正确密钥仍可核销；
    /// 核销成功后才是一次性。
    #[test]
    fn invite_survives_failed_verification() {
        let mut reg = InviteRegistry::new();
        let secret = reg.create();
        let correct: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        // 错误密钥（合法定长 hex）：不得烧掉邀请
        assert!(!reg.verify_and_consume(&"00".repeat(16)));
        // 非法 hex（探测垃圾）：同样不得烧掉邀请
        assert!(!reg.verify_and_consume("zz!not-hex-at-all-------------"));
        assert!(!reg.verify_and_consume("aabb"));
        // 正确密钥在一系列失败尝试后仍应核销成功
        assert!(reg.verify_and_consume(&correct), "失败尝试后正确密钥应仍有效");
        // 成功即焚：一次性语义保持
        assert!(!reg.verify_and_consume(&correct), "核销成功后二次校验失败");
    }

    #[test]
    fn invite_expires() {
        let mut reg = InviteRegistry::new();
        let secret = reg.create();
        reg.expire_now();
        let hex_str: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!reg.verify_and_consume(&hex_str));
    }
}
