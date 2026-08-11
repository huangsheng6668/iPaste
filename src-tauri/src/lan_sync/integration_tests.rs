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
        assert!(matches!(msg, LanMessage::Handshake { code, device_name, auto: _ } if code == "ROOM" && device_name == "guest"));
        conn.write_message(&LanMessage::PairAccepted { host_device_name: "host".into() }, None).await.unwrap();

        // 2. 收推送
        let (msg, payload) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipPush { clip_type, empty: false } if clip_type == "text"));
        assert_eq!(payload.as_deref(), Some(&b"hi"[..]));

        // 3. 收拉取请求，回响应
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipRequest));
        conn.write_message(&LanMessage::ClipResponse { clip_type: "text".into(), empty: false }, Some(b"back")).await.unwrap();

        // 4. 收断开
        let (msg, _) = conn.read_message().await.unwrap();
        assert_eq!(msg, LanMessage::Disconnect);
    });

    let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
    client.write_message(&LanMessage::Handshake { code: "ROOM".into(), device_name: "guest".into(), auto: false }, None).await.unwrap();
    let (msg, _) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairAccepted { host_device_name } if host_device_name == "host"));

    client.write_message(&LanMessage::ClipPush { clip_type: "text".into(), empty: false }, Some(b"hi")).await.unwrap();
    client.write_message(&LanMessage::ClipRequest, None).await.unwrap();
    let (msg, payload) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::ClipResponse { empty: false, .. }));
    assert_eq!(payload.as_deref(), Some(&b"back"[..]));

    client.write_message(&LanMessage::Disconnect, None).await.unwrap();
    host.await.unwrap();
}

#[tokio::test]
async fn handshake_auto_true_roundtrips_with_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        let (msg, _) = conn.read_message().await.unwrap();
        match msg {
            LanMessage::Handshake { code, device_name, auto } => {
                assert_eq!(code, "");
                assert_eq!(device_name, "guest");
                assert!(auto, "auto must be true for scanned join");
            }
            _ => panic!("wrong variant"),
        }
        conn.write_message(
            &LanMessage::PairAccepted { host_device_name: "host".into() },
            None,
        )
        .await
        .unwrap();
    });

    let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
    client
        .write_message(
            &LanMessage::Handshake {
                code: "".into(),
                device_name: "guest".into(),
                auto: true,
            },
            None,
        )
        .await
        .unwrap();
    let (msg, _) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairAccepted { host_device_name } if host_device_name == "host"));
    host.await.unwrap();
}

#[tokio::test]
async fn discover_roundtrips_and_handshake_still_works() {
    // 1) Discover 分流：Host 收 Discover → 回 DiscoverResponse → 连接关闭
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        let (msg, _) = conn.read_message().await.unwrap();
        match msg {
            LanMessage::Discover => {
                conn.write_message(
                    &LanMessage::DiscoverResponse {
                        device_name: "host".into(),
                        tcp_port: 45130,
                    },
                    None,
                )
                .await
                .unwrap();
            }
            _ => panic!("expected Discover, got {:?}", msg),
        }
    });
    let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
    client.write_message(&LanMessage::Discover, None).await.unwrap();
    let (msg, _) = client.read_message().await.unwrap();
    match msg {
        LanMessage::DiscoverResponse { device_name, tcp_port } => {
            assert_eq!(device_name, "host");
            assert_eq!(tcp_port, 45130);
        }
        _ => panic!("expected DiscoverResponse"),
    }
    host.await.unwrap();

    // 2) Handshake 配对路径回归（现有协议不变）：完整握手 + ClipPush 往返
    let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = listener2.local_addr().unwrap();
    let host2 = tokio::spawn(async move {
        let (stream, _) = listener2.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(
            msg,
            LanMessage::Handshake { code, device_name, auto: false }
                if code == "ROOM" && device_name == "guest"
        ));
        conn.write_message(
            &LanMessage::PairAccepted { host_device_name: "host".into() },
            None,
        )
        .await
        .unwrap();
        let (msg, payload) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::ClipPush { empty: false, .. }));
        assert_eq!(payload.as_deref(), Some(&b"hi"[..]));
    });
    let mut client2 = Connection::new(TcpStream::connect(addr2).await.unwrap());
    client2
        .write_message(
            &LanMessage::Handshake {
                code: "ROOM".into(),
                device_name: "guest".into(),
                auto: false,
            },
            None,
        )
        .await
        .unwrap();
    let (msg, _) = client2.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairAccepted { host_device_name } if host_device_name == "host"));
    client2
        .write_message(&LanMessage::ClipPush { clip_type: "text".into(), empty: false }, Some(b"hi"))
        .await
        .unwrap();
    host2.await.unwrap();
}

