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
        conn.write_message(&LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None }, Some(b"back")).await.unwrap();

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

    client.write_message(&LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None }, Some(b"hi")).await.unwrap();
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
            LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name } => {
                assert_eq!(clip_type, "text");
                assert!(!empty);
                assert_eq!(category_name.as_deref(), Some("工作"));
                assert_eq!(category_color.as_deref(), Some("#0D9488"));
                assert_eq!(display_name.as_deref(), Some("重命名条目"));
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
                display_name: Some("重命名条目".into()),
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
                &LanMessage::ClipResponse { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None },
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
            &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None },
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
            &LanMessage::ClipPush { clip_type: "text".into(), empty: false, category_name: None, category_color: None, display_name: None },
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
            LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name } => {
                assert_eq!(clip_type, "text");
                assert!(!empty);
                assert_eq!(category_name.as_deref(), Some("api_key"));
                assert_eq!(category_color.as_deref(), Some("#3B82F6"));
                assert_eq!(display_name.as_deref(), Some("重命名后的密钥"));
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
                display_name: Some("重命名后的密钥".into()),
            },
            Some(b"sk-test-12345"),
        )
        .await
        .unwrap();
    server.await.unwrap();
}


/// 最小复现：std listener + TcpListener::from_std + accept 在测试运行时内是否可用。
#[tokio::test(flavor = "multi_thread")]
async fn minimal_from_std_accept_works() {
    use tokio::net::{TcpListener, TcpStream};
    let std_listener = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let listener = TcpListener::from_std(std_listener).unwrap();
    let addr = listener.local_addr().unwrap();
    eprintln!("[minimal] listening at {addr}");
    let port = addr.port();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
    let mgr = std::sync::Arc::new(crate::lan_sync::LanSessionManager::new_for_test());
    let st = crate::store::test_support::temp_store();
    let code_str = String::from("TESTCD");
    let task = tokio::spawn(async move {
        eprintln!("[minimal] accept task running");
        let (stream, peer) = listener.accept().await.unwrap();
        eprintln!("[minimal] accepted");
        let _permit = sem.clone().try_acquire_owned().unwrap();
        let mgr = mgr.clone();
        let st = st.clone();
        let code_str = code_str.clone();
        let handler = tokio::spawn(async move {
            eprintln!("[minimal] REAL-STYLE handler running");
            let mut conn = crate::lan_sync::session::Connection::new(stream);
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                conn.read_message(),
            )
            .await;
            eprintln!("[minimal] REAL-STYLE read done: {}", read.is_ok());
            let _ = (mgr, st, code_str, peer, _permit);
        });
        handler.await.unwrap();
        true
    });
    let loopback = format!("127.0.0.1:{port}");
    let mut conn = TcpStream::connect(&loopback).await.unwrap();
    eprintln!("[minimal] connected to {loopback}");
    use tokio::io::AsyncWriteExt;
    conn.write_all(b"hi!!").await.unwrap();
    eprintln!("[minimal] wrote 4 bytes");
    assert!(task.await.unwrap());
}

/// 隔离实验：manager 控制通道在不涉及 TCP/session loop 的情况下，
/// `control_tx().send()` 的消息能否被 `take_control_rx()` 拿到的 receiver 收到。
#[tokio::test]
async fn manager_control_channel_roundtrip_in_isolation() {
    use std::sync::Arc;
    use tokio::sync::mpsc;

    use crate::lan_sync::{ControlMsg, LanSessionManager};

    let mgr = Arc::new(LanSessionManager::new_for_test());
    let (tx, rx) = mpsc::channel::<ControlMsg>(16);
    mgr.set_hosting("CODE".into(), "127.0.0.1:1".into(), tx, rx, 42);

    let Some(mut rx) = mgr.take_control_rx() else { panic!("rx missing"); };
    let recv_task = tokio::spawn(async move { rx.recv().await });
    let tx = mgr.control_tx().expect("tx missing");
    tx.send(ControlMsg::Disconnect).await.unwrap();
    let got = tokio::time::timeout(std::time::Duration::from_secs(2), recv_task)
        .await
        .expect("recv timed out")
        .unwrap();
    eprintln!("[isolated] received: {got:?}");
    assert!(matches!(got, Some(ControlMsg::Disconnect)));
}

/// 复现「A 端（guest）主动连接、B 端（host）点断开」全链路，且走**真实**代码路径：
/// - host：真实 `start_host_on`（真实 accept 循环 + 握手 + 配对确认），临时端口；
/// - guest：真实 `join_by_address` 直连；
/// - host 端「断开」：等价于 `lan_disconnect` 的 Connected 分支（经 control channel 发 Disconnect）；
/// - 断言：双方回到 Idle、guest 会话循环退出、host 监听端口被释放。
#[tokio::test(flavor = "multi_thread")]
async fn host_initiated_disconnect_resets_both_sides() {
    use std::sync::Arc;

    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::{ControlMsg, LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let code = "TESTCD";
    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let guest_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();

    // host：真实 start_host 路径，端口 0 = 临时端口。
    let listen_addr = start_host_on(host_manager.clone(), store.clone(), code.to_string(), 0)
        .await
        .unwrap();
    eprintln!("[test] host started at {listen_addr}");
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();
    let dial_addr = format!("127.0.0.1:{port}");

    // guest：真实 join_by_address 直连（内部自建 control channel + set_joining）。
    let join_task = {
        let manager = guest_manager.clone();
        let store = store.clone();
        let code = code.to_string();
        tokio::spawn(async move { join_by_address(manager, store, dial_addr, code).await })
    };

    // host 端用户同意配对（等价于 lan_accept_pair(true)）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        eprintln!(
            "[test] poll: host={:?} guest={:?}",
            host_manager.snapshot().status,
            guest_manager.snapshot().status
        );
        if let Some(tx) = host_manager.take_pair_decision_tx() {
            let _ = tx.send(true);
            eprintln!("[test] pair accepted");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "pair request never arrived");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // 等双方 Connected。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Connected
            && guest_manager.snapshot().status == LanStatus::Connected
        {
            eprintln!("[test] both connected");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "session never connected");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // B 端点「断开」：等价于 lan_disconnect 的 Connected 分支。
    eprintln!("[test] sending Disconnect via host control channel");
    host_manager
        .control_tx()
        .expect("host control tx available")
        .send(ControlMsg::Disconnect)
        .await
        .unwrap();
    eprintln!("[test] Disconnect enqueued");

    // 双方都应回到 Idle（若这里卡住，即复现「host 无法主动断开」）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Idle
            && guest_manager.snapshot().status == LanStatus::Idle
        {
            eprintln!("[test] both idle");
            break;
        }
        eprintln!(
            "[test] after disconnect poll: host={:?} guest={:?}",
            host_manager.snapshot().status,
            guest_manager.snapshot().status
        );
        assert!(std::time::Instant::now() < deadline, "host disconnect did not reset sessions");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // guest 会话循环应已退出（join_by_address 返回）。
    tokio::time::timeout(std::time::Duration::from_secs(5), join_task)
        .await
        .unwrap()
        .unwrap();

    // host 的 accept 任务被 abort 后，监听端口应已释放（断开真正生效）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "host port not released after disconnect");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

