use std::sync::Mutex;

use tokio::net::{TcpListener, TcpStream};

use crate::events::EVENT_LAN_JOIN_FAILED;
use crate::lan_sync::protocol::*;
use crate::lan_sync::session::{run_session_loop, Connection};
use crate::lan_sync::LanEventSink;

/// 测试事件出口：把 (event, payload) 记入共享 Vec，供集成测试断言 emit 内容
/// （如 join-failed 的具体提示文案——「对方版本过旧」曾被误报，需要按文案回归）。
/// 与其唯一消费者（本文件）同住：`LanSessionManager::new_capturing_for_test`
/// 构造它并接入 manager。
pub(crate) struct CapturingEventSink {
    pub(crate) events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl LanEventSink for CapturingEventSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        self.events.lock().expect("capture sink poisoned").push((event.to_string(), payload.clone()));
    }
}

impl CapturingEventSink {
    /// 取所有 `lan-join-failed` 事件的 reason 文案。
    pub(crate) fn join_failed_reasons(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("capture sink poisoned")
            .iter()
            .filter(|(event, _)| event == EVENT_LAN_JOIN_FAILED)
            .map(|(_, payload)| {
                payload
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }
}

#[tokio::test]
async fn full_handshake_push_request_disconnect_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);

        // 1. 握手：host 先发言下发挑战，guest 回 Handshake，host 回 PairAccepted
        conn.write_message(
            &LanMessage::PairChallenge {
                version: LAN_PROTOCOL_VERSION,
                host_device_name: "host".into(),
                host_pubkey: Some("QUJD".into()),
            },
            None,
        ).await.unwrap();
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::Handshake { device_name, .. } if device_name == "guest"));
        conn.write_message(&LanMessage::PairAccepted { host_device_name: "host".into(), auth_tag: None }, None).await.unwrap();

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
    // v4：先读 host 挑战，再回 Handshake，最后读 PairAccepted。
    let (msg, _) = client.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairChallenge { .. }));
    client.write_message(
        &LanMessage::Handshake {
            version: LAN_PROTOCOL_VERSION,
            device_name: "guest".into(),
            guest_pubkey: None,
            guest_proof: None,
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

/// 完整 v4 流程：host 下发挑战 → 双方派生密钥 + 转录 → 持码证明互换 → 加密会话收发。
#[tokio::test]
async fn full_v4_handshake_and_secure_session_roundtrip() {
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{
        derive_session_key, generate_pair_keys, guest_proof, host_transcript_tag, transcript_hash,
        SecureConnection,
    };

    let code = "TESTCODE";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);

        // 1. v4 host 先发言：下发挑战（带 host 公钥）
        let (host_secret, host_public) = generate_pair_keys();
        conn.write_message(
            &LanMessage::PairChallenge {
                version: LAN_PROTOCOL_VERSION,
                host_device_name: "host".into(),
                host_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(host_public.as_bytes())),
            },
            None,
        ).await.unwrap();
        // 2. 读 guest Handshake，校验持码证明
        let (msg, _) = conn.read_message().await.unwrap();
        let (guest_pubkey_b64, proof) = match msg {
            LanMessage::Handshake { version, guest_pubkey, guest_proof, device_name } => {
                assert_eq!(version, LAN_PROTOCOL_VERSION);
                assert_eq!(device_name, "guest");
                (guest_pubkey.unwrap(), guest_proof.unwrap())
            }
            other => panic!("wrong variant: {other:?}"),
        };
        let guest_public = {
            let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
                .decode(&guest_pubkey_b64).unwrap().try_into().unwrap();
            PublicKey::from(bytes)
        };
        let key = derive_session_key(&host_secret, &guest_public, code);
        let transcript = transcript_hash(LAN_PROTOCOL_VERSION, "host", &host_public, "guest", &guest_public);
        assert_eq!(guest_proof(&key, &transcript), proof);
        // 3. 回 PairAccepted（转录绑定的认证标签）
        conn.write_message(
            &LanMessage::PairAccepted {
                host_device_name: "host".into(),
                auth_tag: Some(host_transcript_tag(&key, &transcript)),
            },
            None,
        ).await.unwrap();

        // 4. 加密会话：收 ClipPush、回 ClipResponse
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
    let (msg, _) = conn.read_message().await.unwrap();
    let (host_name, host_public) = match msg {
        LanMessage::PairChallenge { version, host_device_name, host_pubkey } => {
            assert_eq!(version, LAN_PROTOCOL_VERSION);
            let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
                .decode(&host_pubkey.unwrap()).unwrap().try_into().unwrap();
            (host_device_name, PublicKey::from(bytes))
        }
        other => panic!("wrong variant: {other:?}"),
    };
    let key = derive_session_key(&guest_secret, &host_public, code);
    let transcript = transcript_hash(LAN_PROTOCOL_VERSION, &host_name, &host_public, "guest", &guest_public);
    conn.write_message(
        &LanMessage::Handshake {
            version: LAN_PROTOCOL_VERSION,
            device_name: "guest".into(),
            guest_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes())),
            guest_proof: Some(guest_proof(&key, &transcript)),
        },
        None,
    )
    .await
    .unwrap();
    let (msg, _) = conn.read_message().await.unwrap();
    let tag = match msg {
        LanMessage::PairAccepted { auth_tag, .. } => auth_tag.unwrap(),
        other => panic!("wrong variant: {other:?}"),
    };
    assert_eq!(host_transcript_tag(&key, &transcript), tag);

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
    std_listener.set_nonblocking(true).unwrap();
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
    mgr.set_hosting("CODE".into(), "127.0.0.1:1".into(), tx, rx);

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

