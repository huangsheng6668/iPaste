//! 跨设备同步 v5 集成测试：同进程双 endpoint（QUIC 回环、RelayMode::Disabled）。
//!
//! 覆盖 Task 7 `DeviceLinkRegistry` 的全链路语义：票据配对（pair-request →
//! respond_pair(true) → 双向信任落库 → 双端 Connected）、发送与接收侧真实落库、
//! 票据一次性、连接稳定性（配对后不震荡）与撤销断链。全程真实 QUIC 回环
//! （票据携带本机直连地址），不依赖外网与中继。
//!
//! 事件断言一律使用 `crate::events` 常量（仓库规则：`ipaste://` 字面量只在
//! events.rs 出现）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::events::{
    EVENT_DEVICE_CATEGORY_RECEIVED, EVENT_DEVICE_CLIP_RECEIVED, EVENT_DEVICE_STATUS_CHANGED,
    EVENT_PAIR_JOIN_FAILED, EVENT_PAIR_REQUEST,
};
use crate::lan_sync::registry::DeviceLinkRegistry;
use crate::lan_sync::LanEventSink;
use crate::models::{AutoSyncMode, ClipItem, DeviceOnline};
use crate::store::test_support::temp_store;
use crate::store::Store;
use crate::util::{hash_bytes, hash_text, new_id, now, preview};

/// 每步等待窗口（brief 约定 10s 轮询上限；本地回环实际毫秒级完成）。
const WAIT: Duration = Duration::from_secs(10);

/// 测试事件出口：把 (event, payload) 记入共享 Vec，供集成测试断言 emit 内容
/// （自 v4 integration_tests.rs 的 CapturingEventSink 迁移）。
pub(crate) struct CapturingEventSink {
    pub(crate) events: Mutex<Vec<(String, Value)>>,
}

impl LanEventSink for CapturingEventSink {
    fn emit(&self, event: &str, payload: &Value) {
        self.events
            .lock()
            .expect("捕获锁中毒")
            .push((event.to_string(), payload.clone()));
    }
}

fn sink() -> Arc<CapturingEventSink> {
    Arc::new(CapturingEventSink { events: Mutex::new(Vec::new()) })
}

/// 某事件已 emit 的次数。
fn count_events(target: &CapturingEventSink, event: &str) -> usize {
    target
        .events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .filter(|(name, _)| name.as_str() == event)
        .count()
}

/// 某事件的首个 payload（断言字段用）。
fn first_payload(target: &CapturingEventSink, event: &str) -> Option<serde_json::Value> {
    target
        .events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .find(|(name, _)| name.as_str() == event)
        .map(|(_, payload)| payload.clone())
}

