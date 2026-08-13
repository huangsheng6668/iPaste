use tokio::net::{TcpListener, TcpStream};

use crate::lan_sync::protocol::*;
use crate::lan_sync::session::Connection;

#[tokio::test]
async fn full_handshake_push_request_disconnect_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);

        // 1. 握手
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::Handshake { device_name, .. } if device_name == "guest"));
        conn.write_message(&LanMessage::PairAccepted { host_device_name: "host".into(), host_pubkey: None, auth_tag: None }, None).await.unwrap();

        // 2. 收推送
        let (msg, payload) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipPush { clip_type, empty: false, .. } if clip_type == "text"));
        assert_eq!(payload.as_deref(), Some(&b"hi"[..]));

        // 3. 收拉取请求，回响应
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipRequest));
        conn.write_message(&LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None }, Some(b"back")).await.unwrap();

        // 4. 收断开
        let (msg, _) = conn.read_message().await.unwrap();
        assert_eq!(msg, LanMessage::Disconnect);
    });

    let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
    client.write_message(
        &LanMessage::Handshake {
            version: LAN_PROTOCOL_VERSION,
            code_claim: None,
            device_name: "guest".into(),
            guest_pubkey: None,
        },
        None,
    )
    .await
    .unwrap();
    let (msg, _) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairAccepted { host_device_name, .. } if host_device_name == "host"));

    client.write_message(&LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None }, Some(b"hi")).await.unwrap();
    client.write_message(&LanMessage::ClipRequest, None).await.unwrap();
    let (msg, payload) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::ClipResponse { empty: false, .. }));
    assert_eq!(payload.as_deref(), Some(&b"back"[..]));

    client.write_message(&LanMessage::Disconnect, None).await.unwrap();
    host.await.unwrap();
}

/// 验证带分组信息的 ClipPush 在 TCP 往返后字段保持一致。
#[tokio::test]
async fn clip_push_with_category_roundtrips_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        let (msg, payload) = conn.read_message().await.unwrap();
        match msg {
            LanMessage::ClipPush { clip_type, empty, category_name, category_color } => {
                assert_eq!(clip_type, "text");
                assert!(!empty);
                assert_eq!(category_name.as_deref(), Some("工作"));
                assert_eq!(category_color.as_deref(), Some("#0D9488"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert_eq!(payload.as_deref(), Some(&b"hello"[..]));
    });

    let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
    client
        .write_message(
            &LanMessage::ClipPush {
                clip_type: "text".into(),
                empty: false,
                category_name: Some("工作".into()),
                category_color: Some("#0D9488".into()),
            },
            Some(b"hello"),
        )
        .await
        .unwrap();
    server.await.unwrap();
}

/// 完整 v2 流程：明文握手（带 claim/公钥）→ 双方派生密钥 → 加密会话收发。
#[tokio::test]
async fn full_v2_handshake_and_secure_session_roundtrip() {
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::*;

    let code = "TESTCODE";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);

        // 1. 读 v2 Handshake，校验 claim
        let (msg, _) = conn.read_message().await.unwrap();
        let (claim, guest_pubkey_b64) = match msg {
            LanMessage::Handshake { version, code_claim, guest_pubkey, .. } => {
                assert_eq!(version, LAN_PROTOCOL_VERSION);
                (code_claim.unwrap(), guest_pubkey.unwrap())
            }
            other => panic!("wrong variant: {other:?}"),
        };
        assert_eq!(claim, code_claim(code));

        // 2. 派生会话密钥，回 PairAccepted
        let (host_secret, host_public) = generate_pair_keys();
        let guest_public = {
            let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
                .decode(&guest_pubkey_b64)
                .unwrap()
                .try_into()
                .unwrap();
            PublicKey::from(bytes)
        };
        let key = derive_session_key(&host_secret, &guest_public, code);
        conn.write_message(
            &LanMessage::PairAccepted {
                host_device_name: "host".into(),
                host_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(host_public.as_bytes())),
                auth_tag: Some(host_auth_tag(&key)),
            },
            None,
        )
        .await
        .unwrap();

        // 3. 加密会话：收 ClipPush、回 ClipResponse
        let mut secure = SecureConnection::new(conn.into_stream(), key);
        let (msg, payload) = secure.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipPush { clip_type, empty: false, .. } if clip_type == "text"));
        assert_eq!(payload.as_deref(), Some(&b"secret"[..]));
        secure
            .write_message(
                &LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None },
                Some(b"ack"),
            )
            .await
            .unwrap();
    });

    // guest 侧
    let mut conn = Connection::new(TcpStream::connect(addr).await.unwrap());
    let (guest_secret, guest_public) = generate_pair_keys();
    conn.write_message(
        &LanMessage::Handshake {
            version: LAN_PROTOCOL_VERSION,
            code_claim: Some(code_claim(code)),
            device_name: "guest".into(),
            guest_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes())),
        },
        None,
    )
    .await
    .unwrap();
    let (msg, _) = conn.read_message().await.unwrap();
    let (host_pubkey_b64, tag) = match msg {
        LanMessage::PairAccepted { host_pubkey, auth_tag, .. } => (host_pubkey.unwrap(), auth_tag.unwrap()),
        other => panic!("wrong variant: {other:?}"),
    };
    let host_public = {
        let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
            .decode(&host_pubkey_b64)
            .unwrap()
            .try_into()
            .unwrap();
        PublicKey::from(bytes)
    };
    let key = derive_session_key(&guest_secret, &host_public, code);
    assert_eq!(host_auth_tag(&key), tag);

    let mut secure = SecureConnection::new(conn.into_stream(), key);
    secure
        .write_message(
            &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None },
            Some(b"secret"),
        )
        .await
        .unwrap();
    let (msg, payload) = secure.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::ClipResponse { empty: false, .. }));
    assert_eq!(payload.as_deref(), Some(&b"ack"[..]));

    host.await.unwrap();
}