/// 回归（用户报告的核心 bug）：A 端（guest）连上 B 端（host）后，B 端在**收过数据帧
/// 之后**仍能主动断开。
///
/// 旧架构里读任务把帧经 mpsc 转发给主循环、主循环在「control_rx + frame_rx」之间
/// select。全链路下 host 主循环一旦 park（尤其在收过数据后）就再也唤不醒——既收不到
/// 后续帧、也响应不了 Disconnect，表现为「B 端点断开无反应、双方一直 Connected」。
/// 改为读任务原地处理帧、主循环只 select「control + 读任务结束信号」后，本测试通过。
///
/// 流程：host 真实 start_host_on → guest 真实 join → 双方 Connected →
/// guest 经 control 发一条带分组的 SendClip（走 DB 落库、不碰系统剪贴板）→
/// 断言 host 收到落库（修复前这里超时）→ host 发 Disconnect → 断言双方都回 Idle。
#[tokio::test(flavor = "multi_thread")]
async fn host_initiated_disconnect_after_receiving_data() {
    use std::sync::Arc;

    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::{ControlMsg, LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let code = "TESTCD";
    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let guest_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();

    let listen_addr = start_host_on(host_manager.clone(), store.clone(), code.to_string(), 0)
        .await
        .unwrap();
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();
    let dial_addr = format!("127.0.0.1:{port}");

    let join_task = {
        let manager = guest_manager.clone();
        let store = store.clone();
        let code = code.to_string();
        tokio::spawn(async move { join_by_address(manager, store, dial_addr, code).await })
    };

    // host 同意配对
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(tx) = host_manager.take_pair_decision_tx() {
            let _ = tx.send(true);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pair request never arrived"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // 等双方 Connected
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Connected
            && guest_manager.snapshot().status == LanStatus::Connected
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "session never connected"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // guest → host：发一条带分组的文本（走 DB 落库，不写系统剪贴板）
    guest_manager
        .control_tx()
        .expect("guest control tx")
        .send(ControlMsg::SendClip {
            clip_type: "text".into(),
            payload: b"hello-after-data".to_vec(),
            category_name: Some("leftover-cat".into()),
            category_color: Some("#0D9488".into()),
            display_name: None,
        })
        .await
        .unwrap();

    // 等 host 真实收到并落库（收过数据帧是复现「已知遗留」的关键前提）：
    // apply_received 会按名称新建分组「leftover-cat」，分组出现即证明数据帧已被处理。
    // 修复前：host 主循环 park 后再也唤不醒，分组永不出现，本断言超时失败。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if store
            .list_categories()
            .unwrap()
            .iter()
            .any(|c| c.name == "leftover-cat")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "host never received data frame after guest SendClip (regression: host session loop stuck)"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // host 点「断开」（等价于 lan_disconnect 的 Connected 分支）
    host_manager
        .control_tx()
        .expect("host control tx")
        .send(ControlMsg::Disconnect)
        .await
        .unwrap();

    // 双方都应回到 Idle（卡住即复现「收过数据后 host 无法主动断开」）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Idle
            && guest_manager.snapshot().status == LanStatus::Idle
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "host disconnect after data did not reset sessions (leftover reproduced)"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    tokio::time::timeout(std::time::Duration::from_secs(5), join_task)
        .await
        .unwrap()
        .unwrap();
}

/// 回归：两侧并发跑 `run_session_loop`，guest 经 control 发一条带分组的文本，
/// host 必须收到并落库。覆盖「读任务原地处理入站帧」的新架构——确保 frame 不再
/// 经 mpsc 转发给主循环时，host 仍能稳定接收 guest 推送的内容。
#[tokio::test(flavor = "multi_thread")]
async fn two_direct_session_loops_guest_sends_host_receives() {
    use std::sync::Arc;

    use crate::lan_sync::crypto::SecureConnection;
    use crate::lan_sync::{ControlMsg, LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let key = [42u8; 32];

    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let guest_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();

    let (host_ctx, host_crx) = tokio::sync::mpsc::channel::<ControlMsg>(16);
    let (guest_ctx, guest_crx) = tokio::sync::mpsc::channel::<ControlMsg>(16);
    // 模拟全链路：host 的 control channel 经 manager 存取（set_hosting → take_control_rx）
    host_manager.set_hosting("CODE".into(), "127.0.0.1:1".into(), host_ctx, host_crx);

    let host_mgr = host_manager.clone();
    let host_store = store.clone();
    let host_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let host_crx = host_mgr.take_control_rx().expect("host control rx");
        run_session_loop(
            SecureConnection::new(stream, key),
            host_mgr,
            host_store,
            "guest".to_string(),
            host_crx,
        )
        .await;
    });

    let guest_mgr = guest_manager.clone();
    let guest_store = store.clone();
    let guest_task = tokio::spawn(async move {
        let stream = TcpStream::connect(addr).await.unwrap();
        run_session_loop(
            SecureConnection::new(stream, key),
            guest_mgr,
            guest_store,
            "host".to_string(),
            guest_crx,
        )
        .await;
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Connected
            && guest_manager.snapshot().status == LanStatus::Connected
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    guest_ctx
        .send(ControlMsg::SendClip {
            clip_type: "text".into(),
            payload: b"two-loops-payload".to_vec(),
            category_name: Some("twoloops-cat".into()),
            category_color: Some("#0D9488".into()),
            display_name: None,
        })
        .await
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if store
            .list_categories()
            .unwrap()
            .iter()
            .any(|c| c.name == "twoloops-cat")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "two concurrent session loops: host did not receive guest's frame"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let _ = host_manager
        .control_tx()
        .unwrap()
        .send(ControlMsg::Disconnect)
        .await;
    let _ = guest_ctx.send(ControlMsg::Disconnect).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), host_task).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), guest_task).await;
}

