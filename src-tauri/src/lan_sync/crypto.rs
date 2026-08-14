//! 加密原语：X25519 密钥协商 + 配对码派生会话密钥 + AES-256-GCM 加密会话帧。
//!
//! 线格式（v4 握手（挑战-响应）+ v2 会话帧）：`[u32 nonce_len=12][nonce 12B][u32 ct_len][ct]`，
//! ct = AES-256-GCM 加密的明文帧（明文帧格式与 session::Connection 一致：
//! `[u32 header_len][header json][u32 payload_len][payload]`）。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::lan_sync::protocol::{LanMessage, LAN_MAX_PAYLOAD};

type HmacSha256 = Hmac<Sha256>;

/// 外层加密帧密文上限：明文帧最大 ≈ 2 * LAN_MAX_PAYLOAD + 16，加 GCM tag 与余量。
pub(crate) const LAN_MAX_FRAME: usize = 2 * LAN_MAX_PAYLOAD + 64;

const SESSION_KEY_INFO: &[u8] = b"ipaste-lan-sync-v2";

pub(crate) struct SecureConnection {
    stream: TcpStream,
    cipher: Aes256Gcm,
    /// 会话密钥原值，供 `into_split` 为读/写两半各重建一份无状态 cipher。
    session_key: [u8; 32],
}

const GUEST_PROOF_INFO: &[u8] = b"ipaste-lan-v4-guest";
const HOST_TAG_INFO: &[u8] = b"ipaste-lan-v4-host";
const TRANSCRIPT_DOMAIN: &[u8] = b"ipaste-lan-v4-transcript";

/// 转录字段：u32 小端长度前缀 + 字节内容（消除拼接歧义）。
fn extend_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u32).to_le_bytes());
    hasher.update(bytes);
}

/// 握手转录哈希：对（协议版本、host 设备名、host 公钥、guest 设备名、guest 公钥）
/// 逐字段「u32 小端长度前缀 + 字节」串联后 SHA256。v4 的所有握手认证标签都以它为
/// 输入——设备名与公钥因此被绑定进认证范围，无法被中途替换。
pub(crate) fn transcript_hash(
    version: u32,
    host_name: &str,
    host_public: &PublicKey,
    guest_name: &str,
    guest_public: &PublicKey,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TRANSCRIPT_DOMAIN);
    extend_field(&mut hasher, &version.to_le_bytes());
    extend_field(&mut hasher, host_name.as_bytes());
    extend_field(&mut hasher, host_public.as_bytes());
    extend_field(&mut hasher, guest_name.as_bytes());
    extend_field(&mut hasher, guest_public.as_bytes());
    hasher.finalize().into()
}

/// guest 持码证明：MAC(session_key, "v4-guest" ‖ transcript) 截 16 字节 hex。
/// 它是「DH 共享密钥 + 配对码」的函数，被动窃听者没有私钥即无法离线验证码猜测。
pub(crate) fn guest_proof(key: &[u8; 32], transcript: &[u8; 32]) -> String {
    tagged_mac16(key, GUEST_PROOF_INFO, transcript)
}

/// host 认证标签：MAC(session_key, "v4-host" ‖ transcript) 截 16 字节 hex。
/// guest 校验它 = 同时确认对方持码与握手指纹（设备名/公钥未被替换）。
pub(crate) fn host_transcript_tag(key: &[u8; 32], transcript: &[u8; 32]) -> String {
    tagged_mac16(key, HOST_TAG_INFO, transcript)
}

fn tagged_mac16(key: &[u8; 32], domain: &[u8], transcript: &[u8; 32]) -> String {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(domain);
    mac.update(transcript);
    hex(&mac.finalize().into_bytes()[..16])
}

/// X25519 临时密钥对（私钥仅内存，不落盘）。
pub(crate) fn generate_pair_keys() -> (StaticSecret, PublicKey) {
    let secret = StaticSecret::random();
    let public = PublicKey::from(&secret);
    (secret, public)
}