/// 攻击者不知道配对码：claim 校验失败时 host 拒绝，且派生密钥不同无法解密。
#[tokio::test]
async fn wrong_code_claim_derives_mismatched_keys() {
    use crate::lan_sync::crypto::*;

    let (a_sec, a_pub) = generate_pair_keys();
    let (b_sec, b_pub) = generate_pair_keys();
    assert_ne!(
        derive_session_key(&a_sec, &b_pub, "REALCODE"),
        derive_session_key(&b_sec, &a_pub, "WRONG001")
    );
}

/// host 用正确的 key 加密会话帧，但 guest 用错误的 key（不同配对码派生）
/// → guest 的 SecureConnection 解密失败。这验证了 auth_tag 校验逻辑的密码学基础：
/// 若 guest 跳过 auth_tag 校验，它会拿到一个无法解密任何帧的错误 key。
#[tokio::test]
async fn mismatched_key_prevents_decryption() {
    use crate::lan_sync::crypto::*;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let host_key = [1u8; 32];
    let guest_key = [2u8; 32]; // 故意不同

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = SecureConnection::new(stream, host_key);
        // host 用 host_key 加密发送
        conn.write_message(
            &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None },
            Some(b"secret"),
        )
        .await
        .unwrap();
    });

    // guest 用不同的 guest_key → 解密必然失败
    let mut conn = SecureConnection::new(TcpStream::connect(addr).await.unwrap(), guest_key);
    let result = conn.read_message().await;
    assert!(result.is_err(), "密钥不匹配时解密必须失败");
    server.await.unwrap();
}

/// 加密会话（SecureConnection）下，带 category_name/color 的纯文本 ClipPush
/// 经真实 TCP + AES-256-GCM roundtrip 后字段必须完整保留。
///
/// 覆盖 v0.3.22 引入的加密通道 + 分组条目组合（此前该组合零测试，是用户报告
/// 「分组条目丢失」时首先被怀疑的路径）。断言字段保留即证明加密层不丢 category 信息。
#[tokio::test]
async fn secure_connection_preserves_category_fields_over_encrypted_roundtrip() {
    use crate::lan_sync::crypto::SecureConnection;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let key = [9u8; 32];

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = SecureConnection::new(stream, key);
        let (msg, payload) = conn.read_message().await.unwrap();
        match msg {
            LanMessage::ClipPush { clip_type, empty, category_name, category_color } => {
                assert_eq!(clip_type, "text");
                assert!(!empty);
                assert_eq!(category_name.as_deref(), Some("api_key"));
                assert_eq!(category_color.as_deref(), Some("#3B82F6"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        assert_eq!(payload.as_deref(), Some(&b"sk-test-12345"[..]));
    });

    let mut client = SecureConnection::new(TcpStream::connect(addr).await.unwrap(), key);
    client
        .write_message(
            &LanMessage::ClipPush {
                clip_type: "text".into(),
                empty: false,
                category_name: Some("api_key".into()),
                category_color: Some("#3B82F6".into()),
            },
            Some(b"sk-test-12345"),
        )
        .await
        .unwrap();
    server.await.unwrap();
}