/// 回归：host 走**真实全链路**（start_host_on + accept + handle_guest_with_challenge
/// + 配对确认 + 真实 v4 crypto），对端是裸客户端（读挑战后手动握手再写一帧）。确保 host 的
/// 全链路能稳定接收并落库对端推送的帧。
#[tokio::test(flavor = "multi_thread")]
async fn host_full_path_with_raw_client_receives_frame() {
    use std::sync::Arc;

    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{
        derive_session_key, generate_pair_keys, guest_proof, host_transcript_tag, transcript_hash,
        SecureConnection,
    };
    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let code = "RAWCODE";
    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();

    let listen_addr = start_host_on(host_manager.clone(), store.clone(), code.to_string(), 0)
        .await
        .unwrap();
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();
    let dial_addr = format!("127.0.0.1:{port}");

    // 裸客户端：读挑战 → 按 transcript 算 proof → 发 Handshake → 校验 auth_tag + 写一帧。
    let raw_client = tokio::spawn(async move {
        use crate::lan_sync::session::Connection;
        let mut conn = Connection::new(TcpStream::connect(dial_addr).await.unwrap());
        let (msg, _) = conn.read_message().await.unwrap();
        let (host_name, host_public) = match msg {
            LanMessage::PairChallenge { version, host_device_name, host_pubkey } => {
                assert_eq!(version, LAN_PROTOCOL_VERSION);
                let bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
                    .decode(&host_pubkey.unwrap()).unwrap().try_into().unwrap();
                (host_device_name, PublicKey::from(bytes))
            }
            other => panic!("wrong variant: {other:?}"),
        };
        let (guest_secret, guest_public) = generate_pair_keys();
        let key = derive_session_key(&guest_secret, &host_public, code);
        let transcript = transcript_hash(LAN_PROTOCOL_VERSION, &host_name, &host_public, "raw-guest", &guest_public);
        conn.write_message(
            &LanMessage::Handshake {
                version: LAN_PROTOCOL_VERSION,
                device_name: "raw-guest".into(),
                guest_pubkey: Some(
                    base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes()),
                ),
                guest_proof: Some(guest_proof(&key, &transcript)),
            },
            None,
        )
        .await
        .unwrap();
        let (reply, _) = conn.read_message().await.unwrap();
        let LanMessage::PairAccepted { auth_tag: Some(tag), .. } = reply else {
            panic!("expected PairAccepted v4: {reply:?}");
        };
        assert_eq!(host_transcript_tag(&key, &transcript), tag);
        let mut secure = SecureConnection::new(conn.into_stream(), key);
        secure
            .write_message(
                &LanMessage::ClipPush {
                    clip_type: "text".into(),
                    empty: false,
                    category_name: Some("rawclient-cat".into()),
                    category_color: Some("#0D9488".into()),
                    display_name: None,
                },
                Some(b"rawclient-payload"),
            )
            .await
            .unwrap();
        // 保持连接一小段，让 host 有时间读帧。
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    });

    // host 同意配对
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(tx) = host_manager.take_pair_decision_tx() {
            let _ = tx.send(true);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pair request never arrived"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // 等 Connected
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Connected {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // host 是否收到并落库
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if store
            .list_categories()
            .unwrap()
            .iter()
            .any(|c| c.name == "rawclient-cat")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "host full path did not receive raw client's frame"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let _ = raw_client.await;
}

/// v3 老客户端（直接发无 proof 的 Handshake）连 v4 host：必须被 PairRejected，
/// 而不是崩溃、挂起或触发配对弹窗。
#[tokio::test(flavor = "multi_thread")]
async fn legacy_v3_handshake_is_rejected_by_v4_host() {
    use std::sync::Arc;

    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::session::Connection;
    use crate::lan_sync::LanSessionManager;
    use crate::store::test_support::temp_store;
    use tokio::net::TcpStream;

    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();
    let listen_addr = start_host_on(host_manager.clone(), store, "TESTCD".to_string(), 0)
        .await
        .unwrap();
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();

    let mut conn = Connection::new(TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap());
    // v3 形态：不等问题、直接发言，无 guest_proof
    conn.write_message(
        &LanMessage::Handshake {
            version: 3,
            device_name: "old-guest".into(),
            guest_pubkey: None,
            guest_proof: None,
        },
        None,
    )
    .await
    .unwrap();
    // v4 host accept 即发挑战——先读到它（丢弃），再等拒绝
    let (msg, _) = conn.read_message().await.unwrap();
    assert!(matches!(msg, LanMessage::PairChallenge { .. }));
    let (msg, _) = tokio::time::timeout(std::time::Duration::from_secs(5), conn.read_message())
        .await
        .expect("host must reply")
        .unwrap();
    assert!(matches!(msg, LanMessage::PairRejected { .. }));
    assert!(host_manager.take_pair_decision_tx().is_none(), "版本不符不得触发配对弹窗");
}

/// host accept 后保持沉默（模拟 v3 老版本 host 只等不答）：
/// guest 应在挑战等待超时后回到 Idle，不挂起。
#[tokio::test(flavor = "multi_thread")]
async fn guest_times_out_and_resets_when_host_silent() {
    use std::sync::Arc;

    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let silent = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    });

    let guest_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();
    // CHALLENGE_WAIT_TIMEOUT_SECS 在 cfg(test) 下为 2s，join 返回即代表超时已处理
    join_by_address(guest_manager.clone(), store, format!("127.0.0.1:{port}"), "TESTCD".to_string()).await;
    assert_eq!(guest_manager.snapshot().status, LanStatus::Idle);
    silent.abort();
}