/// 全部事件名（失败诊断用）。
fn event_names(target: &CapturingEventSink) -> Vec<String> {
    target
        .events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

async fn wait_until(timeout: Duration, cond: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// A 侧针对某设备的状态变化事件数（震荡断言用）。
fn status_events_for(target: &CapturingEventSink, node_id: &str) -> usize {
    target
        .events
        .lock()
        .expect("捕获锁中毒")
        .iter()
        .filter(|(name, payload)| {
            name.as_str() == EVENT_DEVICE_STATUS_CHANGED
                && payload.get("nodeId").and_then(Value::as_str) == Some(node_id)
        })
        .count()
}

/// 建立真实配对链路：A 生成票据 → B 后台 join（与命令层同构的 spawn 形态）→
/// A 收到 pair-request 后同意 → 双方各出现一条 Connected 设备。
/// 返回 (票据, A 眼中 B 的 node_id, B 眼中 A 的 node_id)。
async fn pair_over_loopback(
    a: &Arc<DeviceLinkRegistry>,
    sink_a: &Arc<CapturingEventSink>,
    b: &Arc<DeviceLinkRegistry>,
    sink_b: &Arc<CapturingEventSink>,
) -> (String, String, String) {
    let ticket = a.create_invite().await.expect("生成邀请票据");
    assert!(ticket.starts_with("ipaste-pair-v1:"), "票据前缀：{ticket}");

    let joiner = b.clone();
    let join_ticket = ticket.clone();
    tokio::spawn(async move {
        if let Err(reason) = joiner.join(&join_ticket).await {
            eprintln!("[integration] join 失败：{reason}");
        }
    });

    assert!(
        wait_until(WAIT, || count_events(sink_a, EVENT_PAIR_REQUEST) >= 1).await,
        "A 应收到配对请求，实际事件：{:?}",
        event_names(sink_a)
    );
    a.respond_pair(true).expect("同意配对");

    assert!(
        wait_until(WAIT, || {
            let a_infos = a.device_infos();
            let b_infos = b.device_infos();
            a_infos.len() == 1
                && a_infos[0].online == DeviceOnline::Connected
                && b_infos.len() == 1
                && b_infos[0].online == DeviceOnline::Connected
        })
        .await,
        "配对成功后双方各有一条在线设备；A 事件：{:?}，B 事件：{:?}",
        event_names(sink_a),
        event_names(sink_b)
    );
    let node_b_on_a = a.device_infos()[0].device.node_id.clone();
    let node_a_on_b = b.device_infos()[0].device.node_id.clone();
    // 信任表里的对端标识必须是真实端点身份（EndpointId hex），而非票据/会话临时值
    assert_eq!(node_b_on_a, b.inner_endpoint_id_hex_for_test());
    assert_eq!(node_a_on_b, a.inner_endpoint_id_hex_for_test());
    (ticket, node_b_on_a, node_a_on_b)
}

/// 全链路：配对 → 发送 → 票据重用拒绝 → 撤销断链 + 静默拒绝重拨。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_send_disconnect_revoke_full_chain() {
    let store_a = temp_store();
    let store_b = temp_store();
    let sink_a = sink();
    let a = DeviceLinkRegistry::start_for_test(store_a.clone(), sink_a.clone())
        .await
        .expect("A 端点");
    let sink_b = sink();
    let b = DeviceLinkRegistry::start_for_test(store_b.clone(), sink_b.clone())
        .await
        .expect("B 端点");

    // 1-4. 配对：双向信任落库 + 双端 Connected（含 node_id == 对端端点身份断言）
    let (ticket, node_b, _node_a) = pair_over_loopback(&a, &sink_a, &b, &sink_b).await;

    // 5. A → B 发送带分组的文本：B 侧按名称建分组并把条目落 category_items。
    // 刻意走分组路径而非历史（clips）路径：分组路径不触碰系统剪贴板，
    // 任何环境（含 headless Linux CI）确定可测；历史路径（auto 分流后）
    // 会尽力写系统剪贴板，无显示服务器时发诊断事件，事件断言不再确定。
    const CHAIN_CATEGORY: &str = "全链路分组";
    a.send_raw(
        None,
        "text",
        b"integration hello",
        Some(CHAIN_CATEGORY),
        Some("#0D9488"),
        None,
    )
    .await
    .expect("发送文本");
    assert!(
        wait_until(WAIT, || count_events(&sink_b, EVENT_DEVICE_CLIP_RECEIVED) >= 1).await,
        "B 应收到条目事件；B 事件：{:?}",
        event_names(&sink_b)
    );
    let clip_payload = first_payload(&sink_b, EVENT_DEVICE_CLIP_RECEIVED).expect("payload");
    assert_eq!(clip_payload["clipType"].as_str(), Some("text"));
    assert_eq!(
        clip_payload["categoryName"].as_str(),
        Some(CHAIN_CATEGORY),
        "带分组发送：接收事件应携带分组名"
    );
    // 接收侧真实落库（而非仅事件）：同名分组下恰好 1 条原文条目
    let conn = store_b.connect().expect("B 库连接");
    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items AS ci
             JOIN categories AS c ON c.id = ci.category_id
             WHERE c.name = ?1 AND ci.text = 'integration hello'",
            [CHAIN_CATEGORY],
            |row| row.get(0),
        )
        .expect("查询 B 的分组条目");
    assert_eq!(hits, 1, "B 的同名分组应恰好包含收到的文本条目");

    // 6. 票据一次性：同一票据第二次 join 失败，绝不再走一次完整配对。
    // 实测收敛行为：A 已信任 B，第二次拨入被按「已配对入站会话」处理并收编
    // 旧会话；旧会话关闭的 Disconnect/EOF 帧落在第二次 join 等待应答的配对流上
    // → join 返回 Err 且 emit EVENT_PAIR_JOIN_FAILED（错误/事件双通道同现）。
    let pair_requests_before = count_events(&sink_a, EVENT_PAIR_REQUEST);
    let reused = b.clone();
    let reuse_ticket = ticket.clone();
    let second_join = tokio::spawn(async move { reused.join(&reuse_ticket).await });
    assert!(
        wait_until(WAIT, || count_events(&sink_b, EVENT_PAIR_JOIN_FAILED) >= 1).await,
        "票据重用应失败：B 应 emit join-failed；B 事件：{:?}",
        event_names(&sink_b)
    );
    let outcome = tokio::time::timeout(WAIT, second_join).await;
    match outcome {
        Ok(Ok(result)) => assert!(result.is_err(), "同一票据第二次 join 应返回 Err，实际 Ok"),
        Ok(Err(join_err)) => panic!("join 任务异常：{join_err}"),
        Err(_) => panic!("同一票据第二次 join 在窗口内未返回（挂起）"),
    }
    // 重用没有触发第二次配对确认（票据一次性在链路层面成立）
    assert_eq!(
        count_events(&sink_a, EVENT_PAIR_REQUEST),
        pair_requests_before,
        "票据重用不得再次触发配对请求"
    );

    // 7. A 撤销 B：A 侧链路断开；撤销行阻断信任——B 之后的重拨被静默拒绝
    //（不再产生 pair-request 事件）。
    // 重用失败带来的会话收编/重拨扰动在此之前已收敛，等链路回到稳定 Connected。
    assert!(
        wait_until(WAIT, || {
            a.device_infos()
                .first()
                .is_some_and(|info| info.online == DeviceOnline::Connected)
        })
        .await,
        "撤销前链路应已回到 Connected；A 事件：{:?}",
        event_names(&sink_a)
    );
    a.revoke(&node_b);
    assert!(
        wait_until(WAIT, || {
            a.device_infos()
                .first()
                .is_some_and(|info| info.online != DeviceOnline::Connected)
        })
        .await,
        "撤销后 A 侧链路断开"
    );
    // 撤销行恒 Offline（即使有残留登记）
    assert!(a.device_infos()[0].device.revoked_at.is_some());
    assert_eq!(a.device_infos()[0].online, DeviceOnline::Offline);
    // B 的重拨静默拒绝：覆盖至少一个 5s 重拨退避周期，A 不再收到任何配对请求
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(
        count_events(&sink_a, EVENT_PAIR_REQUEST),
        pair_requests_before,
        "撤销后 B 的重拨不得触发配对请求（静默拒绝）"
    );
}

