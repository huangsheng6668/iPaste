//! v5 明文会话层：泛型于 `AsyncRead`/`AsyncWrite` 的双工帧循环。
//!
//! 传输安全由 iroh QUIC TLS 承担（线格式即明文 JSON header + payload，见
//! frame.rs）；本文件只负责「读任务 + 控制主循环」的会话编排与入站帧处理。

use std::sync::Arc;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::clipboard::{
    captured_item_from_payload, read_current_clipboard, write_clipboard_image, write_clipboard_text,
};
use crate::events::{
    DeviceCategoryReceived, DeviceClipReceiveFailed, DeviceClipReceived, EVENT_DEVICE_CATEGORY_RECEIVED,
    EVENT_DEVICE_CLIP_RECEIVE_FAILED, EVENT_DEVICE_CLIP_RECEIVED,
};
use crate::lan_sync::frame::{FrameReader, FrameWriter};
use crate::lan_sync::protocol::{LAN_BATCH_MAX_ITEMS, LanMessage};
use crate::lan_sync::{ControlMsg, LanEventSink};
use crate::models::ClipboardRead;
use crate::store::Store;
use crate::util::clean_display_name;

/// 单个会话的固定上下文：事件出口、落库句柄与对端标识。
/// Task 7 的 DeviceLinkRegistry 在建立双向流后构造它并调用 `run_session_loop`。
#[allow(dead_code)] // 构造方在 Task 7 接线；本任务仅会话循环与测试消费
pub(crate) struct SessionCtx {
    pub sink: Arc<dyn LanEventSink>,
    pub store: Store,
    /// 对端 EndpointId 的 hex（64 字符）：v5 事件的设备标识。
    pub peer_node_id: String,
    pub peer_device_name: String,
}

/// 会话内统一事件出口：payload 序列化失败按 Null 发出（与 v4 行为一致）。
fn emit<E: serde::Serialize>(ctx: &SessionCtx, event: &str, payload: E) {
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    ctx.sink.emit(event, &value);
}