/// 码校验：guest 用错误码计算 proof → host 回 WrongCode，且不触发配对弹窗。
#[tokio::test(flavor = "multi_thread")]
async fn wrong_code_guest_is_rejected_without_pair_prompt() {
    use std::sync::Arc;

    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{derive_session_key, generate_pair_keys, guest_proof, transcript_hash};
    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::session::Connection;
    use crate::lan_sync::LanSessionManager;
    use crate::store::test_support::temp_store;
    use tokio::net::TcpStream;

    let code = "REALCODE";
    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();
    let listen_addr = start_host_on(host_manager.clone(), store, code.to_string(), 0)
        .await
        .unwrap();
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();

    let mut conn = Connection::new(TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap());
    let (msg, _) = conn.read_message().await.unwrap();
    let LanMessage::PairChallenge { version, host_device_name, host_pubkey } = msg else {
        panic!("expected challenge: {msg:?}");
    };
    let host_public_bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
        .decode(&host_pubkey.unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let host_public = PublicKey::from(host_public_bytes);
    // 错误码派生密钥与 proof
    let (guest_secret, guest_public) = generate_pair_keys();
    let key = derive_session_key(&guest_secret, &host_public, "WRONG001");
    let transcript = transcript_hash(version, &host_device_name, &host_public, "guest", &guest_public);
    conn.write_message(
        &LanMessage::Handshake {
            version,
            device_name: "guest".into(),
            guest_pubkey: Some(base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes())),
            guest_proof: Some(guest_proof(&key, &transcript)),
        },
        None,
    )
    .await
    .unwrap();
    let (msg, _) = tokio::time::timeout(std::time::Duration::from_secs(5), conn.read_message())
        .await
        .expect("host must reply")
        .unwrap();
    assert!(matches!(msg, LanMessage::PairRejected { reason: PairRejectReason::WrongCode }));
    assert!(host_manager.take_pair_decision_tx().is_none(), "错码不得触发配对弹窗");
}