/// 会话密钥：DH 共享密钥经 HKDF（配对码做 salt）派生。
/// 中间人不知道配对码就无法派生密钥解密/注入会话帧。
pub(crate) fn derive_session_key(secret: &StaticSecret, peer_public: &PublicKey, code: &str) -> [u8; 32] {
    let shared = secret.diffie_hellman(peer_public);
    let hk = Hkdf::<Sha256>::new(Some(code.trim().as_bytes()), shared.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(SESSION_KEY_INFO, &mut okm)
        .expect("32 bytes is valid for sha256 hkdf");
    okm
}

/// 明文帧字节 → (消息, payload)。格式与 session::Connection 线格式一致。
pub(crate) fn parse_plaintext_frame(bytes: &[u8]) -> Result<(LanMessage, Option<Vec<u8>>), String> {
    if bytes.len() < 4 {
        return Err("帧不完整".to_string());
    }
    let header_len = u32::from_le_bytes(bytes[..4].try_into().map_err(|_| "帧不完整")?) as usize;
    if header_len > LAN_MAX_PAYLOAD {
        return Err("帧头超限".to_string());
    }
    if bytes.len() < 4 + header_len {
        return Err("帧不完整".to_string());
    }
    let msg: LanMessage =
        serde_json::from_slice(&bytes[4..4 + header_len]).map_err(|e| e.to_string())?;
    let has_payload = matches!(
        &msg,
        LanMessage::ClipPush { empty: false, .. } | LanMessage::ClipResponse { empty: false, .. }
    );
    if !has_payload {
        return Ok((msg, None));
    }
    if bytes.len() < 8 + header_len {
        return Err("帧不完整".to_string());
    }
    let payload_len =
        u32::from_le_bytes(bytes[4 + header_len..8 + header_len].try_into().map_err(|_| "帧不完整")?) as usize;
    if payload_len > LAN_MAX_PAYLOAD {
        return Err("payload 超限".to_string());
    }
    if bytes.len() != 8 + header_len + payload_len {
        return Err("帧长度不符".to_string());
    }
    Ok((msg, Some(bytes[8 + header_len..].to_vec())))
}

pub(crate) fn build_plaintext_frame(msg: &LanMessage, payload: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let header = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
    if header.len() > LAN_MAX_PAYLOAD {
        return Err("帧头过大".to_string());
    }
    let mut out = Vec::with_capacity(4 + header.len() + payload.map_or(0, |p| 4 + p.len()));
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    if let Some(data) = payload {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
    }
    Ok(out)
}

fn cipher_from_key(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn read_u32<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(buf))
}

impl SecureConnection {
    pub(crate) fn new(stream: TcpStream, key: [u8; 32]) -> Self {
        Self { stream, cipher: cipher_from_key(&key), session_key: key }
    }

    pub(crate) async fn read_message(&mut self) -> Result<(LanMessage, Option<Vec<u8>>), String> {
        let plain = self.read_plaintext_frame().await?;
        parse_plaintext_frame(&plain)
    }

    pub(crate) async fn write_message(
        &mut self,
        msg: &LanMessage,
        payload: Option<&[u8]>,
    ) -> Result<(), String> {
        let plain = build_plaintext_frame(msg, payload)?;
        self.write_plaintext_frame(&plain).await
    }

    async fn read_plaintext_frame(&mut self) -> Result<Vec<u8>, String> {
        let nonce_len = read_u32(&mut self.stream).await? as usize;
        if nonce_len != 12 {
            return Err("nonce 长度异常".to_string());
        }
        let mut nonce_bytes = [0u8; 12];
        self.stream.read_exact(&mut nonce_bytes).await.map_err(|e| e.to_string())?;
        let ct_len = read_u32(&mut self.stream).await? as usize;
        if ct_len > LAN_MAX_FRAME {
            return Err("加密帧超限".to_string());
        }
        let mut ct = vec![0u8; ct_len];
        self.stream.read_exact(&mut ct).await.map_err(|e| e.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, ct.as_ref())
            .map_err(|_| "帧解密失败".to_string())
    }

    async fn write_plaintext_frame(&mut self, plain: &[u8]) -> Result<(), String> {
        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).map_err(|e| e.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self.cipher.encrypt(nonce, plain).map_err(|_| "帧加密失败".to_string())?;
        self.stream.write_all(&12u32.to_le_bytes()).await.map_err(|e| e.to_string())?;
        self.stream.write_all(&nonce_bytes).await.map_err(|e| e.to_string())?;
        self.stream.write_all(&(ct.len() as u32).to_le_bytes()).await.map_err(|e| e.to_string())?;
        self.stream.write_all(&ct).await.map_err(|e| e.to_string())?;
        self.stream.flush().await.map_err(|e| e.to_string())
    }
}

