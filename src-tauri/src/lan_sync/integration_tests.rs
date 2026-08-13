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
