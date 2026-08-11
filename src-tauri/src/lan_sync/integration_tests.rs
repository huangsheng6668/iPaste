use std::net::Ipv4Addr;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

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
async fn multicast_loopback_receives_packet() {
    let sock = UdpSocket::bind(("0.0.0.0", LAN_UDP_PORT))
        .await
        .unwrap();
    sock.join_multicast_v4(LAN_MULTICAST_ADDR, Ipv4Addr::new(127, 0, 0, 1))
        .unwrap();
    sock.set_multicast_loop_v4(true).unwrap();

    let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sender
        .send_to(b"mcast-hello", (LAN_MULTICAST_ADDR, LAN_UDP_PORT))
        .await
        .unwrap();

    let mut buf = [0u8; 64];
    let (n, src) = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        sock.recv_from(&mut buf),
    )
    .await
    .expect("组播 loopback 应在 3s 内收到")
    .expect("recv 失败");
    assert_eq!(&buf[..n], b"mcast-hello");
    assert!(!src.ip().is_unspecified(), "组播包源地址应为发送端 IP");
}
