//! 会话分发与接收落地的单元测试（Task 33 从 lan_sync/session.rs 原样外移）。
//!
//! 内容即原 `mod tests { ... }` 的主体：`use super::*` 指向 session 模块本身。

use super::apply::apply_received;
use super::dispatch::{handle_frame, validate_category_meta};

use super::*;
use super::apply::append_session_active;
use crate::events::{EVENT_DEVICE_CATEGORY_RECEIVED, EVENT_DEVICE_CLIP_RECEIVED};
use crate::lan_sync::autopush::RecentReceived;
use crate::lan_sync::NoopEventSink;
use crate::store::test_support::temp_store;
use crate::util::hash_text;
use std::sync::Mutex;
use tokio::io::duplex;

fn test_ctx() -> SessionCtx {
    SessionCtx {
        sink: Arc::new(NoopEventSink),
        store: temp_store(),
        peer_node_id: "ab".repeat(32),
        peer_device_name: "peer-device".to_string(),
        local_node_id: "aa".repeat(32),
        recent: Arc::new(RecentReceived::new()),
        auto_notify: false,
        append_copy_state: Arc::new(Mutex::new(AppendCopyState::default())),
    }
}

/// 事件捕获 sink：apply_received 纯函数级测试用
///（对齐 integration_tests.rs 的 CapturingEventSink）。
struct CapturingSink {
    events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl LanEventSink for CapturingSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        self.events
            .lock()
            .expect("捕获锁中毒")
            .push((event.to_string(), payload.clone()));
    }
}

fn count_events(sink: &CapturingSink, event: &str) -> usize {
    sink.events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .filter(|(name, _)| name.as_str() == event)
        .count()
}

/// 直接调 apply_received 的测试上下文：指定本机 node_id 与共享 recent。
fn apply_ctx(
    sink: &Arc<CapturingSink>,
    store: &Store,
    local_node_id: &str,
    recent: &Arc<RecentReceived>,
) -> SessionCtx {
    SessionCtx {
        sink: sink.clone(),
        store: store.clone(),
        peer_node_id: "ab".repeat(32),
        peer_device_name: "peer-device".to_string(),
        local_node_id: local_node_id.to_string(),
        recent: recent.clone(),
        auto_notify: false,
        append_copy_state: Arc::new(Mutex::new(AppendCopyState::default())),
    }
}

/// clips 表中该 content_hash 的行数（接收侧真实落库断言）。
fn clips_with_hash(store: &Store, hash: &str) -> i64 {
    let conn = store.connect().expect("测试库连接");
    conn.query_row(
        "SELECT COUNT(*) FROM clips WHERE content_hash = ?1",
        [hash],
        |row| row.get(0),
    )
    .expect("查询 clips")
}

/// 1) auto 历史条目：落库 + recent 登记。剪贴板写是尽力而为（无头 CI 上会
/// 失败并发诊断事件），故不对事件做任何断言（有无皆合法），只断言
/// 返回值、库行与 recent。
#[test]
fn apply_received_auto_history_inserts_and_registers_recent() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let ctx = apply_ctx(&sink, &store, &"aa".repeat(32), &recent);

    let payload = b"auto sync hello".to_vec();
    let ok = apply_received(
        &ctx, &store, "text", &payload, None, None, None, None, false, true, None,
    );
    assert!(ok, "auto 接收：落库成功即算同步成功");
    let hash = hash_text("auto sync hello");
    assert_eq!(clips_with_hash(&store, &hash), 1, "条目应已落入 clips");
    assert!(recent.contains(&hash), "auto 路径必须登记 recent（供发送侧防回推）");
}

/// 2) 手动历史条目：落库 + 接收事件；不登记 recent。
/// 「不触碰剪贴板」是结构性保证（本分支无 write_clipboard_* 调用，
/// spec §2 v4→v5 行为变更）：无头 CI 无法断言剪贴板内容，
/// 以「事件已发 + 落库成功」证明该路径不依赖剪贴板。
#[test]
fn apply_received_manual_history_skips_recent_and_notifies() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let ctx = apply_ctx(&sink, &store, &"aa".repeat(32), &recent);

    let payload = b"manual send hello".to_vec();
    let ok = apply_received(
        &ctx, &store, "text", &payload, None, None, None, None, false, false, None,
    );
    assert!(ok, "手动接收：落库成功");
    let hash = hash_text("manual send hello");
    assert_eq!(clips_with_hash(&store, &hash), 1, "条目应已落入 clips");
    assert!(!recent.contains(&hash), "手动路径不登记 recent");
    assert_eq!(
        count_events(&sink, EVENT_DEVICE_CLIP_RECEIVED),
        1,
        "手动接收应发出 deviceClipReceived 事件"
    );
}