/// EndpointId 前 4 字节 hex —— UI 指纹短码（非安全锚点，仅展示核对用）。
/// Task 7 配对流程消费；纯函数独立测试。
#[allow(dead_code)]
pub(crate) fn fingerprint_hex(endpoint_id: &[u8; 32]) -> String {
    endpoint_id.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// 处理收到的剪贴板内容：写系统剪贴板 + 落库 + emit
///
/// - `category_name` 为 `None`：历史/无分组条目。写系统剪贴板 + 入 `clips`（历史）表（旧行为）。
/// - `category_name` 为 `Some`：分组条目。**不写系统剪贴板**（避免打扰用户当前剪贴板），
///   落到匹配/新建的同名分组下的 `category_items` 表。
///
/// `display_name`：对端条目的重命名显示名（历史条目落到 `clips.display_name`，
/// 分组条目落到 `category_items.display_name`）。
/// `sort_order`：整组接收时预排的分组内顺序（保持发送顺序）；`None` 走旧行为
/// （插到分组顶部）。
/// `silent`：整组接收时为 true——不逐条 emit 事件，由调用方在批量结束时汇总。
/// 返回是否成功落库（供批量统计）。
#[allow(clippy::too_many_arguments)]
fn apply_received(
    ctx: &SessionCtx,
    store: &Store,
    clip_type: &str,
    payload: &[u8],
    category_name: Option<String>,
    category_color: Option<String>,
    display_name: Option<String>,
    sort_order: Option<i64>,
    silent: bool,
) -> bool {
    // 图片 = data url（utf-8）；文本 = 原文 utf-8
    let text = String::from_utf8_lossy(payload).to_string();
    let mut item = match captured_item_from_payload(clip_type, &text) {
        Ok(Some(item)) => item,
        // 空文本（trim 后为空）：不诊断（对端发空 payload 是合法的「清空」语义之外的罕见情况）
        Ok(None) => {
            if !silent {
                emit_clip_receive_failed(ctx, "收到空内容，已忽略".to_string());
            }
            return false;
        }
        // 解析失败（如图片 data url 损坏 / 对端发了本地路径读不到）：暴露原因
        Err(reason) => {
            if !silent {
                emit_clip_receive_failed(ctx, format!("解析收到的内容失败：{reason}"));
            }
            return false;
        }
    };

    // 对端重命名：清洗（trim、空 → None、超长报错）。清洗失败按该条目失败处理。
    let display_name = match clean_display_name(display_name) {
        Ok(name) => name,
        Err(reason) => {
            if !silent {
                emit_clip_receive_failed(ctx, format!("条目名称无效：{reason}"));
            }
            return false;
        }
    };
    item.display_name = display_name.clone();

    match category_name {
        // 分组条目：不触碰系统剪贴板，直接落到 category_items
        Some(name) => {
            // 图片条目：captured_item_from_payload 解码出的是内存字节（text 为空串），
            // 先落盘成文件路径（与历史条目 insert_captured_item 的处理一致），
            // 否则 B 端 category_items/clips 里存的 text 是空串，图片内容等于丢失。
            let stored_text = if clip_type == "image" {
                match item.image_bytes.as_deref() {
                    Some(bytes) => match store.save_image_bytes(&item.content_hash, bytes) {
                        Ok(path) => path,
                        Err(reason) => {
                            if !silent {
                                emit_clip_receive_failed(ctx, format!("保存图片文件失败：{reason}"));
                            }
                            return false;
                        }
                    },
                    None => item.text.clone(),
                }
            } else {
                item.text.clone()
            };
            match store.insert_received_category_item(
                item.clip_type,
                item.content_hash,
                item.preview_text,
                stored_text,
                name.clone(),
                category_color,
                display_name,
                sort_order,
            ) {
                Ok(_) => {
                    if !silent {
                        emit_clip_received(ctx, clip_type.to_string(), Some(name));
                    }
                    true
                }
                // 落库失败（DB 约束/磁盘等）：暴露原因，不再静默吞掉。
                // name 是对端可控输入（协议层仅限制帧大小），失败提示里必须截断，
                // 避免恶意对端把超长字符串灌进 UI。
                Err(reason) => {
                    if !silent {
                        let short_name: String = name.chars().take(40).collect();
                        emit_clip_receive_failed(
                            ctx,
                            format!("保存到分组「{short_name}」失败：{reason}"),
                        );
                    }
                    false
                }
            }
        }
        // 历史/无分组：保持旧行为
        None => {
            let write_result = if clip_type == "image" {
                write_clipboard_image(&text)
            } else {
                write_clipboard_text(item.text.trim())
            };
            if let Err(reason) = write_result {
                if !silent {
                    emit_clip_receive_failed(ctx, format!("写入系统剪贴板失败：{reason}"));
                }
                return false;
            }
            // 落库失败必须暴露：此前静默吞掉后仍提示「已接收」，导致
            // 「B 端提示已接收但条目未入库」且无从排查。
            match store.insert_captured_item(item) {
                Ok(_) => {
                    if !silent {
                        emit_clip_received(ctx, clip_type.to_string(), None);
                    }
                    true
                }
                Err(reason) => {
                    if !silent {
                        emit_clip_receive_failed(ctx, format!("保存到历史失败：{reason}"));
                    }
                    false
                }
            }
        }
    }
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

/// 单条接收成功：emit 汇总事件（历史条目 category_name = None）。
fn emit_clip_received(ctx: &SessionCtx, clip_type: String, category_name: Option<String>) {
    emit(
        ctx,
        EVENT_DEVICE_CLIP_RECEIVED,
        &DeviceClipReceived { node_id: ctx.peer_node_id.clone(), clip_type, category_name },
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
        (LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name, .. }, payload) => {
            if !empty {
                if let Err(reason) = validate_category_meta(category_name.as_deref(), category_color.as_deref()) {
                    emit_clip_receive_failed(ctx, reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    match batch.as_mut() {
                        // 批量中：静默逐条落库（顺序预排），结束时统一 emit。
                        Some(b) => {
                            let order = Some(b.base_order + b.next_index);
                            b.next_index += 1;
                            let ok = apply_received(
                                ctx, &ctx.store, &clip_type, &data,
                                Some(b.category_name.clone()), b.category_color.clone(),
                                display_name, order, true,
                            );
                            if ok { b.received += 1 } else { b.failed += 1 }
                        }
                        None => {
                            apply_received(
                                ctx, &ctx.store, &clip_type, &data,
                                category_name, category_color, display_name, None, false,
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
                    apply_received(
                        ctx, &ctx.store, &clip_type, &data,
                        category_name, category_color, display_name, None, false,
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
                Some(ControlMsg::SendClip { clip_type, payload, category_name, category_color, display_name }) => {
                    let empty = payload.is_empty();
                    // auto/origin_node_id：Spec 2 捕获即自动同步由 Task 7 接线时填写；
                    // 手动发送恒为非自动、无 origin。
                    let msg = LanMessage::ClipPush {
                        clip_type,
                        empty,
                        category_name,
                        category_color,
                        display_name,
                        auto: false,
                        origin_node_id: None,
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
                // 本地主动断开：尽力发一帧 Disconnect，随即退出。
                Some(ControlMsg::Disconnect) | None => {
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
    use crate::lan_sync::NoopEventSink;
    use crate::store::test_support::temp_store;
    use tokio::io::duplex;

    fn test_ctx() -> SessionCtx {
        SessionCtx {
            sink: Arc::new(NoopEventSink),
            store: temp_store(),
            peer_node_id: "ab".repeat(32),
            peer_device_name: "peer-device".to_string(),
        }
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