/// 转录绑定（帧级双向验证）：challenge 的设备名被中途篡改 →
/// host 用真实转录校验 guest proof 必不匹配；guest 用篡改转录校验 host tag 必不匹配。
#[tokio::test]
async fn tampered_challenge_name_breaks_proof_in_both_directions() {
    use base64::Engine as _;
    use x25519_dalek::PublicKey;

    use crate::lan_sync::crypto::{
        derive_session_key, generate_pair_keys, guest_proof, host_transcript_tag, transcript_hash,
    };

    let code = "TESTCODE";
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        let (host_secret, host_public) = generate_pair_keys();
        conn.write_message(
            &LanMessage::PairChallenge {
                version: 4,
                host_device_name: "RealHost".into(),
                host_pubkey: Some(
                    base64::engine::general_purpose::STANDARD.encode(host_public.as_bytes()),
                ),
            },
            None,
        )
        .await
        .unwrap();
        let (msg, _) = conn.read_message().await.unwrap();
        let LanMessage::Handshake { device_name, guest_pubkey: Some(gp), guest_proof: Some(proof), .. } = msg
        else {
            panic!("expected v4 handshake: {msg:?}");
        };
        let guest_public_bytes: [u8; 32] =
            base64::engine::general_purpose::STANDARD.decode(&gp).unwrap().try_into().unwrap();
        let guest_public = PublicKey::from(guest_public_bytes);
        let key = derive_session_key(&host_secret, &guest_public, code);
        // host 按真实设备名 "RealHost" 计算转录 → guest 的 proof（基于 "EvilHost"）必不匹配
        let authentic = transcript_hash(4, "RealHost", &host_public, &device_name, &guest_public);
        assert_ne!(guest_proof(&key, &authentic), proof, "篡改名后 host 侧校验必须失败");
    });

    // guest：真实收到的挑战名是 "RealHost"，但模拟中间人改帧、guest 实际按 "EvilHost" 参与转录
    let mut conn = TcpStream::connect(addr).await.unwrap();
    let mut conn = Connection::new(conn);
    let (msg, _) = conn.read_message().await.unwrap();
    let LanMessage::PairChallenge { version, host_device_name, host_pubkey } = msg else {
        panic!("expected challenge: {msg:?}");
    };
    assert_eq!(host_device_name, "RealHost");
    let host_public_bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
        .decode(&host_pubkey.unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let host_public = PublicKey::from(host_public_bytes);
    let (guest_secret, guest_public) = generate_pair_keys();
    let key = derive_session_key(&guest_secret, &host_public, code);
    let tampered = transcript_hash(version, "EvilHost", &host_public, "guest", &guest_public);
    // guest 自检：host 若按真实名签 tag，篡改转录下校验必失败（对称方向）
    let real_tag = host_transcript_tag(&key, &transcript_hash(version, "RealHost", &host_public, "guest", &guest_public));
    assert_ne!(host_transcript_tag(&key, &tampered), real_tag, "篡改名后 guest 侧校验必须失败");
    conn.write_message(
        &LanMessage::Handshake {
            version,
            device_name: "guest".into(),
            guest_pubkey: Some(
                base64::engine::general_purpose::STANDARD.encode(guest_public.as_bytes()),
            ),
            guest_proof: Some(guest_proof(&key, &tampered)),
        },
        None,
    )
    .await
    .unwrap();
    host.await.unwrap();
}

