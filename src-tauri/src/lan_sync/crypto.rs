//! 加密原语：X25519 密钥协商 + 配对码派生会话密钥 + AES-256-GCM 加密会话帧。
//!
//! 线格式（v2 会话）：`[u32 nonce_len=12][nonce 12B][u32 ct_len][ct]`，
//! ct = AES-256-GCM 加密的明文帧（明文帧格式与 session::Connection 一致：
//! `[u32 header_len][header json][u32 payload_len][payload]`）。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::lan_sync::protocol::{LanMessage, LAN_MAX_PAYLOAD};

type HmacSha256 = Hmac<Sha256>;

/// 外层加密帧密文上限：明文帧最大 ≈ 2 * LAN_MAX_PAYLOAD + 16，加 GCM tag 与余量。
pub(crate) const LAN_MAX_FRAME: usize = 2 * LAN_MAX_PAYLOAD + 64;

const CODE_CLAIM_INFO: &[u8] = b"ipaste-lan-pair-v2";
const HOST_AUTH_INFO: &[u8] = b"ipaste-lan-auth-v2-host";
const SESSION_KEY_INFO: &[u8] = b"ipaste-lan-sync-v2";

pub(crate) struct SecureConnection {
    stream: TcpStream,
    cipher: Aes256Gcm,
}

pub(crate) fn code_claim(code: &str) -> String {
    // 注：`new_from_slice` 在 `HmacSha256` 上同时由 `hmac::Mac` 与 `aes_gcm::KeyInit`（经
    // `use aes_gcm::aead::KeyInit` 引入）提供，存在歧义，故显式消歧到 `Mac` trait。
    let mut mac = <HmacSha256 as Mac>::new_from_slice(code.trim().as_bytes()).expect("hmac accepts any key length");
    mac.update(CODE_CLAIM_INFO);
    hex(&mac.finalize().into_bytes()[..16])
}

pub(crate) fn host_auth_tag(key: &[u8; 32]) -> String {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(HOST_AUTH_INFO);
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

async fn read_u32(stream: &mut TcpStream) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(buf))
}

impl SecureConnection {
    pub(crate) fn new(stream: TcpStream, key: [u8; 32]) -> Self {
        Self { stream, cipher: cipher_from_key(&key) }
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use tokio::net::TcpListener;

    #[test]
    fn code_claim_is_stable_and_hex16() {
        assert_eq!(code_claim("ROOM"), code_claim("ROOM"));
        assert_ne!(code_claim("ROOM"), code_claim("room"));
        assert_eq!(code_claim("ROOM").len(), 32);
    }

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
    fn host_auth_tag_depends_on_key() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        assert_ne!(host_auth_tag(&k1), host_auth_tag(&k2));
        assert_eq!(host_auth_tag(&k1).len(), 32);
    }

    #[test]
    fn plaintext_frame_build_parse_roundtrip() {
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: Some("工作".into()),
            category_color: Some("#0D9488".into()),
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
        };
        let mut bytes = build_plaintext_frame(&msg, None).unwrap();
        // 手工把 payload_len 字段改成超限值
        let payload_len_off = bytes.len() - 4;
        bytes[payload_len_off..].copy_from_slice(&((LAN_MAX_PAYLOAD as u32) + 1).to_le_bytes());
        bytes.extend_from_slice(&[0u8; 4]);
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
                &LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None },
                Some(b"ack"),
            )
            .await
            .unwrap();
        });

        let mut client = SecureConnection::new(TcpStream::connect(addr).await.unwrap(), key);
        client
            .write_message(
                &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None },
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
                &LanMessage::ClipPush { clip_type: "text".into(), empty: true, category_name: None, category_color: None },
                None,
            )
            .await
            .unwrap();
        server.await.unwrap();
    }
}
