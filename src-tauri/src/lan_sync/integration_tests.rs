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
