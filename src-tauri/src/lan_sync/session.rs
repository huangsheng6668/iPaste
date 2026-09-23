//! v5 明文会话层：泛型于 `AsyncRead`/`AsyncWrite` 的双工帧循环。
//!
//! 传输安全由 iroh QUIC TLS 承担（线格式即明文 JSON header + payload，见
//! frame.rs）；本文件只负责「读任务 + 控制主循环」的会话编排与入站帧处理。
//! 接收内容的落库决策（文本/链接/颜色/HTML/图片/文件的落库、落盘与写剪贴板
//! 编排）在子模块 `session/apply.rs`。

use std::sync::{Arc, Mutex};

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::clipboard::read_current_clipboard;
use crate::events::{
    DeviceCategoryReceived, DeviceClipReceiveFailed, EVENT_DEVICE_CATEGORY_RECEIVED,
    EVENT_DEVICE_CLIP_RECEIVE_FAILED,
};
use crate::lan_sync::autopush::RecentReceived;
use crate::lan_sync::frame::{FrameReader, FrameWriter};
use crate::lan_sync::protocol::{LAN_BATCH_MAX_ITEMS, LanMessage};
use crate::lan_sync::{ControlMsg, LanEventSink};
use crate::models::{AppendCopyState, ClipboardRead};
use crate::store::Store;

mod apply;

use self::apply::apply_received;

/// 单个会话的固定上下文：事件出口、落库句柄与对端标识。
/// DeviceLinkRegistry 在建立双向流后构造它并调用 `run_session_loop`。
pub(crate) struct SessionCtx {
    pub sink: Arc<dyn LanEventSink>,
    pub store: Store,
    /// 对端 EndpointId 的 hex（64 字符）：v5 事件的设备标识。
    pub peer_node_id: String,
    pub peer_device_name: String,
    /// 本机 EndpointId 的 hex（64 字符）：接收侧 origin 自环防御的比对基准
    ///（spec §3 第三道）。
    pub local_node_id: String,
    /// 最近接收哈希滑窗（共享 registry 级单例）：auto 路径登记，发送侧
    /// 扇出经同一实例防回推。
    pub recent: Arc<RecentReceived>,
    /// auto 接收的轻提示开关（settings.auto_push.notify）：仅控制 auto 成功后
    /// 是否 emit deviceClipReceived；诊断事件不受此开关影响。
    pub auto_notify: bool,
    /// 追加复制会话状态（与 clipboard watcher 共享同一实例）：活跃期间
    /// auto 接收跳过剪贴板写（见 append_session_active）。
    pub append_copy_state: Arc<Mutex<AppendCopyState>>,
}

/// 会话内统一事件出口：payload 序列化失败按 Null 发出（与 v4 行为一致）。
fn emit<E: serde::Serialize>(ctx: &SessionCtx, event: &str, payload: E) {
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    ctx.sink.emit(event, &value);
}