/// 配对建立后连接必须稳定：6s 采样窗口内 B 在 A 侧恒为 Connected，且无任何
/// Connected→Offline→Connected 的状态抖动事件（Task 7 振荡修复的回归）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connection_stays_connected_without_oscillation_after_pairing() {
    let store_a = temp_store();
    let store_b = temp_store();
    let sink_a = sink();
    let a = DeviceLinkRegistry::start_for_test(store_a, sink_a.clone())
        .await
        .expect("A 端点");
    let sink_b = sink();
    let b = DeviceLinkRegistry::start_for_test(store_b, sink_b.clone())
        .await
        .expect("B 端点");

    let (_ticket, node_b, _) = pair_over_loopback(&a, &sink_a, &b, &sink_b).await;

    // 采样 6s（500ms × 12）：device_infos 恒 Connected + 无新增状态事件
    let status_before = status_events_for(&sink_a, &node_b);
    let mut samples: Vec<Option<DeviceOnline>> = Vec::new();
    for _ in 0..12 {
        samples.push(
            a.device_infos()
                .into_iter()
                .find(|info| info.device.node_id == node_b)
                .map(|info| info.online),
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let status_after = status_events_for(&sink_a, &node_b);

    assert!(
        samples.iter().all(|sample| *sample == Some(DeviceOnline::Connected)),
        "6s 采样窗口内 B 应恒为 Connected，实测序列：{samples:?}"
    );
    assert_eq!(
        status_before, status_after,
        "稳定窗口内不应出现任何针对 B 的状态变化事件（连接抖动/振荡）"
    );
}

/// 整组发送：A 建分组 + 2 条目 → send_category 广播 → B 收到
/// EVENT_DEVICE_CATEGORY_RECEIVED 且分组与 2 个条目真实落库。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_category_delivers_batch_to_paired_device() {
    const CATEGORY_NAME: &str = "集成分组";

    let store_a = temp_store();
    let store_b = temp_store();
    let sink_a = sink();
    let a = DeviceLinkRegistry::start_for_test(store_a.clone(), sink_a.clone())
        .await
        .expect("A 端点");
    let sink_b = sink();
    let b = DeviceLinkRegistry::start_for_test(store_b.clone(), sink_b.clone())
        .await
        .expect("B 端点");

    pair_over_loopback(&a, &sink_a, &b, &sink_b).await;

    // A 的库：分组（按名称建）+ 2 条文本条目（display_name 覆盖其中一条）
    store_a
        .insert_received_category_item(
            "text".into(),
            "cat-hash-1".into(),
            "预览一".into(),
            "分类条目一".into(),
            CATEGORY_NAME.into(),
            Some("#0D9488".into()),
            None,
            None,
        )
        .expect("插入条目一");
    store_a
        .insert_received_category_item(
            "text".into(),
            "cat-hash-2".into(),
            "预览二".into(),
            "分类条目二".into(),
            CATEGORY_NAME.into(),
            Some("#0D9488".into()),
            Some("改名条目".into()),
            None,
        )
        .expect("插入条目二");
    let category_a = store_a
        .list_categories()
        .expect("A 分组列表")
        .into_iter()
        .find(|category| category.name == CATEGORY_NAME)
        .expect("A 分组存在");

    // 广播发送（target None = 全部 Connected 链路）
    let (name, sent, failed) = a
        .send_category(None, &category_a.id)
        .await
        .expect("整组发送");
    assert_eq!(name, CATEGORY_NAME);
    assert_eq!(sent, 2, "两条全部送达");
    assert_eq!(failed, 0);

    assert!(
        wait_until(WAIT, || count_events(&sink_b, EVENT_DEVICE_CATEGORY_RECEIVED) >= 1).await,
        "B 应收到整组接收事件；B 事件：{:?}",
        event_names(&sink_b)
    );
    let payload = first_payload(&sink_b, EVENT_DEVICE_CATEGORY_RECEIVED).expect("payload");
    assert_eq!(payload["categoryName"].as_str(), Some(CATEGORY_NAME));
    assert_eq!(payload["count"].as_u64(), Some(2), "两条均落库成功");
    assert_eq!(payload["failed"].as_u64(), Some(0));

    // B 的库：同名分组真实出现，且恰好 2 个条目（display_name 随帧送达）
    let category_b = store_b
        .list_categories()
        .expect("B 分组列表")
        .into_iter()
        .find(|category| category.name == CATEGORY_NAME)
        .expect("B 应按名称建立同名分组");
    assert_eq!(category_b.color, category_a.color, "新建分组采用发送端颜色");
    let conn = store_b.connect().expect("B 库连接");
    let item_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items WHERE category_id = ?1",
            [&category_b.id],
            |row| row.get(0),
        )
        .expect("查询 B 的分组条目");
    assert_eq!(item_count, 2, "B 的分组下应恰好 2 条");
    let renamed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items WHERE category_id = ?1 AND display_name = '改名条目'",
            [&category_b.id],
            |row| row.get(0),
        )
        .expect("查询重命名条目");
    assert_eq!(renamed, 1, "display_name 应随整组发送送达");
}