/// 接线回归：超长分组名的帧被拒收（分组不落库），且会话存活——
/// 同一会话里随后发送的合法分组能正常出现。
#[tokio::test(flavor = "multi_thread")]
async fn oversized_category_name_frame_is_rejected_and_session_survives() {
    use std::sync::Arc;

    use crate::lan_sync::crypto::SecureConnection;
    use crate::lan_sync::{ControlMsg, LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let key = [43u8; 32];

    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let guest_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();

    let (host_ctx, host_crx) = tokio::sync::mpsc::channel::<ControlMsg>(16);
    let (guest_ctx, guest_crx) = tokio::sync::mpsc::channel::<ControlMsg>(16);
    host_manager.set_hosting("CODE".into(), "127.0.0.1:1".into(), host_ctx, host_crx);

    let host_mgr = host_manager.clone();
    let host_store = store.clone();
    let host_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let host_crx = host_mgr.take_control_rx().expect("host control rx");
        run_session_loop(SecureConnection::new(stream, key), host_mgr, host_store, "guest".to_string(), host_crx).await;
    });

    let guest_mgr = guest_manager.clone();
    let guest_store = store.clone();
    let guest_task = tokio::spawn(async move {
        let stream = TcpStream::connect(addr).await.unwrap();
        run_session_loop(SecureConnection::new(stream, key), guest_mgr, guest_store, "host".to_string(), guest_crx).await;
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if host_manager.snapshot().status == LanStatus::Connected
            && guest_manager.snapshot().status == LanStatus::Connected
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // 超长分组名（81 字符）→ 拒收
    guest_ctx
        .send(ControlMsg::SendClip {
            clip_type: "text".into(),
            payload: b"oversized".to_vec(),
            category_name: Some("x".repeat(81)),
            category_color: None,
            display_name: None,
        })
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !store.list_categories().unwrap().iter().any(|c| c.name.len() == 81),
        "超长分组名不得落库"
    );

    // 同一会话继续发合法分组 → 必须成功（证明会话未被超长帧打断）
    guest_ctx
        .send(ControlMsg::SendClip {
            clip_type: "text".into(),
            payload: b"valid".to_vec(),
            category_name: Some("valid-cat".into()),
            category_color: Some("#0D9488".into()),
            display_name: None,
        })
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if store.list_categories().unwrap().iter().any(|c| c.name == "valid-cat") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "合法帧在超长帧之后必须仍被处理");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let _ = host_manager.control_tx().unwrap().send(ControlMsg::Disconnect).await;
    let _ = guest_ctx.send(ControlMsg::Disconnect).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), host_task).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), guest_task).await;
}

