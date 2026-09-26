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
use crate::lan_sync::protocol::LanMessage;
use crate::lan_sync::{ControlMsg, LanEventSink};
use crate::models::{AppendCopyState, ClipboardRead};
use crate::store::Store;

mod apply;
mod dispatch;
#[cfg(test)]
mod tests;

use self::dispatch::{handle_frame, BatchState};

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
    endpoint_id
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 接收侧解析/落库失败：emit 诊断事件 + 打印日志，避免静默丢弃。
fn emit_clip_receive_failed(ctx: &SessionCtx, reason: String) {
    eprintln!("[lan-sync] 接收条目失败：{reason}");
    emit(
        ctx,
        EVENT_DEVICE_CLIP_RECEIVE_FAILED,
        &DeviceClipReceiveFailed {
            node_id: ctx.peer_node_id.clone(),
            reason,
        },
    );
}

/// 整组接收完成：emit 汇总事件（接收端据此刷新一次列表并提示）。
fn emit_category_received(ctx: &SessionCtx, category_name: String, count: u32, failed: u32) {
    emit(
        ctx,
        EVENT_DEVICE_CATEGORY_RECEIVED,
        &DeviceCategoryReceived {
            node_id: ctx.peer_node_id.clone(),
            category_name,
            count,
            failed,
        },
    );
}

/// 读取当前剪贴板并转换为 LAN 协议要发的 `(clip_type, payload_bytes)`。
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
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!("data:image/png;base64,{}", b64))
}

/// 接收路径的分组字段上限：与本地 `clean_display_name` 的 80 字符口径对齐，
/// 防已配对对端灌超长字符串污染本地 DB 与 UI 事件流。
pub(crate) const MAX_CATEGORY_NAME_LEN: usize = 80;
pub(crate) const MAX_CATEGORY_COLOR_LEN: usize = 32;
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