/// 撤销后的设备持**有效**票据再次拨入（F2）：host 侧不得弹配对请求
///（无 pair-request 事件）、不回任何帧（拨号方只能等到自己的超时）——
/// 静默拒绝 doctrine（spec §3），弹窗 + 可区分拒绝会构成信任态预言机。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoked_device_with_valid_invite_is_silently_ignored() {
    let store_a = temp_store();
    let store_b = temp_store();
    let sink_a = sink();
    let a = DeviceLinkRegistry::start_for_test(store_a, sink_a.clone())
        .await
        .expect("A 端点");
    let sink_b = sink();
    let b = DeviceLinkRegistry::start_for_test(store_b, sink_b.clone())
        .await
        .expect("B 端点");

    // 真实配对后 A 撤销 B，再生成全新有效邀请
    let (_ticket, node_b, _node_a) = pair_over_loopback(&a, &sink_a, &b, &sink_b).await;
    a.revoke(&node_b);
    assert!(
        wait_until(WAIT, || {
            a.device_infos()
                .first()
                .is_some_and(|info| info.online != DeviceOnline::Connected)
        })
        .await,
        "撤销后 A 侧链路断开"
    );
    let fresh_ticket = a.create_invite().await.expect("撤销后可再生成邀请");

    // B 持有效票据 join：A 不得出现任何 pair-request 事件（撤销门静默拒绝）
    let pair_requests_before = count_events(&sink_a, EVENT_PAIR_REQUEST);
    let joiner = b.clone();
    let join_ticket = fresh_ticket.clone();
    let join_task = tokio::spawn(async move { joiner.join(&join_ticket).await });
    // 覆盖拨号 + 首帧送达窗口（回环毫秒级，3s 余量充足）
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        count_events(&sink_a, EVENT_PAIR_REQUEST),
        pair_requests_before,
        "撤销设备持有效票据拨入不得触发配对请求弹窗；A 事件：{:?}",
        event_names(&sink_a)
    );
    // B 侧只能等到 join 超时（30s），不提前等待；abort 收尾
    join_task.abort();
}

