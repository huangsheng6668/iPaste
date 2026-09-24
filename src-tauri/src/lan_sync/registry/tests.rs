//! registry 单元测试（原 registry.rs 内联 `mod tests` 整体外移，纯移动）。
//!
//! 被测项经 `super::` 显式导入：父模块私有项对子模块天然可见；后续拆出的
//! 域子模块（pairing/link/sender）中需跨模块访问的项以 pub(super) 暴露。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iroh::SecretKey;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::events::{
    EVENT_DEVICE_CATEGORY_SENT, EVENT_DEVICE_STATUS_CHANGED, EVENT_PAIR_INVITE_STATE,
    EVENT_PAIR_JOIN_FAILED, EVENT_PAIR_REQUEST,
};
use crate::lan_sync::frame::{read_message_with_raw, PrefixedFrame};
use crate::lan_sync::protocol::{LanMessage, LAN_MAX_PAYLOAD};
use crate::lan_sync::ticket::{PairTicket, INVITE_TTL};
use crate::lan_sync::{ControlMsg, LanEventSink};
use crate::models::{AutoSyncMode, ClipItem, DeviceOnline};
use crate::store::test_support::temp_store;

use super::link::reconnect_backoff;
use super::pairing::PendingPair;
use super::sender::try_send_auto;
use super::{
    build_send_payload, endpoint_id_from_hex, hex_encode_32, max_sendable_image_bytes,
    DeviceLinkRegistry, LinkHandle,
};

    /// 记录型事件出口：捕获 (事件名, payload) 供断言。
    #[derive(Clone, Default)]
    struct RecordingSink(Arc<std::sync::Mutex<Vec<(String, Value)>>>);

    impl LanEventSink for RecordingSink {
        fn emit(&self, event: &str, payload: &Value) {
            self.0.lock().unwrap().push((event.to_string(), payload.clone()));
        }
    }

    fn recorded(sink: &RecordingSink) -> Vec<(String, Value)> {
        sink.0.lock().unwrap().clone()
    }

    fn find_events(sink: &RecordingSink, event: &str) -> Vec<Value> {
        recorded(sink)
            .into_iter()
            .filter(|(name, _)| name == event)
            .map(|(_, payload)| payload)
            .collect()
    }

    fn hex32(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    async fn test_registry() -> (Arc<DeviceLinkRegistry>, RecordingSink) {
        let sink = RecordingSink::default();
        let registry =
            DeviceLinkRegistry::start_for_test(temp_store(), Arc::new(sink.clone()))
                .await
                .expect("test registry");
        (registry, sink)
    }

    /// 在 links 里伪造一条 Connected 登记（持有真实控制通道），返回接收端。
    fn fake_connected_link(
        registry: &DeviceLinkRegistry,
        node_id: &str,
    ) -> mpsc::Receiver<ControlMsg> {
        let (tx, rx) = mpsc::channel(16);
        registry
            .inner
            .links
            .lock()
            .unwrap()
            .insert(
                node_id.to_string(),
                LinkHandle {
                    gen: registry.next_gen(),
                    control_tx: Some(tx),
                    status: DeviceOnline::Connected,
                    batch_busy: false,
                    task: None,
                },
            );
        rx
    }

    /// 退避序列：5,10,20,40,80,160，之后恒为 300。
    #[test]
    fn reconnect_backoff_progression() {
        assert_eq!(
            (0..6).map(reconnect_backoff).collect::<Vec<_>>(),
            vec![
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(20),
                Duration::from_secs(40),
                Duration::from_secs(80),
                Duration::from_secs(160),
            ]
        );
        assert_eq!(reconnect_backoff(6), Duration::from_secs(300));
        assert_eq!(reconnect_backoff(100), Duration::from_secs(300));
    }

    /// build_send_payload：文本条目直接转 UTF-8 字节。
    #[test]
    fn build_send_payload_text_returns_utf8_bytes() {
        let payload = build_send_payload("text", "hello-api-key").unwrap();
        assert_eq!(payload, b"hello-api-key");
    }

    /// build_send_payload：图片条目读回文件并编码成自包含 data url。
    #[test]
    fn build_send_payload_image_reads_file_and_encodes_data_url() {
        use crate::clipboard::image_bytes_from_data_url;

        // 建一个临时 png 文件模拟 DB 里图片条目的 text（文件路径）。
        let dir = std::env::temp_dir().join(format!("ipaste-send-payload-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("img.png");
        // 1x1 透明 png 的最小字节
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // png signature
        ];
        std::fs::write(&png_path, png_bytes).unwrap();

        let payload = build_send_payload("image", png_path.to_str().unwrap()).unwrap();
        let text = String::from_utf8(payload).unwrap();
        assert!(
            text.starts_with("data:image/png;base64,"),
            "图片 payload 应是 data url，实际：{text}"
        );
        // 解码回来的字节与原文件一致
        let decoded = image_bytes_from_data_url(&text).unwrap();
        assert_eq!(decoded, png_bytes);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// build_send_payload：文件缺失返回可读错误，而非 panic 或发空 payload。
    #[test]
    fn build_send_payload_image_missing_file_errors() {
        let result = build_send_payload("image", "/definitely/not/here/xyz.png");
        assert!(result.is_err());
    }

    /// build_send_payload：超限图片在读文件前拒绝，避免无意义的整文件读入
    /// 与注定失败的传输。
    #[test]
    fn build_send_payload_rejects_image_exceeding_frame_limit() {
        let dir = std::env::temp_dir().join(format!("ipaste-send-limit-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big_path = dir.join("big.png");
        std::fs::write(&big_path, vec![0u8; LAN_MAX_PAYLOAD]).unwrap();

        let error = build_send_payload("image", big_path.to_str().unwrap())
            .expect_err("超限图片应被拒绝");
        assert!(error.contains("过大"), "got: {error}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// build_send_payload：恰好编码后不超上限的文件可正常构建 payload。
    #[test]
    fn build_send_payload_allows_image_at_frame_limit() {
        let dir = std::env::temp_dir().join(format!("ipaste-send-bound-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edge.png");
        std::fs::write(&path, vec![0u8; max_sendable_image_bytes() as usize]).unwrap();

        let payload = build_send_payload("image", path.to_str().unwrap()).unwrap();
        assert!(payload.len() <= LAN_MAX_PAYLOAD);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// create_invite：票据可解码回本端 EndpointId，emit 的 payload 带 ticket 与
    /// 未来时间戳；cancel_invite 后 emit {None, None}。
    #[tokio::test]
    async fn create_and_cancel_invite_emit_state() {
        let (registry, sink) = test_registry().await;
        let ticket = registry.create_invite().await.expect("invite");
        assert!(ticket.starts_with("ipaste-pair-v1:"));
        let decoded = PairTicket::decode(&ticket).expect("decodable ticket");
        assert_eq!(decoded.endpoint_id, *registry.inner.endpoint.id().as_bytes());
        // RelayMode::Disabled（relay_disabled=true 跳过 online 等待）：票据无中继，
        // 只有直连地址——即 online() 超时兜底的 LAN-only 形态，必须依然可解析。
        assert_eq!(decoded.relay_url, None);
        assert!(!decoded.direct_addrs.is_empty(), "本端直连地址应进票据");

        let events = find_events(&sink, EVENT_PAIR_INVITE_STATE);
        assert_eq!(events.len(), 1, "create_invite 应 emit 一次邀请状态");
        assert_eq!(events[0]["ticket"].as_str(), Some(ticket.as_str()));
        let expires_at = events[0]["expiresAt"].as_u64().expect("expiresAt 为数字");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(
            expires_at > now_ms && expires_at <= now_ms + INVITE_TTL.as_millis() as u64 + 5_000,
            "expires_at 应在未来 10 分钟内"
        );

        registry.cancel_invite().expect("cancel");
        let events = find_events(&sink, EVENT_PAIR_INVITE_STATE);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1]["ticket"].as_str(), None);
        assert_eq!(events[1]["expiresAt"].as_u64(), None);
    }

    /// 无 pending 配对请求时 respond_pair 报错。
    #[tokio::test]
    async fn respond_pair_without_pending_errors() {
        let (registry, _sink) = test_registry().await;
        let err = registry.respond_pair(true).expect_err("无 pending 应报错");
        assert!(err.contains("当前没有待确认的配对请求"), "实际错误：{err}");
    }

    /// respond_pair：take 语义——决定送达 oneshot，第二次调用报错。
    #[tokio::test]
    async fn respond_pair_resolves_pending_once() {
        let (registry, _sink) = test_registry().await;
        let (decision_tx, decision_rx) = oneshot::channel();
        *registry.inner.pending_pair.lock().unwrap() = Some(PendingPair {
            device_name: "MBP".into(),
            node_id: hex32(1),
            decision_tx,
        });
        registry.respond_pair(true).expect("第一次应成功");
        assert_eq!(decision_rx.await, Ok(true));
        assert!(registry.respond_pair(true).is_err(), "pending 已消费，第二次报错");
    }

    /// device_infos：store 行 + links 状态合成；撤销恒 Offline。
    #[tokio::test]
    async fn device_infos_merges_link_status() {
        let (registry, _sink) = test_registry().await;
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(1), "MBP", None, &[])
            .unwrap();
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(2), "PC", None, &[])
            .unwrap();
        registry
            .inner
            .store
            .upsert_paired_device(&hex32(3), "Old", None, &[])
            .unwrap();
        registry.inner.store.revoke_device(&hex32(3)).unwrap();

        let _rx = fake_connected_link(&registry, &hex32(1));
        let infos = registry.device_infos();
        assert_eq!(infos.len(), 3);
        let by_node: std::collections::HashMap<&str, DeviceOnline> = infos
            .iter()
            .map(|info| (info.device.node_id.as_str(), info.online))
            .collect();
        assert_eq!(by_node[hex32(1).as_str()], DeviceOnline::Connected, "有活跃登记 → Connected");
        assert_eq!(by_node[hex32(2).as_str()], DeviceOnline::Offline, "无登记 → Offline");
        assert_eq!(by_node[hex32(3).as_str()], DeviceOnline::Offline, "撤销恒 Offline");
    }

    /// send_raw：无在线目标（None / 指定不存在）都报「没有在线的目标设备」。
    #[tokio::test]
    async fn send_raw_without_online_target_errors() {
        let (registry, _sink) = test_registry().await;
        let err = registry
            .send_raw(None, "text", b"hi", None, None, None)
            .await
            .expect_err("无在线目标应报错");
        assert_eq!(err, "没有在线的目标设备");
        let err = registry
            .send_raw(Some(&hex32(9)), "text", b"hi", None, None, None)
            .await
            .expect_err("指定设备不在线应报错");
        assert_eq!(err, "没有在线的目标设备");
    }

    /// send_category：空分组报「该分组没有可发送的条目」。
    #[tokio::test]
    async fn send_category_empty_category_errors() {
        let (registry, _sink) = test_registry().await;
        let category = registry
            .inner
            .store
            .create_category("空分组".into(), "#9CA3AF".into())
            .unwrap();
        let _rx = fake_connected_link(&registry, &hex32(1));
        let err = registry
            .send_category(Some(&hex32(1)), &category.id)
            .await
            .expect_err("空分组应报错");
        assert_eq!(err, "该分组没有可发送的条目");
    }

    /// send_category：装配 + 分发 + 汇总事件（用伪造 link 通道验证控制指令序列）。
    #[tokio::test]
    async fn send_category_dispatches_batch_messages() {
        let (registry, sink) = test_registry().await;
        let store = &registry.inner.store;
        store
            .insert_received_category_item(
                "text".into(),
                "hash-1".into(),
                "preview".into(),
                "第一条".into(),
                "工作".into(),
                Some("#0D9488".into()),
                None,
                None,
            )
            .unwrap();
        store
            .insert_received_category_item(
                "text".into(),
                "hash-2".into(),
                "preview".into(),
                "第二条".into(),
                "工作".into(),
                Some("#0D9488".into()),
                Some("改名".into()),
                None,
            )
            .unwrap();
        // insert_received_category_item 按名称建分组；查回分组 id
        let conn = store.connect().unwrap();
        let cat_id: String = conn
            .query_row(
                "SELECT id FROM categories WHERE name = '工作'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);

        let mut rx = fake_connected_link(&registry, &hex32(1));
        let (name, sent, failed) = registry
            .send_category(Some(&hex32(1)), &cat_id)
            .await
            .expect("整组发送成功");
        assert_eq!(name, "工作");
        assert_eq!(sent, 2, "两条全部送达");
        assert_eq!(failed, 0);

        // 控制指令序列：BatchStart → SendClip × 2 → BatchEnd。
        // 两条 SendClip 的相对顺序由分组内 sort_order 决定（后插入的在最上），
        // 断言按内容集合而非固定顺序。
        let msgs: Vec<ControlMsg> = {
            let mut collected = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                collected.push(msg);
            }
            collected
        };
        assert_eq!(msgs.len(), 4, "BatchStart + 2×SendClip + BatchEnd");
        assert!(matches!(&msgs[0], ControlMsg::BatchStart { category_name, item_count, .. }
            if category_name == "工作" && *item_count == 2));
        let sent_payloads: Vec<(Vec<u8>, Option<String>)> = msgs[1..3]
            .iter()
            .filter_map(|msg| match msg {
                ControlMsg::SendClip { payload, display_name, category_name, category_color, .. } => {
                    assert_eq!(category_name.as_deref(), Some("工作"));
                    assert_eq!(category_color.as_deref(), Some("#0D9488"));
                    Some((payload.clone(), display_name.clone()))
                }
                _ => None,
            })
            .collect();
        let as_text = |entry: &(Vec<u8>, Option<String>)| {
            (String::from_utf8_lossy(&entry.0).to_string(), entry.1.clone())
        };
        let mut got: Vec<(String, Option<String>)> = sent_payloads.iter().map(as_text).collect();
        got.sort();
        let mut want = vec![
            ("第一条".to_string(), None),
            ("第二条".to_string(), Some("改名".to_string())),
        ];
        want.sort();
        assert_eq!(got, want);
        assert!(matches!(msgs[3], ControlMsg::BatchEnd));

        // 汇总事件：DeviceCategorySent 携带 node_id / 组名 / 计数
        let events = find_events(&sink, EVENT_DEVICE_CATEGORY_SENT);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["nodeId"].as_str(), Some(hex32(1).as_str()));
        assert_eq!(events[0]["categoryName"].as_str(), Some("工作"));
        assert_eq!(events[0]["sent"].as_u64(), Some(2));
        assert_eq!(events[0]["failed"].as_u64(), Some(0));
    }

    /// hex 往返：endpoint_id_from_hex(hex_encode_32(id)) == id；非法输入 None。
    /// 注意 EndpointId::from_bytes 会校验 ed25519 曲线点，全 0x5a 之类非法字节会
    /// 被拒——用真实生成的密钥测往返。
    #[test]
    fn endpoint_id_hex_roundtrip() {
        let id = SecretKey::generate().public();
        assert_eq!(
            endpoint_id_from_hex(&hex_encode_32(id.as_bytes())).map(|p| *p.as_bytes()),
            Some(*id.as_bytes())
        );
        assert!(endpoint_id_from_hex("zz").is_none());
        assert!(endpoint_id_from_hex(&"ab".repeat(31)).is_none());
        // 非法曲线点（全 0x5a）在 from_bytes 层被拒 → None
        assert!(endpoint_id_from_hex(&hex_encode_32(&[0x5a; 32])).is_none());
    }

    /// start_for_test 端点可建可用：EndpointId 是 32 字节公钥的稳定封装。
    #[tokio::test]
    async fn test_registry_binds_endpoint() {
        let (registry, _sink) = test_registry().await;
        let id = registry.inner.endpoint.id();
        assert_eq!(id.as_bytes().len(), 32);
        assert_eq!(hex_encode_32(id.as_bytes()).len(), 64);
        // shutdown 幂等且不 panic
        registry.shutdown();
        registry.shutdown();
    }

    /// 互踢震荡修复（Fix 1）：入站会话在线（Connected + 活跃控制通道）时，
    /// link_task 不得拨号——拨号路径的第一步就是 set_status(Connecting) 事件，
    /// 若未跳过会立刻观察到状态事件并随后 Offline（拨号指向不可达地址必失败）。
    #[tokio::test]
    async fn link_task_skips_dial_when_inbound_session_live() {
        let (registry, sink) = test_registry().await;
        let node = hex32(1);
        // 地址指向 TEST-NET（不可达）：若 link_task 误拨号，Connecting → 15s 内 Offline
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &["203.0.113.1:1".into()])
            .unwrap();
        let (tx, _rx) = mpsc::channel(16);
        registry.inner.links.lock().unwrap().insert(
            node.clone(),
            LinkHandle {
                gen: registry.next_gen(),
                control_tx: Some(tx),
                status: DeviceOnline::Connected,
                batch_busy: false,
                task: None,
            },
        );
        let task = tokio::spawn(registry.clone().link_task(node.clone()));
        // 留足调度余量：未跳过时 Connecting 事件在 spawn 后毫秒级出现
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert!(
            statuses.is_empty(),
            "入站会话在线时 link_task 不应拨号（不应出现任何状态事件）：{statuses:?}"
        );
        assert_eq!(
            registry.device_infos()[0].online,
            DeviceOnline::Connected,
            "在线登记不被 link_task 覆写"
        );
        // 会话结束后（drop 控制通道）跳过条件解除——此处仅验证跳过路径本身可退出
        task.abort();
    }

    /// 互踢震荡修复（Fix Round 2）：真实的 run_session 被入站会话收编后结束时，
    /// 不得覆写存活会话的状态、不得发假日 Offline 事件、link_task 停靠不重拨。
    /// 这是对「对端重拨落地 → 本端会话死 → 本端 link_task 醒来」完整时序的复刻：
    /// 旧实现的 set_status(Offline) 在此刻把入站会话的 Connected 砸成 Offline，
    /// 进而在循环顶骗过 guard 引发重拨互踢。
    #[tokio::test]
    async fn superseded_session_end_keeps_inbound_session_and_parks_link_task() {
        use tokio::io::duplex;
        let (registry, sink) = test_registry().await;
        let node = hex32(3);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &["203.0.113.1:1".into()])
            .unwrap();

        // 1) 真实 run_session 在 duplex 流上运行（link_task 的会话入口，非伪造登记）
        let (client, server) = duplex(4096);
        let (read_half, write_half) = tokio::io::split(server);
        let (_dead_tx, dead_rx) = oneshot::channel();
        let session_registry = registry.clone();
        let session_node = node.clone();
        let session = tokio::spawn(async move {
            session_registry
                .run_session(None, session_node, read_half, write_half, dead_rx)
                .await
        });
        let mut polls = 0;
        let my_gen = loop {
            let registered = {
                let links = registry.inner.links.lock().unwrap();
                links.get(&node).and_then(|handle| {
                    handle.control_tx.is_some().then_some(handle.gen)
                })
            };
            if let Some(gen) = registered {
                break gen;
            }
            polls += 1;
            assert!(polls < 100, "会话应在 2s 内注册");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(registry.device_infos()[0].online, DeviceOnline::Connected);

        // 2) 对端重拨落地：入站会话收编登记（新 gen + 活跃 control_tx + Connected）
        let (inbound_tx, _inbound_rx) = mpsc::channel(16);
        let inbound_gen = registry.next_gen();
        {
            let mut links = registry.inner.links.lock().unwrap();
            let handle = links.get_mut(&node).expect("登记存在");
            handle.gen = inbound_gen;
            handle.control_tx = Some(inbound_tx);
            handle.status = DeviceOnline::Connected;
        }

        // 3) 旧会话结束（收编时其唯一 sender 被 drop → 会话循环收到 None 退出；EOF 兜底）
        drop(client);
        let ended_gen = session.await.unwrap();
        assert_eq!(ended_gen, my_gen, "run_session 应返回本会话的 gen");

        // 4) 登记原封不动：仍是入站会话的 gen + 活跃通道 + Connected
        {
            let links = registry.inner.links.lock().unwrap();
            let handle = links.get(&node).expect("登记未被移除");
            assert_eq!(handle.gen, inbound_gen);
            assert!(handle.control_tx.is_some());
            assert_eq!(
                handle.status,
                DeviceOnline::Connected,
                "被收编的会话结束不得覆写存活会话的状态"
            );
        }

        // 5) link_task 会话后的 Offline 写（凭据 = 旧会话 gen）必须 no-op
        registry.set_status_if_owner(&node, ended_gen, DeviceOnline::Offline);
        assert_eq!(
            registry.inner.links.lock().unwrap().get(&node).unwrap().status,
            DeviceOnline::Connected
        );

        // 6) 无假日 Offline 事件：至此状态事件只有会话建立时的 Connected 一条
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert_eq!(statuses.len(), 1, "被收编会话的结束不应发 Offline 事件：{statuses:?}");
        assert_eq!(statuses[0]["status"].as_str(), Some("connected"));

        // 7) link_task 醒来后在 guard 停靠：不拨号、不再新增状态事件
        let task = tokio::spawn(registry.clone().link_task(node.clone()));
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let statuses = find_events(&sink, EVENT_DEVICE_STATUS_CHANGED);
        assert_eq!(statuses.len(), 1, "入站会话在线时 link_task 不得重拨：{statuses:?}");
        assert!(registry.has_live_session(&node));
        assert_eq!(registry.device_infos()[0].online, DeviceOnline::Connected);
        task.abort();
    }

    /// 持久断开（Fix 2）：disconnect() 后会话准入门关闭，重新配对/撤销/删除解除。
    /// 真实入站连接的拒绝由 handle_inbound 的首帧路由 + run_session 登记前复验
    ///（A1）的同一道门（session_admission_denied）保证，Task 9 的双端集成测试覆盖。
    #[tokio::test]
    async fn disconnect_blocks_inbound_until_repair() {
        let (registry, _sink) = test_registry().await;
        let node = hex32(2);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        assert!(
            registry.session_admission_denied(&node).is_none(),
            "已配对未断开：允许入站会话"
        );

        registry.disconnect(&node);
        assert_eq!(
            registry.session_admission_denied(&node),
            Some(&b"disconnected"[..]),
            "显式断开后：已配对也静默拒绝入站会话"
        );
        assert!(registry.inner.disconnected.lock().unwrap().contains(&node));
        // store 行保留（断开 ≠ 撤销），设备列表仍可见
        assert_eq!(registry.device_infos().len(), 1);

        // 重新配对成功（join/accept 路径的清除点）→ 恢复
        registry.clear_disconnected(&node);
        assert!(
            registry.session_admission_denied(&node).is_none(),
            "重新配对后恢复入站"
        );

        // 撤销/删除也清标记（tidiness：撤销行本身即拒绝入站）
        registry.disconnect(&node);
        registry.revoke(&node);
        assert!(!registry.inner.disconnected.lock().unwrap().contains(&node));
        assert_eq!(
            registry.session_admission_denied(&node),
            Some(&b"revoked"[..]),
            "撤销后按 revoked 拒绝"
        );
    }

    /// A1（TOCTOU）：撤销在「入站门检查之后、run_session 登记之前」落地的竞态
    /// 窗口——登记临界区内复验必须拦下：无登记、无 Connected 状态/事件。
    /// 复现方式：直接对 store 撤销（绕过 kill_link），再驱动 run_session。
    #[tokio::test]
    async fn run_session_aborts_registration_when_revoked_before_link() {
        let (registry, sink) = test_registry().await;
        let node = hex32(7);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        // 模拟 revoke 的 store 写入在连接放行后、登记前落地（kill_link 无条目可杀）
        registry.inner.store.revoke_device(&node).unwrap();

        let (client, server) = tokio::io::duplex(4096);
        let (read_half, write_half) = tokio::io::split(server);
        let (_dead_tx, dead_rx) = oneshot::channel();
        let gen = registry
            .clone()
            .run_session(None, node.clone(), read_half, write_half, dead_rx)
            .await;

        // 无幽灵登记：撤销设备不得顶着 Connected 会话继续收推送
        assert!(
            registry.inner.links.lock().unwrap().get(&node).is_none(),
            "撤销后 run_session 不得登记条目"
        );
        // 无 Connected 状态事件（也无所谓 Offline——从未在线）
        assert!(
            find_events(&sink, EVENT_DEVICE_STATUS_CHANGED).is_empty(),
            "被拒会话不得发任何状态事件：{:?}",
            find_events(&sink, EVENT_DEVICE_STATUS_CHANGED)
        );
        // store 行仍在（撤销 ≠ 删除），设备列表恒 Offline
        let infos = registry.device_infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].online, DeviceOnline::Offline);
        assert!(infos[0].device.revoked_at.is_some());
        // 返回的 gen 从未登记：后续按 gen 的状态写必然 no-op（防御性确认）
        registry.set_status_if_owner(&node, gen, DeviceOnline::Offline);
        assert!(
            registry.inner.links.lock().unwrap().get(&node).is_none(),
            "按未登记 gen 的状态写不得凭空创建条目"
        );
        drop(client);
    }

    /// A1（TOCTOU）的 disconnect 分支：显式断开标记落在登记之前 → 同样拒绝，
    /// 关闭原因区分 revoked / disconnected（conn=None 时靠流 drop，行为一致）。
    #[tokio::test]
    async fn run_session_aborts_registration_when_disconnected_before_link() {
        let (registry, sink) = test_registry().await;
        let node = hex32(8);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        // 模拟 disconnect() 的标记写入在连接放行后、登记前落地（kill_link 已错过）
        registry
            .inner
            .disconnected
            .lock()
            .unwrap()
            .insert(node.clone());

        let (client, server) = tokio::io::duplex(4096);
        let (read_half, write_half) = tokio::io::split(server);
        let (_dead_tx, dead_rx) = oneshot::channel();
        registry
            .clone()
            .run_session(None, node.clone(), read_half, write_half, dead_rx)
            .await;

        assert!(
            registry.inner.links.lock().unwrap().get(&node).is_none(),
            "显式断开后 run_session 不得登记条目"
        );
        assert!(
            find_events(&sink, EVENT_DEVICE_STATUS_CHANGED).is_empty(),
            "被拒会话不得发任何状态事件"
        );
        // 行未撤销：设备列表仍可见但 Offline
        assert_eq!(registry.device_infos().len(), 1);
        assert_eq!(registry.device_infos()[0].online, DeviceOnline::Offline);
        assert!(registry.device_infos()[0].device.revoked_at.is_none());
        drop(client);
    }

    /// A2 回放适配器：路由层先从流中消费首帧，前缀（首帧线格式字节）+ 流的
    /// 剩余字节拼接后，帧序列不丢不重（Ping 恰好一次 + ClipPush 完整一次）。
    #[tokio::test]
    async fn prefixed_frame_replays_first_frame_without_loss_or_duplication() {
        use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

        // 对端写两帧（Ping + 带 payload 的 ClipPush），路由层只消费第一帧
        let (mut peer, mut inner) = duplex(8192);
        let ping_wire = {
            let header = br#"{"kind":"ping"}"#.to_vec();
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&header);
            bytes
        };
        let mut expected = ping_wire.clone();
        let clip_wire = {
            let header = br#"{"kind":"clipPush","clip_type":"text","empty":false}"#.to_vec();
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(&(5u32).to_le_bytes());
            bytes.extend_from_slice(b"hello");
            bytes
        };
        expected.extend_from_slice(&clip_wire);
        peer.write_all(&ping_wire).await.unwrap();
        peer.write_all(&clip_wire).await.unwrap();

        // 路由层消费首帧（与 handle_inbound 的 read_message_with_raw 同路径）
        let (msg, raw) = read_message_with_raw(&mut inner).await.expect("首帧可读");
        assert!(matches!(msg, LanMessage::Ping), "首帧应为 Ping");
        peer.shutdown().await.unwrap(); // 关对端写半：回放流最终读到 EOF

        let mut replay = PrefixedFrame::new(raw, inner);
        let mut got = Vec::new();
        let n = replay.read_to_end(&mut got).await.unwrap();
        assert_eq!(n, expected.len());
        assert_eq!(got, expected, "前缀 + 流的剩余字节应原样拼接，不丢不重");
    }

    /// 撤销门 helper（F2）：只有「行存在且 revoked_at 非空」才拦；
    /// 无记录/未撤销/已删除都不拦（删除后重新视为陌生设备）。
    #[tokio::test]
    async fn locally_revoked_guard_distinguishes_states() {
        let (registry, _sink) = test_registry().await;
        let node = hex32(5);
        assert!(!registry.is_locally_revoked(&node), "无记录：不拦（走陌生配对流程）");
        registry
            .inner
            .store
            .upsert_paired_device(&node, "MBP", None, &[])
            .unwrap();
        assert!(!registry.is_locally_revoked(&node), "已配对未撤销：不拦（走已配对分支）");
        registry.inner.store.revoke_device(&node).unwrap();
        assert!(registry.is_locally_revoked(&node), "已撤销：静默拒绝");
        registry.inner.store.delete_device(&node).unwrap();
        assert!(!registry.is_locally_revoked(&node), "已删除：重新视为陌生设备");
    }

    /// join 前置撤销门（F2）：票据指向本地已撤销设备 → 不拨号直接失败，
    /// 错误提示先删除记录；Err 与 EVENT_PAIR_JOIN_FAILED 双通道（与其他
    /// join 失败一致）。
    #[tokio::test]
    async fn join_fails_fast_when_target_locally_revoked() {
        let (registry, sink) = test_registry().await;
        let node = hex32(6);
        registry
            .inner
            .store
            .upsert_paired_device(&node, "旧设备", None, &[])
            .unwrap();
        registry.inner.store.revoke_device(&node).unwrap();
        // 票据 endpoint_id 只需 hex 对得上 store 行（不拨号，无需合法曲线点）
        let bytes = [0x06u8; 32];
        assert_eq!(hex_encode_32(&bytes), node);
        let ticket = PairTicket {
            version: 1,
            endpoint_id: bytes,
            relay_url: None,
            direct_addrs: vec![],
            invite_secret: [0u8; 16],
        };
        let err = registry
            .join(&ticket.encode())
            .await
            .expect_err("已撤销目标应立即失败");
        assert!(err.contains("已被撤销"), "实际错误：{err}");
        let events = find_events(&sink, EVENT_PAIR_JOIN_FAILED);
        assert_eq!(events.len(), 1, "失败应 emit join-failed 事件");
        assert!(events[0]["reason"].as_str().unwrap().contains("已被撤销"));
    }

    /// join 应答等待超时（F5）：对端校验通过但停在用户确认（无人应答）时，
    /// join 不得无限挂起——JOIN_REPLY_TIMEOUT（测试构建 2s）后返回
    /// 「等待对方响应超时」并 emit join-failed；host 侧的 120s 用户确认
    /// 限时远晚于 join 超时，保证先在拨号方触发。
    #[tokio::test]
    async fn join_reply_wait_times_out() {
        let (host, host_sink) = test_registry().await;
        let (joiner, sink) = test_registry().await;
        // host 生成有效邀请：对端 PairRequest 校验通过 → 走到用户确认等待
        //（连接保持打开、不回帧），拨号方只能靠自己的 30s 超时收场
        let secret = host.inner.invites.lock().unwrap().create();
        let addr = host.inner.endpoint.addr();
        let direct_addrs: Vec<String> = addr.ip_addrs().map(|a| a.to_string()).collect();
        assert!(!direct_addrs.is_empty(), "回环端点应有直连地址");
        let ticket = PairTicket {
            version: 1,
            endpoint_id: *host.inner.endpoint.id().as_bytes(),
            relay_url: None,
            direct_addrs,
            invite_secret: secret,
        };
        let err = joiner.join(&ticket.encode()).await.expect_err("对端不回帧时应超时失败");
        assert_eq!(err, "等待对方响应超时");
        let events = find_events(&sink, EVENT_PAIR_JOIN_FAILED);
        assert_eq!(events.len(), 1, "超时失败应 emit join-failed");
        assert_eq!(events[0]["reason"].as_str(), Some("等待对方响应超时"));
        // host 确实停在用户确认（而不是提前断开）：已弹出配对请求事件
        assert!(
            !find_events(&host_sink, EVENT_PAIR_REQUEST).is_empty(),
            "host 应已进入用户确认等待（连接保持打开）"
        );
    }

    /// try_send_auto：队列满时计数丢弃、不阻塞（容量 1 塞满后第二条失败 → 计数 +1）。
    #[test]
    fn try_send_auto_counts_drop_when_channel_full() {
        let (tx, _rx) = mpsc::channel(1);
        let dropped = AtomicU64::new(0);
        assert!(try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "首条应入队");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        // 容量已满：同步返回失败并计数，绝不等待容量（fire-and-forget 保证）
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "满队应丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "丢弃计数 +1");
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "继续满队仍丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 2, "计数持续累计");
    }

    /// try_send_auto：会话已死（接收端 drop）同样计数丢弃——与满队列同一处理。
    #[test]
    fn try_send_auto_counts_drop_when_session_closed() {
        let (tx, rx) = mpsc::channel(16);
        drop(rx);
        let dropped = AtomicU64::new(0);
        assert!(!try_send_auto(&tx, ControlMsg::BatchEnd, &dropped), "死通道应丢弃");
        assert_eq!(dropped.load(Ordering::Relaxed), 1, "丢弃计数 +1");
    }

    /// 测试用 ClipItem 装配（fan_out_auto 的输入形态：text 字段为文本原文或
    /// 图片落盘路径——与捕获入库后的约定一致）。
    fn text_clip(clip_type: &str, hash: &str, text: &str) -> ClipItem {
        ClipItem {
            id: crate::util::new_id(),
            clip_type: clip_type.to_string(),
            content_hash: hash.to_string(),
            display_name: None,
            preview_text: text.chars().take(20).collect(),
            text: text.to_string(),
            source_app: None,
            last_captured_at: String::new(),
            favorite_count: 0,
            is_pinned: false,
        }
    }

    /// fan_out_auto 闸门：master 关 / recent 命中都在 payload 构建之前短路——
    /// 被抑制时即使图片路径不可读也不报错（build_send_payload 根本不会被调用）。
    #[tokio::test]
    async fn fan_out_auto_short_circuits_before_payload_build() {
        let (registry, _sink) = test_registry().await;
        let store = &registry.inner.store;
        // 图片条目的 text 指向不存在路径：若错误地先构建 payload 会返回 Err
        let bad_image = text_clip("image", "hash-bad", "/definitely/not/here/x.png");

        // 无在线候选：同样短路在 payload 构建之前（无人在线不读图片文件）
        store.update_auto_push_settings(true, false).unwrap();
        registry
            .fan_out_auto(&bad_image)
            .await
            .expect("无在线候选应短路返回 Ok");

        // master 关：静默 Ok，无投递
        store.update_auto_push_settings(false, false).unwrap();
        let mut rx = fake_connected_link(&registry, &hex32(1));
        registry
            .fan_out_auto(&bad_image)
            .await
            .expect("master 关应短路返回 Ok");
        assert!(rx.try_recv().is_err(), "master 关：无投递");

        // master 开 + recent 命中：同样短路在 payload 构建之前
        store.update_auto_push_settings(true, false).unwrap();
        registry.inner.recent.insert("hash-bad");
        registry
            .fan_out_auto(&bad_image)
            .await
            .expect("recent 命中应短路返回 Ok");
        assert!(rx.try_recv().is_err(), "recent 命中：无投递");
    }

    /// 批量互斥（Critical 发送侧）：链路的批量标记置位期间，fan_out_auto 跳过
    /// 该链路并按丢弃计数；标记清除后恢复投递。标记的置/清由 send_category
    /// 在 BatchStart 入队前 / BatchEnd 入队后完成（此处直接操纵登记模拟中段）。
    #[tokio::test]
    async fn fan_out_auto_skips_link_with_batch_in_flight() {
        let (registry, _sink) = test_registry().await;
        let store = &registry.inner.store;
        store.update_auto_push_settings(true, false).unwrap();
        store.upsert_paired_device(&hex32(1), "MBP", None, &[]).unwrap();
        store.set_auto_sync_mode(&hex32(1), AutoSyncMode::All).unwrap();
        let mut rx = fake_connected_link(&registry, &hex32(1));

        // 模拟整组发送中段：BatchStart 已入队、BatchEnd 未入队
        registry
            .inner
            .links
            .lock()
            .unwrap()
            .get_mut(&hex32(1))
            .unwrap()
            .batch_busy = true;
        let clip = text_clip("text", "hash-busy", "during batch");
        registry.fan_out_auto(&clip).await.unwrap();
        assert!(rx.try_recv().is_err(), "批量进行中的链路不得收到 auto 推送");
        assert_eq!(
            registry.inner.auto_dropped.load(Ordering::Relaxed),
            1,
            "忙碌跳过应计入 auto_dropped"
        );

        // 批量结束（标记清除）：恢复投递
        registry
            .inner
            .links
            .lock()
            .unwrap()
            .get_mut(&hex32(1))
            .unwrap()
            .batch_busy = false;
        registry.fan_out_auto(&clip).await.unwrap();
        assert!(
            matches!(rx.try_recv(), Ok(ControlMsg::SendClip { auto, .. }) if auto),
            "标记清除后应恢复 auto 投递"
        );
        assert_eq!(
            registry.inner.auto_dropped.load(Ordering::Relaxed),
            1,
            "恢复投递不再计数"
        );
    }

    /// fan_out_auto 投递：在线目标按每设备偏好过滤；SendClip 携带 auto=true +
    /// origin（本端 node_id）；超限文本跳过扇出但不是错误；payload 构建失败是
    /// 唯一 Err 来源。
    #[tokio::test]
    async fn fan_out_auto_dispatches_with_per_device_filtering() {
        let (registry, _sink) = test_registry().await;
        let store = &registry.inner.store;
        store.update_auto_push_settings(true, false).unwrap();
        store.upsert_paired_device(&hex32(1), "MBP", None, &[]).unwrap();
        store.set_auto_sync_mode(&hex32(1), AutoSyncMode::All).unwrap();
        store.upsert_paired_device(&hex32(2), "PC", None, &[]).unwrap();
        store.set_auto_sync_mode(&hex32(2), AutoSyncMode::Off).unwrap();
        store.upsert_paired_device(&hex32(3), "Phone", None, &[]).unwrap(); // 默认 TextOnly
        let mut rx1 = fake_connected_link(&registry, &hex32(1));
        let mut rx2 = fake_connected_link(&registry, &hex32(2));
        let mut rx3 = fake_connected_link(&registry, &hex32(3));
        let mut rx4 = fake_connected_link(&registry, &hex32(9)); // store 无行：按 TextOnly 兜底

        let clip = text_clip("text", "hash-t1", "hello auto");
        registry.fan_out_auto(&clip).await.unwrap();

        let my_id = registry.inner_endpoint_id_hex_for_test();
        for (label, rx) in [("all", &mut rx1), ("text-only", &mut rx3), ("unknown", &mut rx4)] {
            match rx.try_recv() {
                Ok(ControlMsg::SendClip {
                    clip_type, payload, auto, origin_node_id, ..
                }) => {
                    assert_eq!(clip_type, "text");
                    assert_eq!(payload, b"hello auto");
                    assert!(auto, "{label}：应为 auto 推送");
                    assert_eq!(
                        origin_node_id.as_deref(),
                        Some(my_id.as_str()),
                        "{label}：origin 应为本端 node_id"
                    );
                }
                other => panic!("{label} 设备应收到 auto 推送：{other:?}"),
            }
        }
        assert!(rx2.try_recv().is_err(), "Off 偏好不得收到");

        // 图片条目：只有 All 设备收到；TextOnly / 未知（TextOnly 兜底）被过滤。
        // 用真实临时图片文件，避免 payload 构建失败干扰断言。
        let dir = std::env::temp_dir().join(format!("ipaste-fanout-{}", crate::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("img.png");
        std::fs::write(&png_path, [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        let image_clip = text_clip("image", "hash-img", png_path.to_str().unwrap());
        registry.fan_out_auto(&image_clip).await.unwrap();
        match rx1.try_recv() {
            Ok(ControlMsg::SendClip { payload, auto, .. }) => {
                assert!(auto, "图片 auto 推送");
                let text = String::from_utf8(payload).unwrap();
                assert!(
                    text.starts_with("data:image/png;base64,"),
                    "图片 payload 应为 data url：{text}"
                );
            }
            other => panic!("All 设备应收到图片 auto 推送：{other:?}"),
        }
        assert!(rx3.try_recv().is_err(), "TextOnly 不收图片");
        assert!(rx4.try_recv().is_err(), "未知设备（TextOnly 兜底）不收图片");
        std::fs::remove_dir_all(&dir).ok();

        // 超限文本（> LAN_MAX_PAYLOAD）：跳过整次扇出且返回 Ok
        let big = text_clip("text", "hash-big", &"x".repeat(LAN_MAX_PAYLOAD + 1));
        registry
            .fan_out_auto(&big)
            .await
            .expect("超限跳过不是错误");
        assert!(rx1.try_recv().is_err(), "超限：无投递");

        // payload 构建失败（图片文件缺失）是唯一 Err 来源
        let missing = text_clip("image", "hash-miss", "/definitely/not/here/y.png");
        assert!(
            registry.fan_out_auto(&missing).await.is_err(),
            "payload 构建失败应返回 Err"
        );
    }