/// 回归（最终审查 Critical #1）：预认证阶段收到坏 base64 公钥的 Handshake，
/// host 不得 reset 整个监听——仅断开该连接，host 状态保持 Hosting。
///
/// 注：测试模式下 accept 循环单次 accept 后退出（见 start_host_on 的 cfg!(test)
/// 断点，规避测试环境里持续 accept 的运行时冻结），故本测试用「状态不被重置 +
/// 恶意连接被断开」钉住缺陷：修复前该分支调用 reset_to_idle，状态翻成 Idle、
/// host 任务被 abort，Hosting 断言即失败。
#[tokio::test(flavor = "multi_thread")]
async fn malformed_pubkey_frame_does_not_stop_hosting() {
    use std::sync::Arc;

    use crate::lan_sync::server::start_host_on;
    use crate::lan_sync::session::Connection;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;
    use tokio::net::TcpStream;

    let code = "REALCODE";
    let host_manager = Arc::new(LanSessionManager::new_for_test());
    let store = temp_store();
    let listen_addr = start_host_on(host_manager.clone(), store, code.to_string(), 0)
        .await
        .unwrap();
    let port: u16 = listen_addr.rsplit(':').next().unwrap().parse().unwrap();

    // 恶意对端：读到挑战后回一个坏公钥的 Handshake
    let attacker = tokio::spawn(async move {
        let mut conn = Connection::new(TcpStream::connect(format!("127.0.0.1:{port}")).await.unwrap());
        let (msg, _) = conn.read_message().await.unwrap();
        assert!(matches!(msg, LanMessage::PairChallenge { .. }));
        conn.write_message(
            &LanMessage::Handshake {
                version: LAN_PROTOCOL_VERSION,
                device_name: "attacker".into(),
                guest_pubkey: Some("!!!not-base64!!!".into()),
                guest_proof: Some("deadbeef".into()),
            },
            None,
        )
        .await
        .unwrap();
        // host 应直接断开本连接（读到 EOF/错误）且不回任何帧；无论读到什么到这里都是异常
        let reply = conn.read_message().await;
        panic!("malformed pubkey peer must be dropped without any reply, got: {reply:?}");
    });

    // host 状态必须保持 Hosting（修复前坏公钥分支 reset_to_idle → Idle，本断言失败即回归）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        assert_eq!(
            host_manager.snapshot().status,
            LanStatus::Hosting,
            "host must keep Hosting after malformed pubkey frame"
        );
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // 恶意连接应被 host 快速断开（任务以 panic 收场即代表连接被断、无回复帧）
    let attacked = tokio::time::timeout(std::time::Duration::from_secs(5), attacker)
        .await
        .expect("host must drop the malformed-pubkey peer promptly");
    assert!(attacked.is_err(), "attacker must be dropped, not left hanging or replied to");
}

/// 回归（用户报告）：对端 accept 后立即关闭连接（模拟 host 断开/退出时的 EOF/RST）。
/// guest 不得误报「对方版本过旧」——修复前所有非挑战结果一律报成版本过旧，
/// 实际双方版本相同。应报连接类原因并回到 Idle。
#[tokio::test(flavor = "multi_thread")]
async fn peer_close_during_challenge_wait_reports_connection_error_not_version() {
    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let closing = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        drop(stream); // accept 后立刻关闭 → guest 的挑战读取得到 EOF
    });

    let (guest_manager, sink) = LanSessionManager::new_capturing_for_test();
    let store = temp_store();
    join_by_address(
        guest_manager.clone(),
        store,
        format!("127.0.0.1:{port}"),
        "TESTCD".to_string(),
    )
    .await;

    assert_eq!(guest_manager.snapshot().status, LanStatus::Idle);
    let reasons = sink.join_failed_reasons();
    assert_eq!(reasons.len(), 1, "应恰好 emit 一次 join-failed：{reasons:?}");
    assert!(
        !reasons[0].contains("版本过旧"),
        "对端关闭连接不得误报版本过旧：{}",
        reasons[0]
    );
    assert!(reasons[0].contains("连接"), "应提示连接类原因：{}", reasons[0]);
    closing.abort();
}