// —— 自动推送全链路（Spec 2 Task 6）——

/// 否定断言的固定等待窗：回环投递毫秒级完成，2s 已是百倍级余量；
/// 比 WAIT 短是为了不让「证明没有发生」拖慢 CI。
const NEGATIVE_WINDOW: Duration = Duration::from_secs(2);

/// 构造捕获路径形态的文本 ClipItem（fan_out_auto 的输入）。
/// content_hash 按捕获侧口径 = hash_text(原文)（B 的接收路径会按内容重算出
/// 同一哈希，见 captured_item_from_payload），其余字段取最小合法值。
fn text_clip(content: &str) -> ClipItem {
    ClipItem {
        id: new_id(),
        clip_type: "text".to_string(),
        content_hash: hash_text(content),
        display_name: None,
        preview_text: preview(content),
        text: content.to_string(),
        source_app: None,
        last_captured_at: now(),
        favorite_count: 0,
        is_pinned: false,
    }
}

/// 构造图片 ClipItem：text = 已落盘 png 的路径（捕获侧口径），content_hash =
/// hash_bytes(png)。接收端会按 data url 重解码并落自己的文件与哈希，
/// 断言只认 clip_type，不比对哈希。
fn image_clip(png_path: &str, content_hash: &str) -> ClipItem {
    ClipItem {
        id: new_id(),
        clip_type: "image".to_string(),
        content_hash: content_hash.to_string(),
        display_name: None,
        preview_text: "1 x 1".to_string(),
        text: png_path.to_string(),
        source_app: None,
        last_captured_at: now(),
        favorite_count: 0,
        is_pinned: false,
    }
}

/// 编码一张真实可解码的 1x1 png：接收路径的 image_from_bytes 要能解出来，
/// 裸签名字节过不了解码。
fn tiny_png_bytes() -> Vec<u8> {
    use image::ImageEncoder as _;

    let img = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_pixel(
        1,
        1,
        image::Rgba([0x10, 0x80, 0xF0, 0xFF]),
    );
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), 1, 1, image::ColorType::Rgba8.into())
        .expect("编码 1x1 png");
    png
}

/// 某库 clips 表中 text 恰为该原文的行数。
/// 断言口径刻意按 text 匹配而非 content_hash：auto 接收路径的哈希由
/// captured_item_from_payload 按**内容**重算（不信任发送端字段），图片还会
/// 重编码出新哈希——text/clip_type 才是两端一致的稳定观测面。
fn clip_rows_with_text(store: &Store, text: &str) -> i64 {
    let conn = store.connect().expect("库连接");
    conn.query_row("SELECT COUNT(*) FROM clips WHERE text = ?1", [text], |row| {
        row.get(0)
    })
    .expect("查询 clips 表")
}

/// 某库 clips 表中 image 条目的行数。
fn image_clip_rows(store: &Store) -> i64 {
    let conn = store.connect().expect("库连接");
    conn.query_row(
        "SELECT COUNT(*) FROM clips WHERE clip_type = 'image'",
        [],
        |row| row.get(0),
    )
    .expect("查询 clips 表 image 行")
}