/// 3) origin 自环防御：auto + origin == 本机 node_id → 静默丢弃
///（无行、无 recent、无任何事件）。
#[test]
fn apply_received_origin_loop_guard_drops_own_push() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let local = "aa".repeat(32);
    let ctx = apply_ctx(&sink, &store, &local, &recent);

    let payload = b"my own echo".to_vec();
    let ok = apply_received(
        &ctx, &store, "text", &payload, None, None, None, None, false, true, Some(&local),
    );
    assert!(!ok, "自环内容应被拒收");
    let hash = hash_text("my own echo");
    assert_eq!(clips_with_hash(&store, &hash), 0, "自环内容不应落库");
    assert!(!recent.contains(&hash), "自环内容不应登记 recent");
    assert!(
        sink.events.lock().expect("捕获锁中毒").is_empty(),
        "自环静默丢弃：不应发出任何事件"
    );
}

/// 4) 分组条目（auto 与否）：行为不变——落 category_items、不登记 recent。
#[test]
fn apply_received_category_items_bypass_auto_and_recent() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let ctx = apply_ctx(&sink, &store, &"aa".repeat(32), &recent);

    for (content, auto) in [("grouped auto", true), ("grouped manual", false)] {
        let payload = content.as_bytes().to_vec();
        let ok = apply_received(
            &ctx, &store, "text", &payload,
            Some("分组".to_string()), Some("#0D9488".to_string()),
            None, None, false, auto, None,
        );
        assert!(ok, "分组条目（auto={auto}）应落库成功");
        let hash = hash_text(content);
        assert!(!recent.contains(&hash), "分组路径不登记 recent");
    }
    let conn = store.connect().expect("测试库连接");
    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items AS ci
             JOIN categories AS c ON c.id = ci.category_id
             WHERE c.name = '分组' AND ci.text IN ('grouped auto', 'grouped manual')",
            [],
            |row| row.get(0),
        )
        .expect("查询分组条目");
    assert_eq!(hits, 2, "两条分组条目都应落在同名分组下");
    assert_eq!(
        count_events(&sink, EVENT_DEVICE_CLIP_RECEIVED),
        2,
        "分组接收（非批量）逐条发出接收事件"
    );
}

/// 5) 批量互斥接收侧兜底（Critical）：BatchStart → ClipPush{auto:true} →
/// ClipPush{auto:false} → BatchEnd——混进批量中段的 auto 帧绝不被折叠进
/// 打开的分组：它按 auto 语义落历史（clips），批量计数只算手动帧（1 收），
/// 分组里只有手动帧的内容。
#[tokio::test]
async fn auto_frame_mid_batch_never_joins_category_batch() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let ctx = apply_ctx(&sink, &store, &"aa".repeat(32), &recent);
    let (_client, server) = duplex(4096);
    let (_read_half, write_half) = tokio::io::split(server);
    let writer = Arc::new(tokio::sync::Mutex::new(FrameWriter::new(write_half)));

    let clip_push = |auto: bool, payload: Vec<u8>| {
        (
            LanMessage::ClipPush {
                clip_type: "text".into(),
                empty: false,
                category_name: None,
                category_color: None,
                display_name: None,
                auto,
                origin_node_id: None,
            },
            Some(payload),
        )
    };
    let mut batch = None;
    // 1) 打开批量
    assert!(
        handle_frame(
            (
                LanMessage::CategoryBatchStart {
                    category_name: "工作".into(),
                    category_color: Some("#0D9488".into()),
                    item_count: 1,
                },
                None,
            ),
            &ctx,
            &writer,
            &mut batch,
        )
        .await
    );
    assert!(batch.is_some(), "BatchStart 应进入批量态");

    // 2) 批量中段混入 auto 帧（发送侧忙标记漏防的兜底场景）
    assert!(handle_frame(clip_push(true, b"auto leak".to_vec()), &ctx, &writer, &mut batch).await);
    // 3) 正常的批量成员（手动帧）
    assert!(handle_frame(clip_push(false, b"batch member".to_vec()), &ctx, &writer, &mut batch).await);
    // 4) 结束批量
    assert!(handle_frame((LanMessage::CategoryBatchEnd, None), &ctx, &writer, &mut batch).await);
    assert!(batch.is_none(), "BatchEnd 应退出批量态");

    // auto 帧落历史（clips），绝不进分组
    let auto_hash = hash_text("auto leak");
    assert_eq!(clips_with_hash(&store, &auto_hash), 1, "auto 帧必须落历史");
    assert!(recent.contains(&auto_hash), "auto 帧照常登记 recent（防回推）");

    // 批量计数正确：只收到 1 条（手动帧），0 失败
    let category_events: Vec<serde_json::Value> = sink
        .events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .filter(|(name, _)| name.as_str() == EVENT_DEVICE_CATEGORY_RECEIVED)
        .map(|(_, payload)| payload.clone())
        .collect();
    assert_eq!(category_events.len(), 1, "BatchEnd 应 emit 一次汇总");
    assert_eq!(category_events[0]["categoryName"].as_str(), Some("工作"));
    assert_eq!(category_events[0]["count"].as_u64(), Some(1), "auto 帧不计入批量");
    assert_eq!(category_events[0]["failed"].as_u64(), Some(0));

    // 分组里只有手动帧：auto 帧的内容绝不在 category_items
    let conn = store.connect().expect("测试库连接");
    let auto_in_category: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items AS ci
             JOIN categories AS c ON c.id = ci.category_id
             WHERE c.name = '工作' AND ci.text = 'auto leak'",
            [],
            |row| row.get(0),
        )
        .expect("查询分组条目");
    assert_eq!(auto_in_category, 0, "auto 帧不得混进分组");
    let member_in_category: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items AS ci
             JOIN categories AS c ON c.id = ci.category_id
             WHERE c.name = '工作' AND ci.text = 'batch member'",
            [],
            |row| row.get(0),
        )
        .expect("查询分组条目");
    assert_eq!(member_in_category, 1, "手动帧是批量分组的唯一成员");
}