/// host 静默（v3 只等不答）触发真超时：提示应指向「版本过旧」并说明是超时。
#[tokio::test(flavor = "multi_thread")]
async fn challenge_timeout_message_mentions_version_and_timeout() {
    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let silent = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    });

    let (guest_manager, sink) = LanSessionManager::new_capturing_for_test();
    let store = temp_store();
    join_by_address(
        guest_manager.clone(),
        store,
        format!("127.0.0.1:{port}"),
        "TESTCD".to_string(),
    )
    .await;

    assert_eq!(guest_manager.snapshot().status, LanStatus::Idle);
    let reasons = sink.join_failed_reasons();
    assert_eq!(reasons.len(), 1, "应恰好 emit 一次 join-failed：{reasons:?}");
    assert!(reasons[0].contains("版本"), "超时仍应提示可能版本过旧：{}", reasons[0]);
    assert!(reasons[0].contains("超时"), "应说明是等待超时：{}", reasons[0]);
    silent.abort();
}

/// 挑战帧版本不符（对端真跑老协议）：确定报「对方版本过旧」。
#[tokio::test(flavor = "multi_thread")]
async fn old_version_challenge_reports_version_too_old() {
    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let old_host = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = Connection::new(stream);
        conn.write_message(
            &LanMessage::PairChallenge {
                version: 3,
                host_device_name: "old-host".into(),
                host_pubkey: Some("QUJD".into()),
            },
            None,
        )
        .await
        .unwrap();
        // 保持连接打开，让 guest 走「版本不符」分支而非「连接断开」
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });

    let (guest_manager, sink) = LanSessionManager::new_capturing_for_test();
    let store = temp_store();
    join_by_address(
        guest_manager.clone(),
        store,
        format!("127.0.0.1:{port}"),
        "TESTCD".to_string(),
    )
    .await;

    assert_eq!(guest_manager.snapshot().status, LanStatus::Idle);
    let reasons = sink.join_failed_reasons();
    assert_eq!(reasons.len(), 1, "应恰好 emit 一次 join-failed：{reasons:?}");
    assert!(reasons[0].contains("版本过旧"), "版本不符应明确报版本过旧：{}", reasons[0]);
    old_host.abort();
}

/// reset_to_idle 必须中止已登记的 join 任务（lan_disconnect 的 WaitingPair 分支
/// 走此路径）：用户断开后，残留握手任务不得再跑完并覆写状态。
#[tokio::test(flavor = "multi_thread")]
async fn reset_to_idle_aborts_registered_join_task() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use crate::lan_sync::LanSessionManager;

    let manager = LanSessionManager::new_for_test();
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let zombie = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        flag.store(true, Ordering::SeqCst);
    });
    manager.set_join_task(zombie);
    manager.reset_to_idle("test".to_string());
    // 若未被 abort，任务 500ms 后置位标志；被 abort 则永远不会。
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        !finished.load(Ordering::SeqCst),
        "登记的 join 任务应被 abort，不得继续运行"
    );
}

/// 端到端时序回归（用户报告的「A 断开后才弹错」）：join 等待挑战期间用户断开
/// （reset_to_idle）——残留 join 任务应被 abort，之后不得再 emit lan-join-failed。
#[tokio::test(flavor = "multi_thread")]
async fn disconnect_during_join_aborts_task_without_late_failure_event() {
    use crate::lan_sync::client::join_by_address;
    use crate::lan_sync::{LanSessionManager, LanStatus};
    use crate::store::test_support::temp_store;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let silent = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    });

    let (manager, sink) = LanSessionManager::new_capturing_for_test();
    let store = temp_store();
    let join = tokio::spawn(join_by_address(
        manager.clone(),
        store,
        format!("127.0.0.1:{port}"),
        "TESTCD".to_string(),
    ));
    manager.set_join_task(join);
    // 等 join 任务进入 WaitingPair（try_set_joining 已生效），再等连接建立进入挑战等待
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "join 任务应及时进入 WaitingPair，实际 {:?}",
            manager.snapshot().status
        );
        if matches!(manager.snapshot().status, LanStatus::WaitingPair) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 模拟 lan_disconnect 的 WaitingPair 分支：abort join 任务 + reset
    manager.reset_to_idle("已断开".to_string());

    // 超过测试期挑战超时（2s）后：没有迟到的 join-failed（若任务未被 abort，
    // 它会在 ~2.4s 时 emit join-failed，下面的断言即失败）
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(
        sink.join_failed_reasons().is_empty(),
        "断开后不得再 emit join-failed，实际：{:?}",
        sink.join_failed_reasons()
    );
    silent.abort();
}