/// 加密连接的「读半」：持有 `OwnedReadHalf` 与一份无状态的 cipher。
///
/// 设计动机：`run_session_loop` 在 `tokio::select!` 循环里调用 `read_message`
/// 会命中 `AsyncReadExt::read_exact` 在 `select!` 中**不 cancellation-safe**
/// 的陷阱——心跳分支先就绪时丢弃读 future 会破坏流状态，进而把整个
/// `select!` 绑死（host 无法响应 Disconnect 的根因）。把读循环挪进独立任务、
/// 主循环改为在「控制通道 / 帧通道」之间 select 即可彻底规避（两者均 cancel-safe）。
pub(crate) struct SecureReadHalf {
    read: tokio::net::tcp::OwnedReadHalf,
    cipher: Aes256Gcm,
}

/// 加密连接的「写半」：持有 `OwnedWriteHalf` 与一份 cipher。
/// 仅在 select 分支**体内** `.await`（非分支 future），不涉及 cancel-safety。
pub(crate) struct SecureWriteHalf {
    write: tokio::net::tcp::OwnedWriteHalf,
    cipher: Aes256Gcm,
}

impl SecureConnection {
    /// 拆成读/写两半，分别交给读任务与会话主循环。
    /// cipher 无状态，两半各自从同一密钥重建，互不影响。
    pub(crate) fn into_split(self) -> (SecureReadHalf, SecureWriteHalf) {
        let cipher = cipher_from_key(&self.session_key);
        let cipher2 = cipher_from_key(&self.session_key);
        let (read, write) = self.stream.into_split();
        (SecureReadHalf { read, cipher }, SecureWriteHalf { write, cipher: cipher2 })
    }
}

impl SecureReadHalf {
    /// 读取并解密一帧。读循环在独立任务里反复调用它。
    pub(crate) async fn read_message(&mut self) -> Result<(LanMessage, Option<Vec<u8>>), String> {
        let plain = self.read_plaintext_frame().await?;
        parse_plaintext_frame(&plain)
    }

    async fn read_plaintext_frame(&mut self) -> Result<Vec<u8>, String> {
        let nonce_len = read_u32(&mut self.read).await? as usize;
        if nonce_len != 12 {
            return Err("nonce 长度异常".to_string());
        }
        let mut nonce_bytes = [0u8; 12];
        self.read.read_exact(&mut nonce_bytes).await.map_err(|e| e.to_string())?;
        let ct_len = read_u32(&mut self.read).await? as usize;
        if ct_len > LAN_MAX_FRAME {
            return Err("加密帧超限".to_string());
        }
        let mut ct = vec![0u8; ct_len];
        self.read.read_exact(&mut ct).await.map_err(|e| e.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, ct.as_ref())
            .map_err(|_| "帧解密失败".to_string())
    }
}