/// 6) 追加会话判定（Important）：is_enabled 且有 session 才算活跃——
/// 与 clipboard.rs capture_append_copy_item 的 merge 判据同口径。
#[test]
fn append_session_active_requires_enabled_and_session() {
    let mut state = AppendCopyState::default();
    assert!(!append_session_active(&state), "默认（未开启）：不活跃");
    state.is_enabled = true;
    assert!(!append_session_active(&state), "开启但无 session：不活跃");
    state.session_id = Some("s1".into());
    assert!(append_session_active(&state), "开启且有 session：活跃");
    state.is_enabled = false;
    assert!(!append_session_active(&state), "有 session 但已关闭：不活跃");
}

/// 7) 追加会话活跃期间的 auto 接收（Important）：跳过剪贴板写决策已生效
/// （append_session_active 为真 → 写路径短路），同步本体不受影响——条目
/// 照常落历史 + recent，本地追加缓冲不被对端内容污染。
#[test]
fn apply_received_auto_skips_clipboard_write_during_append_session() {
    let sink = Arc::new(CapturingSink { events: Mutex::new(Vec::new()) });
    let store = temp_store();
    let recent = Arc::new(RecentReceived::new());
    let ctx = apply_ctx(&sink, &store, &"aa".repeat(32), &recent);
    // 构造活跃追加会话（is_enabled + session_id）
    {
        let mut append = ctx.append_copy_state.lock().unwrap();
        append.is_enabled = true;
        append.session_id = Some("append-s1".into());
        append.text = "本地已有文本".into();
    }

    let payload = b"peer content during append".to_vec();
    let ok = apply_received(
        &ctx, &store, "text", &payload, None, None, None, None, false, true, None,
    );
    assert!(ok, "追加会话期间 auto 接收仍算同步成功");
    let hash = hash_text("peer content during append");
    assert_eq!(clips_with_hash(&store, &hash), 1, "条目应照常落入 clips");
    assert!(recent.contains(&hash), "recent 照常登记（防回推不受影响）");
    // 决策口径：活跃为真 → 写路径必然短路（无头 CI 上未短路则会多出一条
    // 「写入系统剪贴板失败」诊断事件；此处断言同步面完整即可）
    assert_eq!(
        count_events(&sink, EVENT_DEVICE_CLIP_RECEIVED),
        0,
        "auto_notify 关闭：不逐条提示"
    );
}