/// 自动推送全链路（真实 QUIC 回环）：配对 → TextOnly 文本扇出送达落库并登记
/// recent → 环抑制（B 回扇出被自己的 recent 命中短路，A 永不收到）→ 偏好过滤
/// （Off 拒收文本 / All 收 image）→ 手动对照（落库但不登记 recent）。
///
/// 全程不触系统剪贴板：auto 接收路径的剪贴板写是尽力而为的诊断性副作用
/// （无头环境必失败、事件不计入同步结果），所有断言只看 clips 表行与 recent
/// 滑窗——headless CI 确定。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn autopush_full_chain() {
    let store_a = temp_store();
    let store_b = temp_store();
    let sink_a = sink();
    let a = DeviceLinkRegistry::start_for_test(store_a.clone(), sink_a.clone())
        .await
        .expect("A 端点");
    let sink_b = sink();
    let b = DeviceLinkRegistry::start_for_test(store_b.clone(), sink_b.clone())
        .await
        .expect("B 端点");

    // 1) 配对（复用 harness）
    let (_ticket, node_b, _node_a) = pair_over_loopback(&a, &sink_a, &b, &sink_b).await;

    // 2) 显式设一遍 TextOnly（缺省即 TextOnly，显式化测试意图）
    a.set_auto_sync(&node_b, AutoSyncMode::TextOnly);

    // 3-4) 文本捕获即扇出
    const AUTO_TEXT: &str = "auto-hello";
    let auto_clip = text_clip(AUTO_TEXT);
    a.fan_out_auto(&auto_clip).await.expect("A 扇出文本");

    // 5) B 的 clips 表出现该原文行（哈希由 B 按内容重算，断言按 text 匹配）
    assert!(
        wait_until(WAIT, || clip_rows_with_text(&store_b, AUTO_TEXT) == 1).await,
        "TextOnly 偏好下文本自动推送应落 B 的历史表"
    );

    // 6) auto 接收路径登记 recent（供发送侧防回推）
    let auto_hash = hash_text(AUTO_TEXT);
    assert!(
        b.recent_contains(&auto_hash),
        "auto 路径必须登记 recent（B 侧防回推的前提）"
    );

    // 7) 环断言：B 对同一条目回扇出必须被自己的 recent 命中短路（never sent），
    //    A 的 clips 表在等待窗内不出现该原文。A 侧的捕获是 watcher 行为、
    //    不参与本测试——B 的 recent 短路 + A 无行共同佐证回环被抑制。
    b.fan_out_auto(&auto_clip).await.expect("B 回扇出（应静默短路）");
    assert!(
        b.recent_contains(&auto_hash),
        "短路前提：recent 命中在等待窗内仍有效（TTL 60s）"
    );
    tokio::time::sleep(NEGATIVE_WINDOW).await;
    assert_eq!(
        clip_rows_with_text(&store_a, AUTO_TEXT),
        0,
        "环断言：B 的 recent 命中应短路回扇出，A 不得收到回推内容"
    );

    // 8a) 偏好过滤 - Off：新哈希的文本扇出，B 无行
    a.set_auto_sync(&node_b, AutoSyncMode::Off);
    const OFF_TEXT: &str = "auto-off-never-arrives";
    let off_clip = text_clip(OFF_TEXT);
    a.fan_out_auto(&off_clip).await.expect("A 扇出（应被偏好过滤）");
    tokio::time::sleep(NEGATIVE_WINDOW).await;
    assert_eq!(
        clip_rows_with_text(&store_b, OFF_TEXT),
        0,
        "Off 偏好：B 不应收到任何类型的自动推送"
    );

    // 8b) 偏好过滤 - All：image 扇出（A 先落盘 1x1 png，clip.text=路径），B 收到
    //     并落自己的 image 行（B 侧重解码、存自己的文件路径——只认 clip_type）
    a.set_auto_sync(&node_b, AutoSyncMode::All);
    let png = tiny_png_bytes();
    let png_hash = hash_bytes(&png);
    let png_path = store_a.save_image_bytes(&png_hash, &png).expect("A 落盘图片");
    let clip_image = image_clip(&png_path, &png_hash);
    a.fan_out_auto(&clip_image).await.expect("A 扇出图片");
    assert!(
        wait_until(WAIT, || image_clip_rows(&store_b) == 1).await,
        "All 偏好：B 应收到 image 自动推送并落库"
    );

    // 9) 手动对照：send_raw 落库 + 接收事件，但不登记 recent——auto/手动两路径
    //    的可观测分界。手动路径成功恒 emit EVENT_DEVICE_CLIP_RECEIVED（非
    //    silent），与 DB 行一起等齐后再断言 recent。
    const MANUAL_TEXT: &str = "manual-contrast";
    a.send_raw(
        Some(&node_b),
        "text",
        MANUAL_TEXT.as_bytes(),
        None,
        None,
        None,
    )
    .await
    .expect("手动发送");
    assert!(
        wait_until(WAIT, || {
            count_events(&sink_b, EVENT_DEVICE_CLIP_RECEIVED) >= 1
                && clip_rows_with_text(&store_b, MANUAL_TEXT) == 1
        })
        .await,
        "手动发送应落 B 的历史表并 emit 接收事件；B 事件：{:?}",
        event_names(&sink_b)
    );
    assert!(
        !b.recent_contains(&hash_text(MANUAL_TEXT)),
        "手动路径不得登记 recent（否则会误伤后续同内容自动推送的回环判定）"
    );
}