/// EndpointId 前 4 字节 hex —— UI 指纹短码（非安全锚点，仅展示核对用）。
/// registry 的配对流程消费；纯函数独立测试。
pub(crate) fn fingerprint_hex(endpoint_id: &[u8; 32]) -> String {
    endpoint_id.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// 接收侧解析/落库失败：emit 诊断事件 + 打印日志，避免静默丢弃。
fn emit_clip_receive_failed(ctx: &SessionCtx, reason: String) {
    eprintln!("[lan-sync] 接收条目失败：{reason}");
    emit(
        ctx,
        EVENT_DEVICE_CLIP_RECEIVE_FAILED,
        &DeviceClipReceiveFailed { node_id: ctx.peer_node_id.clone(), reason },
    );
}

/// 整组接收完成：emit 汇总事件（接收端据此刷新一次列表并提示）。
fn emit_category_received(ctx: &SessionCtx, category_name: String, count: u32, failed: u32) {
    emit(
        ctx,
        EVENT_DEVICE_CATEGORY_RECEIVED,
        &DeviceCategoryReceived { node_id: ctx.peer_node_id.clone(), category_name, count, failed },
    );
}

/// 接收侧整组传输的中间状态：`CategoryBatchStart` 与 `CategoryBatchEnd` 之间
/// 收到的条目静默落库，结束时统一 emit 汇总事件。
struct BatchState {
    category_name: String,
    category_color: Option<String>,
    /// 第一条新条目的 sort_order（= 现有最小 sort_order - 预计条目数），
    /// 之后每条 +1，保证新条目整体排在现有条目之上且保持发送顺序。
    base_order: i64,
    next_index: i64,
    received: u32,
    failed: u32,
}

/// 读当前剪贴板，返回 (clip_type, payload_bytes)；空返回 None
fn read_current_payload() -> Result<Option<(String, Vec<u8>)>, String> {
    match read_current_clipboard()? {
        ClipboardRead::Item(item) => {
            let clip_type = item.clip_type.clone();
            let text = if clip_type == "image" {
                // 图片存的是 png bytes，需要编码成 data url 以复用 captured_item_from_payload
                image_data_url(&item.image_bytes)?
            } else {
                item.text.clone()
            };
            Ok(Some((clip_type, text.into_bytes())))
        }
        _ => Ok(None),
    }
}

fn image_data_url(bytes: &Option<Vec<u8>>) -> Result<String, String> {
    let png = bytes.as_ref().ok_or_else(|| "无图片数据".to_string())?;
    let b64: String = base64::engine::general_purpose::STANDARD
        .encode(png)
        .chars()
        .collect();
    Ok(format!("data:image/png;base64,{}", b64))
}

/// 接收路径的分组字段上限：与本地 `clean_display_name` 的 80 字符口径对齐，
/// 防已配对对端灌超长字符串污染本地 DB 与 UI 事件流。
pub(crate) const MAX_CATEGORY_NAME_LEN: usize = 80;
pub(crate) const MAX_CATEGORY_COLOR_LEN: usize = 32;

/// 校验对端发来的分组元数据；Err(原因) 表示该帧应拒收（会话不断开）。
fn validate_category_meta(name: Option<&str>, color: Option<&str>) -> Result<(), String> {
    if let Some(n) = name {
        if n.chars().count() > MAX_CATEGORY_NAME_LEN {
            return Err(format!("分组名不能超过 {MAX_CATEGORY_NAME_LEN} 个字符"));
        }
    }
    if let Some(c) = color {
        if c.chars().count() > MAX_CATEGORY_COLOR_LEN {
            return Err(format!("分组颜色不能超过 {MAX_CATEGORY_COLOR_LEN} 个字符"));
        }
    }
    Ok(())
}

/// 处理一条入站帧。返回 `false` 表示该帧要求结束会话（对端 Disconnect / 回写失败）。
///
/// 由会话的**读任务**逐条调用（读到即原地处理）；`write_half` 是与主循环共享的
/// 写半（仅回 `ClipResponse`/`Pong` 时短暂加锁）。
async fn handle_frame<W>(
    frame: (LanMessage, Option<Vec<u8>>),
    ctx: &SessionCtx,
    write_half: &Arc<tokio::sync::Mutex<FrameWriter<W>>>,
    batch: &mut Option<BatchState>,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    match frame {
        (LanMessage::CategoryBatchStart { category_name, category_color, item_count }, _) => {
            if let Err(reason) = validate_category_meta(Some(&category_name), category_color.as_deref()) {
                emit_clip_receive_failed(ctx, reason);
                // 拒收该 BatchStart：不进入新的批量态（已存在的旧批量态保持不变——其元数据
                // 已在各自的 BatchStart 处通过校验）；后续逐条帧若无批量态则走单条路径并被再次校验。
                return true;
            }
            // 预排 sort_order：新条目整体插到现有条目之上，且按发送顺序排列。
            // item_count 由对端提供，封顶 LAN_BATCH_MAX_ITEMS 防偏移被放大。
            let min_order = ctx.store.category_min_sort_order(&category_name).unwrap_or(0);
            let cap = i64::from(item_count.min(LAN_BATCH_MAX_ITEMS));
            *batch = Some(BatchState {
                category_name,
                category_color,
                base_order: min_order - cap,
                next_index: 0,
                received: 0,
                failed: 0,
            });
        }
        (LanMessage::CategoryBatchEnd, _) => {
            if let Some(b) = batch.take() {
                emit_category_received(ctx, b.category_name, b.received, b.failed);
            }
        }
        (LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name, auto, origin_node_id }, payload) => {
            if !empty {
                if let Err(reason) = validate_category_meta(category_name.as_deref(), category_color.as_deref()) {
                    emit_clip_receive_failed(ctx, reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    match batch.as_mut() {
                        // 批量中：静默逐条落库（顺序预排），结束时统一 emit。
                        // 批量（分组发送）恒手动路径：auto=false、无 origin。
                        // 守卫 `!auto`：发送侧虽有批量忙标记防 auto 帧插入批量
                        //（registry），此处再兜底——即使 auto 帧因任何原因混进
                        // 批量中段，也绝不被折叠进打开的分组（那会污染对端用户
                        // 分拣的分组），一律走下面的单条路径按 auto 语义处理。
                        Some(b) if !auto => {
                            let order = Some(b.base_order + b.next_index);
                            b.next_index += 1;
                            let ok = apply_received(
                                ctx, &ctx.store, &clip_type, &data,
                                Some(b.category_name.clone()), b.category_color.clone(),
                                display_name, order, true, false, None,
                            );
                            if ok { b.received += 1 } else { b.failed += 1 }
                        }
                        _ => {
                            apply_received(
                                ctx, &ctx.store, &clip_type, &data,
                                category_name, category_color, display_name, None, false,
                                auto, origin_node_id.as_deref(),
                            );
                        }
                    }
                }
            }
        }
        (LanMessage::ClipRequest, _) => {
            match read_current_payload() {
                Ok(Some((ct, data))) => {
                    let empty = false;
                    let msg = LanMessage::ClipResponse {
                        clip_type: ct,
                        empty,
                        category_name: None,
                        category_color: None,
                        display_name: None,
                    };
                    let mut wh = write_half.lock().await;
                    if wh.write_message(&msg, Some(&data)).await.is_err() {
                        return false;
                    }
                }
                Ok(None) | Err(_) => {
                    let msg = LanMessage::ClipResponse {
                        clip_type: "text".into(),
                        empty: true,
                        category_name: None,
                        category_color: None,
                        display_name: None,
                    };
                    let mut wh = write_half.lock().await;
                    let _ = wh.write_message(&msg, None).await;
                }
            }
        }
        (LanMessage::ClipResponse { clip_type, empty, category_name, category_color, display_name }, payload) => {
            if !empty {
                if let Err(reason) = validate_category_meta(category_name.as_deref(), category_color.as_deref()) {
                    emit_clip_receive_failed(ctx, reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    // ClipResponse（对端应答「请求剪贴板」）恒手动路径：auto=false、无 origin。
                    apply_received(
                        ctx, &ctx.store, &clip_type, &data,
                        category_name, category_color, display_name, None, false, false, None,
                    );
                }
            }
        }
        // 心跳：收到 Ping 立即回 Pong（持锁写半，与控制写互斥但不长期争用）。
        (LanMessage::Ping, _) => {
            let mut wh = write_half.lock().await;
            if wh.write_message(&LanMessage::Pong, None).await.is_err() {
                return false;
            }
        }
        // Pong：对端对我方 Ping 的应答，无需处理（无超时判定——连接死亡由
        // iroh 连接层与 dead watcher 兜底）。
        (LanMessage::Pong, _) => {}
        (LanMessage::Disconnect, _) => {
            // 对端主动发来 Disconnect 帧：读任务退出并经 peer_gone 通知主循环；
            // 会话级状态清理（DeviceStatusChanged 等）由 Task 7 的 registry 承担。
            return false;
        }
        // PairRequest/PairAccept/PairReject 不应出现在会话期（配对在首条流完成，
        // 会话流是配对成功后新开的流），忽略。
        _ => {}
    }
    true
}

/// 会话主循环：在「控制指令」「读任务结束信号」「连接死亡」「心跳」之间 `select!`。
///
/// **架构**：两条并行路径共享同一连接——
/// - **读任务**：独立 `tokio::spawn`，反复 `read_message` 并**原地处理**每一条入站帧
///   （`handle_frame`）。`read_message` 内部走 `AsyncReadExt::read_exact`，tokio 官方
///   明确它在 `select!` 里**不 cancellation-safe**，故绝不能放进 select 分支。读任务
///   退出（读到 Disconnect / EOF / 读错）时经 `Notify` 通知主循环。
/// - **主循环（本函数）**：只处理本地控制指令（`control_rx`）、读任务的结束信号、
///   registry 侧的连接死亡通知（`dead`）与心跳定时器，四者都 cancel-safe
///   （`mpsc::Receiver::recv` / `Notify::notified` / `oneshot::Receiver` / `sleep`）。
///
/// **为什么不再用「帧 mpsc」把帧从读任务转发给主循环**：早期实现让读任务把帧经
/// `mpsc` 送给主循环、主循环在「control_rx + frame_rx」两个 receiver 之间 select。
/// 实测在 host 全链路（start_host → accept → 配对 → 会话）里，一旦对端（guest 的
/// 会话循环）并发运行，host 主循环 park 后就**再也唤不醒**——frame_rx 的 send 唤不醒、
/// 心跳定时器唤不醒、连 control_rx 也唤不醒（host 点「断开」因此无响应）；而直连
/// `run_session_loop` 的单测却一切正常。把帧处理下沉到读任务、彻底取消帧通道后，
/// 主循环不再依赖任何「跨任务 receiver 唤醒」来处理入站帧，该现象消失。
/// `write_half` 经 `tokio::sync::Mutex` 在两条路径间共享（写操作互斥；帧处理只在回
/// `ClipResponse`/`Pong` 时短暂持锁，不与控制写长期争用）。
pub(crate) async fn run_session_loop<R, W>(
    read: R,
    write: W,
    ctx: SessionCtx,
    mut control_rx: mpsc::Receiver<ControlMsg>,
    mut dead: tokio::sync::oneshot::Receiver<()>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let peer_label = ctx.peer_device_name.clone();
    eprintln!("[lan-sync] 会话开始：{peer_label}");

    let mut reader = FrameReader::new(read);
    let writer = Arc::new(tokio::sync::Mutex::new(FrameWriter::new(write)));
    // 读任务结束信号：读到 Disconnect / EOF / 读错时通知主循环。
    let peer_gone = Arc::new(tokio::sync::Notify::new());

    // 读任务：循环读帧并原地处理。处理要求结束会话（对端 Disconnect / 回写失败）
    // 或读错时退出，并通知主循环。批态 BatchState 仅帧处理使用，读任务独占。
    let read_ctx = ctx; // move 进读任务
    let read_write = writer.clone();
    let read_signal = peer_gone.clone();
    let read_task = tokio::spawn(async move {
        // 接收侧整组传输状态：None = 未在批量中（逐条行为）。
        let mut batch: Option<BatchState> = None;
        loop {
            match reader.read_message().await {
                Ok(frame) => {
                    // false = 对端 Disconnect / 回写失败：结束读任务。
                    if !handle_frame(frame, &read_ctx, &read_write, &mut batch).await {
                        break;
                    }
                }
                Err(_) => {
                    // EOF / 流错误：读任务退出，通知主循环（对端断开）。
                    break;
                }
            }
        }
        read_signal.notify_one();
    });

    loop {
        tokio::select! {
            biased;
            // 本地控制指令；None = 所有 sender dropped（如 registry 关闭）= 干净关闭。
            control = control_rx.recv() => {
                match control {
                Some(ControlMsg::BatchStart { category_name, category_color, item_count }) => {
                    let msg = LanMessage::CategoryBatchStart { category_name, category_color, item_count };
                    let mut wh = writer.lock().await;
                    if wh.write_message(&msg, None).await.is_err() {
                        break;
                    }
                }
                Some(ControlMsg::BatchEnd) => {
                    let mut wh = writer.lock().await;
                    if wh.write_message(&LanMessage::CategoryBatchEnd, None).await.is_err() {
                        break;
                    }
                }
                Some(ControlMsg::SendClip { clip_type, payload, category_name, category_color, display_name, auto, origin_node_id }) => {
                    let empty = payload.is_empty();
                    // auto/origin_node_id 透传（Spec 2 捕获即自动同步由后续任务
                    // 在构造 ControlMsg 时填写）；手动发送恒为非自动、无 origin。
                    let msg = LanMessage::ClipPush {
                        clip_type,
                        empty,
                        category_name,
                        category_color,
                        display_name,
                        auto,
                        origin_node_id,
                    };
                    let mut wh = writer.lock().await;
                    if wh.write_message(&msg, if empty { None } else { Some(&payload) }).await.is_err() {
                        break;
                    }
                }
                Some(ControlMsg::RequestClip) => {
                    let mut wh = writer.lock().await;
                    if wh.write_message(&LanMessage::ClipRequest, None).await.is_err() {
                        break;
                    }
                }
                // 本地主动断开（registry drop 控制通道）：尽力发一帧 Disconnect，随即退出。
                None => {
                    let mut wh = writer.lock().await;
                    let _ = wh.write_message(&LanMessage::Disconnect, None).await;
                    break;
                }
                }
            },
            // 读任务结束 = 对端断开（Disconnect 帧 / EOF / 读错）。
            // 会话级状态清理（DeviceStatusChanged）由 Task 7 的 registry 承担，
            // 这里只结束循环。
            _ = peer_gone.notified() => break,
            // registry 侧连接死亡（conn.closed() watcher）：立即结束。
            _ = &mut dead => break,
            // 心跳：每 30s 发一帧 Ping。同时定期唤醒 select!，避免因漏注册
            // waker 而长期沉睡（v4 曾以此 1s 空唤醒自愈过 park 问题）。
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                let mut wh = writer.lock().await;
                if wh.write_message(&LanMessage::Ping, None).await.is_err() {
                    break;
                }
            }
        }
    }

    // 主循环退出：中止读任务（若它仍在阻塞读），避免悬挂。
    read_task.abort();
    eprintln!("[lan-sync] 会话结束：{peer_label}");
}

#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod category_meta_tests {
    use super::*;

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