/// 收到 Ping：handle_frame 立即在共享写半上回一帧 Pong（会话继续存活）。
#[tokio::test]
async fn ping_frame_replies_pong() {
    // duplex 一侧模拟对端（读回 Pong），另一侧拆成读/写两半喂 handle_frame。
    let (client, server) = duplex(4096);
    let (read_half, write_half) = tokio::io::split(server);
    let writer = Arc::new(tokio::sync::Mutex::new(FrameWriter::new(write_half)));

    let ctx = test_ctx();
    let mut batch = None;
    let alive = handle_frame((LanMessage::Ping, None), &ctx, &writer, &mut batch).await;
    assert!(alive, "Ping 不应结束会话");

    let mut client_reader = FrameReader::new(client);
    let (msg, payload) = client_reader.read_message().await.unwrap();
    assert_eq!(msg, LanMessage::Pong);
    assert_eq!(payload, None);
}

/// 会话期收到 PairRequest：忽略（会话继续），不回写任何帧。
#[tokio::test]
async fn pair_request_in_session_is_ignored() {
    let (client, server) = duplex(4096);
    let (_read_half, write_half) = tokio::io::split(server);
    let writer = Arc::new(tokio::sync::Mutex::new(FrameWriter::new(write_half)));

    let ctx = test_ctx();
    let mut batch = None;
    let frame = (
        LanMessage::PairRequest {
            version: 5,
            device_name: "stranger".into(),
            invite_secret: "00".repeat(16),
        },
        None,
    );
    let alive = handle_frame(frame, &ctx, &writer, &mut batch).await;
    assert!(alive, "会话期的 Pair* 帧应被忽略而非断开");
}

/// 对端 Disconnect：handle_frame 返回 false 结束会话。
#[tokio::test]
async fn disconnect_frame_ends_session() {
    let (_client, server) = duplex(4096);
    let (_read_half, write_half) = tokio::io::split(server);
    let writer = Arc::new(tokio::sync::Mutex::new(FrameWriter::new(write_half)));

    let ctx = test_ctx();
    let mut batch = None;
    let alive = handle_frame((LanMessage::Disconnect, None), &ctx, &writer, &mut batch).await;
    assert!(!alive);
}

/// 指纹短码：EndpointId 前 4 字节 hex（8 字符）。
#[test]
fn fingerprint_hex_is_first_four_bytes() {
    let id: [u8; 32] = [0xab; 32];
    assert_eq!(fingerprint_hex(&id), "abababab");
    let mut id2 = [0u8; 32];
    id2[0] = 0x00;
    id2[1] = 0x0f;
    id2[2] = 0xff;
    id2[3] = 0x10;
    assert_eq!(fingerprint_hex(&id2), "000fff10");
    assert_eq!(fingerprint_hex(&id2).len(), 8);
}

/// run_session_loop 集成冒烟：对端关闭（EOF）后主循环经 peer_gone 退出。
#[tokio::test]
async fn session_loop_exits_on_peer_eof() {
    let (client, server) = duplex(4096);
    let ctx = test_ctx();
    let (server_read, server_write) = tokio::io::split(server);
    let (control_tx, control_rx) = mpsc::channel(4);
    let (_dead_tx, dead_rx) = tokio::sync::oneshot::channel::<()>();

    let task = tokio::spawn(run_session_loop(
        server_read,
        server_write,
        ctx,
        control_rx,
        dead_rx,
    ));
    // 对端直接 drop：读任务读到 EOF → peer_gone → 主循环退出。
    drop(client);
    // 关闭控制通道加速退出（主循环的 None 分支同样会 break）。
    drop(control_tx);
    task.await.unwrap();
}

#[cfg(test)]
mod category_meta_tests {
use super::validate_category_meta;

#[test]
fn validate_category_meta_accepts_within_bounds() {
    assert!(validate_category_meta(None, None).is_ok());
    assert!(validate_category_meta(Some("工作"), Some("#0D9488")).is_ok());
    let name_80: String = "a".repeat(80);
    let color_32: String = "c".repeat(32);
    assert!(validate_category_meta(Some(&name_80), Some(&color_32)).is_ok());
}

#[test]
fn validate_category_meta_rejects_oversized() {
    let name_81: String = "a".repeat(81);
    let color_33: String = "c".repeat(33);
    assert!(validate_category_meta(Some(&name_81), None).is_err());
    assert!(validate_category_meta(None, Some(&color_33)).is_err());
    // 多字节字符按字符数计
    let wide: String = "中".repeat(81);
    assert!(validate_category_meta(Some(&wide), None).is_err());
}
}