impl SecureWriteHalf {
    /// 加密并写出一帧。仅被会话主循环在响应控制/帧时调用。
    pub(crate) async fn write_message(
        &mut self,
        msg: &LanMessage,
        payload: Option<&[u8]>,
    ) -> Result<(), String> {
        let plain = build_plaintext_frame(msg, payload)?;
        let mut nonce_bytes = [0u8; 12];
        getrandom::getrandom(&mut nonce_bytes).map_err(|e| e.to_string())?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self.cipher.encrypt(nonce, plain.as_ref()).map_err(|_| "帧加密失败".to_string())?;
        self.write.write_all(&12u32.to_le_bytes()).await.map_err(|e| e.to_string())?;
        self.write.write_all(&nonce_bytes).await.map_err(|e| e.to_string())?;
        self.write.write_all(&(ct.len() as u32).to_le_bytes()).await.map_err(|e| e.to_string())?;
        self.write.write_all(&ct).await.map_err(|e| e.to_string())?;
        self.write.flush().await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn both_sides_derive_same_session_key() {
        let (a_sec, a_pub) = generate_pair_keys();
        let (b_sec, b_pub) = generate_pair_keys();
        let ka = derive_session_key(&a_sec, &b_pub, "ABC123");
        let kb = derive_session_key(&b_sec, &a_pub, "ABC123");
        assert_eq!(ka, kb);
        // 码不同 → 密钥不同
        assert_ne!(ka, derive_session_key(&a_sec, &b_pub, "ABC124"));
    }

    #[test]
    fn plaintext_frame_build_parse_roundtrip() {
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: Some("工作".into()),
            category_color: Some("#0D9488".into()),
            display_name: Some("重命名".into()),
        };
        let bytes = build_plaintext_frame(&msg, Some(b"hello")).unwrap();
        let (parsed, payload) = parse_plaintext_frame(&bytes).unwrap();
        assert_eq!(parsed, msg);
        assert_eq!(payload.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn plaintext_frame_rejects_oversized_payload_length() {
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: None,
            category_color: None,
            display_name: None,
        };
        // 用真实 payload 构造，确保帧里存在 payload_len 字段
        let mut bytes = build_plaintext_frame(&msg, Some(b"x")).unwrap();
        // payload_len 字段紧跟 header 之后，偏移 = 4 + header_len
        let header_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        let payload_len_off = 4 + header_len;
        // 把 payload_len 改成超限值，使解析命中 payload_len > LAN_MAX_PAYLOAD 分支
        bytes[payload_len_off..payload_len_off + 4]
            .copy_from_slice(&((LAN_MAX_PAYLOAD as u32) + 1).to_le_bytes());
        assert!(parse_plaintext_frame(&bytes).is_err());
    }

    #[tokio::test]
    async fn secure_connection_roundtrips_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let key = [7u8; 32];

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = SecureConnection::new(stream, key);
            let (msg, payload) = conn.read_message().await.unwrap();
            assert!(matches!(msg, LanMessage::ClipPush { clip_type, empty: false, .. } if clip_type == "text"));
            assert_eq!(payload.as_deref(), Some(&b"secret"[..]));
            conn.write_message(
                &LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None },
                Some(b"ack"),
            )
            .await
            .unwrap();
        });

        let mut client = SecureConnection::new(TcpStream::connect(addr).await.unwrap(), key);
        client
            .write_message(
                &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None },
                Some(b"secret"),
            )
            .await
            .unwrap();
        let (msg, payload) = client.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipResponse { empty: false, .. }));
        assert_eq!(payload.as_deref(), Some(&b"ack"[..]));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn secure_connection_rejects_wrong_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = SecureConnection::new(stream, [1u8; 32]);
            assert!(conn.read_message().await.is_err());
        });

        let mut client = SecureConnection::new(TcpStream::connect(addr).await.unwrap(), [2u8; 32]);
        client
            .write_message(
                &LanMessage::ClipPush { clip_type: "text".into(), empty: true, category_name: None, category_color: None, display_name: None },
                None,
            )
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[test]
    fn transcript_hash_changes_when_any_field_changes() {
        let (_, host_pub) = generate_pair_keys();
        let (_, guest_pub) = generate_pair_keys();
        let base = transcript_hash(4, "host", &host_pub, "guest", &guest_pub);
        assert_ne!(base, transcript_hash(3, "host", &host_pub, "guest", &guest_pub), "版本参与转录");
        assert_ne!(base, transcript_hash(4, "evil", &host_pub, "guest", &guest_pub), "host 名参与转录");
        assert_ne!(base, transcript_hash(4, "host", &host_pub, "evil", &guest_pub), "guest 名参与转录");
        let (_, other_pub) = generate_pair_keys();
        assert_ne!(base, transcript_hash(4, "host", &other_pub, "guest", &guest_pub), "host 公钥参与转录");
        assert_ne!(base, transcript_hash(4, "host", &host_pub, "guest", &other_pub), "guest 公钥参与转录");
    }

    #[test]
    fn transcript_hash_is_deterministic() {
        let (_, host_pub) = generate_pair_keys();
        let (_, guest_pub) = generate_pair_keys();
        assert_eq!(
            transcript_hash(4, "host", &host_pub, "guest", &guest_pub),
            transcript_hash(4, "host", &host_pub, "guest", &guest_pub)
        );
    }

    #[test]
    fn guest_proof_and_host_tag_are_domain_separated() {
        let key = [5u8; 32];
        let (_, host_pub) = generate_pair_keys();
        let (_, guest_pub) = generate_pair_keys();
        let transcript = transcript_hash(4, "h", &host_pub, "g", &guest_pub);
        assert_ne!(guest_proof(&key, &transcript), host_transcript_tag(&key, &transcript));
        assert_eq!(guest_proof(&key, &transcript).len(), 32, "HMAC 截 16 字节 hex = 32 字符");
    }

    #[test]
    fn transcript_tags_depend_on_transcript() {
        let key = [5u8; 32];
        let (_, host_pub) = generate_pair_keys();
        let (_, guest_pub) = generate_pair_keys();
        let t1 = transcript_hash(4, "h", &host_pub, "g", &guest_pub);
        let t2 = transcript_hash(4, "h", &host_pub, "g2", &guest_pub);
        assert_ne!(guest_proof(&key, &t1), guest_proof(&key, &t2));
        assert_ne!(host_transcript_tag(&key, &t1), host_transcript_tag(&key, &t2));
    }
}
