use std::sync::Arc;

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::clipboard::{
    captured_item_from_payload, read_current_clipboard, write_clipboard_image, write_clipboard_text,
};
use crate::util::clean_display_name;
use crate::lan_sync::crypto::SecureConnection;
use crate::lan_sync::protocol::*;
use crate::lan_sync::{ControlMsg, LanSessionManager, LanStatus};
use crate::models::ClipboardRead;
use crate::store::Store;

/// TCP 帧读写：`[u32 header_len LE][header bytes]` 紧接 `[u32 payload_len LE][payload bytes]`
/// （仅当 header 为 `ClipPush`/`ClipResponse` 且 `empty=false` 时才有 payload 段）。
pub(crate) struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub(crate) fn new(stream: TcpStream) -> Self {
        Self { stream }
    }

    /// 消费 Connection，返回底层 TcpStream（用于握手完成后转入 session loop）。
    pub(crate) fn into_stream(self) -> TcpStream {
        self.stream
    }

    async fn read_u32(&mut self) -> Result<u32, String> {
        let mut buf = [0u8; 4];
        self.stream
            .read_exact(&mut buf)
            .await
            .map_err(|e| e.to_string())?;
        Ok(u32::from_le_bytes(buf))
    }

    pub(crate) async fn read_message(&mut self) -> Result<(LanMessage, Option<Vec<u8>>), String> {
        let header_len = self.read_u32().await? as usize;
        if header_len > LAN_MAX_PAYLOAD {
            return Err("帧头超限".to_string());
        }
        let mut header = vec![0u8; header_len];
        self.stream
            .read_exact(&mut header)
            .await
            .map_err(|e| e.to_string())?;
        let msg: LanMessage = serde_json::from_slice(&header).map_err(|e| e.to_string())?;

        let has_payload = matches!(
            &msg,
            LanMessage::ClipPush { empty: false, .. }
                | LanMessage::ClipResponse { empty: false, .. }
        );
        let payload = if has_payload {
            let payload_len = self.read_u32().await? as usize;
            if payload_len > LAN_MAX_PAYLOAD {
                return Err("payload 超限".to_string());
            }
            let mut payload = vec![0u8; payload_len];
            self.stream
                .read_exact(&mut payload)
                .await
                .map_err(|e| e.to_string())?;
            Some(payload)
        } else {
            None
        };
        Ok((msg, payload))
    }

    pub(crate) async fn write_message(
        &mut self,
        msg: &LanMessage,
        payload: Option<&[u8]>,
    ) -> Result<(), String> {
        let header = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
        self.stream
            .write_all(&(header.len() as u32).to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.stream
            .write_all(&header)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(data) = payload {
            self.stream
                .write_all(&(data.len() as u32).to_le_bytes())
                .await
                .map_err(|e| e.to_string())?;
            self.stream
                .write_all(data)
                .await
                .map_err(|e| e.to_string())?;
        }
        self.stream.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }
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
    manager: &LanSessionManager,
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
                manager.emit_clip_receive_failed("收到空内容，已忽略".to_string());
            }
            return false;
        }
        // 解析失败（如图片 data url 损坏 / 对端发了本地路径读不到）：暴露原因
        Err(reason) => {
            if !silent {
                manager.emit_clip_receive_failed(format!("解析收到的内容失败：{reason}"));
            }
            return false;
        }
    };

    // 对端重命名：清洗（trim、空 → None、超长报错）。清洗失败按该条目失败处理。
    let display_name = match clean_display_name(display_name) {
        Ok(name) => name,
        Err(reason) => {
            if !silent {
                manager.emit_clip_receive_failed(format!("条目名称无效：{reason}"));
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
                                manager.emit_clip_receive_failed(format!("保存图片文件失败：{reason}"));
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
                        manager.emit_clip_received(clip_type.to_string(), Some(name));
                    }
                    true
                }
                // 落库失败（DB 约束/磁盘等）：暴露原因，不再静默吞掉。
                // name 是对端可控输入（协议层仅限制帧大小），失败提示里必须截断，
                // 避免恶意对端把超长字符串灌进 UI。
                Err(reason) => {
                    if !silent {
                        let short_name: String = name.chars().take(40).collect();
                        manager.emit_clip_receive_failed(format!("保存到分组「{short_name}」失败：{reason}"));
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
                    manager.emit_clip_receive_failed(format!("写入系统剪贴板失败：{reason}"));
                }
                return false;
            }
            // 落库失败必须暴露：此前静默吞掉后仍提示「已接收」，导致
            // 「B 端提示已接收但条目未入库」且无从排查。
            match store.insert_captured_item(item) {
                Ok(_) => {
                    if !silent {
                        manager.emit_clip_received(clip_type.to_string(), None);
                    }
                    true
                }
                Err(reason) => {
                    if !silent {
                        manager.emit_clip_receive_failed(format!("保存到历史失败：{reason}"));
                    }
                    false
                }
            }
        }
    }
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

/// 处理一条入站帧。返回 `false` 表示该帧要求结束会话（对端 Disconnect）。
///
/// 由会话的**读任务**逐条调用（读到即原地处理）；`write_half` 是与主循环共享的
/// 写半（仅回 `ClipResponse` 时短暂加锁）。
#[allow(clippy::too_many_arguments)]
async fn handle_frame(
    frame: (LanMessage, Option<Vec<u8>>),
    manager: &Arc<LanSessionManager>,
    store: &Store,
    write_half: &Arc<tokio::sync::Mutex<crate::lan_sync::crypto::SecureWriteHalf>>,
    batch: &mut Option<BatchState>,
) -> bool {
    match frame {
        (LanMessage::CategoryBatchStart { category_name, category_color, item_count }, _) => {
            if let Err(reason) = validate_category_meta(Some(&category_name), category_color.as_deref()) {
                manager.emit_clip_receive_failed(reason);
                // 拒收该 BatchStart：不进入新的批量态（已存在的旧批量态保持不变——其元数据
                // 已在各自的 BatchStart 处通过校验）；后续逐条帧若无批量态则走单条路径并被再次校验。
                return true;
            }
            // 预排 sort_order：新条目整体插到现有条目之上，且按发送顺序排列。
            // item_count 由对端提供，封顶 LAN_BATCH_MAX_ITEMS 防偏移被放大。
            let min_order = store.category_min_sort_order(&category_name).unwrap_or(0);
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
                manager.emit_category_received(b.category_name, b.received, b.failed);
            }
        }
        (LanMessage::ClipPush { clip_type, empty, category_name, category_color, display_name }, payload) => {
            if !empty {
                if let Err(reason) = validate_category_meta(category_name.as_deref(), category_color.as_deref()) {
                    manager.emit_clip_receive_failed(reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    match batch.as_mut() {
                        // 批量中：静默逐条落库（顺序预排），结束时统一 emit。
                        Some(b) => {
                            let order = Some(b.base_order + b.next_index);
                            b.next_index += 1;
                            let ok = apply_received(
                                manager, store, &clip_type, &data,
                                Some(b.category_name.clone()), b.category_color.clone(),
                                display_name, order, true,
                            );
                            if ok { b.received += 1 } else { b.failed += 1 }
                        }
                        None => {
                            apply_received(
                                manager, store, &clip_type, &data,
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
                    manager.emit_clip_receive_failed(reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    apply_received(
                        manager, store, &clip_type, &data,
                        category_name, category_color, display_name, None, false,
                    );
                }
            }
        }
        (LanMessage::Disconnect, _) => {
            // 对端主动发来 Disconnect 帧。若本地主循环已抢先清理（如本地刚断开、
            // abort 读任务前来得及处理这条帧），不再重复 reset，避免二次 emit。
            if !matches!(manager.snapshot().status, LanStatus::Idle) {
                manager.reset_to_idle("对方已断开".to_string());
            }
            return false;
        }
        _ => { /* Handshake/Pair* 不应在会话期出现，忽略 */ }
    }
    true
}

/// 会话主循环：在「控制指令」与「读任务结束信号」之间 `select!`。
///
/// 调用方（Task 4/5）保证在进入前已调用 `set_hosting`/`set_joining`，因此 `role` 已设置。
///
/// **架构**：两条并行路径共享同一连接——
/// - **读任务**：独立 `tokio::spawn`，反复 `read_message` 并**原地处理**每一条入站帧
///   （`handle_frame`）。`read_message` 内部走 `AsyncReadExt::read_exact`，tokio 官方
///   明确它在 `select!` 里**不 cancellation-safe**，故绝不能放进 select 分支。读任务
///   退出（读到 Disconnect / EOF / 解密失败）时经 `Notify` 通知主循环。
/// - **主循环（本函数）**：只处理本地控制指令（`control_rx`）与读任务的结束信号，
///   两者都 cancel-safe（`mpsc::Receiver::recv` / `Notify::notified`）。
///
/// **为什么不再用「帧 mpsc」把帧从读任务转发给主循环**：早期实现让读任务把帧经
/// `mpsc` 送给主循环、主循环在「control_rx + frame_rx」两个 receiver 之间 select。
/// 实测在 host 全链路（start_host → accept → 配对 → 会话）里，一旦对端（guest 的
/// 会话循环）并发运行，host 主循环 park 后就**再也唤不醒**——frame_rx 的 send 唤不醒、
/// 心跳定时器唤不醒、连 control_rx 也唤不醒（host 点「断开」因此无响应）；而直连
/// `run_session_loop` 的单测却一切正常。把帧处理下沉到读任务、彻底取消帧通道后，
/// 主循环不再依赖任何「跨任务 receiver 唤醒」来处理入站帧，该现象消失。
/// `write_half` 经 `tokio::sync::Mutex` 在两条路径间共享（写操作互斥；帧处理只在回
/// `ClipResponse` 时短暂持锁，不与控制写长期争用）。
pub(crate) async fn run_session_loop(
    conn: SecureConnection,
    manager: Arc<LanSessionManager>,
    store: Store,
    peer_device_name: String,
    mut control_rx: mpsc::Receiver<ControlMsg>,
) {
    manager.set_connected(peer_device_name);

    // 拆成读/写两半：读半交给独立读任务；写半经 Mutex 在读任务与主循环间共享。
    let (mut read_half, write_half) = conn.into_split();
    let write_half = Arc::new(tokio::sync::Mutex::new(write_half));
    // 读任务结束信号：读到 Disconnect / EOF / 解密失败时通知主循环。
    let peer_gone = Arc::new(tokio::sync::Notify::new());

    // 读任务：循环读帧并原地处理。处理要求结束会话（对端 Disconnect）或读错时退出，
    // 并通知主循环。
    let read_manager = manager.clone();
    let read_store = store.clone();
    let read_write = write_half.clone();
    let read_signal = peer_gone.clone();
    let read_task = tokio::spawn(async move {
        // 接收侧整组传输状态：None = 未在批量中（逐条行为）。仅帧处理使用，读任务独占。
        let mut batch: Option<BatchState> = None;
        loop {
            match read_half.read_message().await {
                Ok(frame) => {
                    // false = 对端 Disconnect / 回写失败：结束读任务。
                    if !handle_frame(frame, &read_manager, &read_store, &read_write, &mut batch).await {
                        break;
                    }
                }
                Err(_) => {
                    // 解密失败 / EOF / 流错误：读任务退出，通知主循环（对端断开）。
                    break;
                }
            }
        }
        read_signal.notify_one();
    });

    loop {
        tokio::select! {
            biased;
            // 本地控制指令；None = 所有 sender dropped（如 reset_to_idle 后）= 干净关闭。
            control = control_rx.recv() => {
                match control {
                Some(ControlMsg::BatchStart { category_name, category_color, item_count }) => {
                    let msg = LanMessage::CategoryBatchStart { category_name, category_color, item_count };
                    let mut wh = write_half.lock().await;
                    if wh.write_message(&msg, None).await.is_err() {
                        drop(wh);
                        manager.reset_to_idle("连接已断开".to_string());
                        break;
                    }
                }
                Some(ControlMsg::BatchEnd) => {
                    let mut wh = write_half.lock().await;
                    if wh.write_message(&LanMessage::CategoryBatchEnd, None).await.is_err() {
                        drop(wh);
                        manager.reset_to_idle("连接已断开".to_string());
                        break;
                    }
                }
                Some(ControlMsg::SendClip { clip_type, payload, category_name, category_color, display_name }) => {
                    let empty = payload.is_empty();
                    let msg = LanMessage::ClipPush {
                        clip_type: clip_type.clone(),
                        empty,
                        category_name,
                        category_color,
                        display_name,
                    };
                    let mut wh = write_half.lock().await;
                    if wh.write_message(&msg, if empty { None } else { Some(&payload) }).await.is_err() {
                        drop(wh);
                        manager.reset_to_idle("连接已断开".to_string());
                        break;
                    }
                }
                Some(ControlMsg::RequestClip) => {
                    let mut wh = write_half.lock().await;
                    if wh.write_message(&LanMessage::ClipRequest, None).await.is_err() {
                        drop(wh);
                        manager.reset_to_idle("连接已断开".to_string());
                        break;
                    }
                }
                // 所有 sender dropped（如 reset_to_idle 后）视作干净关闭
                Some(ControlMsg::Disconnect) | None => {
                    // 本地主动断开：尽力发一帧 Disconnect，随即清理。
                    let mut wh = write_half.lock().await;
                    let _ = wh.write_message(&LanMessage::Disconnect, None).await;
                    drop(wh);
                    manager.reset_to_idle("已断开".to_string());
                    break;
                }
                }
            },
            // 读任务结束 = 对端断开（Disconnect 帧 / EOF / 解密失败）。
            // Disconnect 帧已由读任务的 handle_frame 处理并 reset（状态已 Idle），
            // 这里只兜底读错/EOF 的情况，避免重复 emit 断开事件。
            _ = peer_gone.notified() => {
                if !matches!(manager.snapshot().status, LanStatus::Idle) {
                    manager.reset_to_idle("连接已断开".to_string());
                }
                break;
            },
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                // 心跳：定期唤醒，避免 select! 因漏注册 waker 而长期沉睡（也用于诊断）。
            }
        }
    }

    // 主循环退出：中止读任务（若它仍在阻塞读），避免悬挂。
    read_task.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn connection_roundtrips_clip_push_with_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let (msg, payload) = conn.read_message().await.unwrap();
            assert!(matches!(msg, LanMessage::ClipPush { clip_type, empty: false, .. } if clip_type == "text"));
            assert_eq!(payload, Some(b"hello".to_vec()));
        });

        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        client
            .write_message(
                &LanMessage::ClipPush {
                    clip_type: "text".into(),
                    empty: false,
                    category_name: None,
                    category_color: None,
                    display_name: None,
                },
                Some(b"hello"),
            )
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn empty_clip_push_has_no_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let (msg, payload) = conn.read_message().await.unwrap();
            assert!(matches!(msg, LanMessage::ClipPush { empty: true, .. }));
            assert_eq!(payload, None);
        });
        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        client
            .write_message(
                &LanMessage::ClipPush {
                    clip_type: "text".into(),
                    empty: true,
                    category_name: None,
                    category_color: None,
                    display_name: None,
                },
                None,
            )
            .await
            .unwrap();
        server.await.unwrap();
    }

    /// 整组传输帧（CategoryBatchStart/End）在 TCP 上往返后字段完整、无 payload。
    #[tokio::test]
    async fn category_batch_frames_roundtrip_without_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut conn = Connection::new(stream);
            let (msg, payload) = conn.read_message().await.unwrap();
            match msg {
                LanMessage::CategoryBatchStart { category_name, category_color, item_count } => {
                    assert_eq!(category_name, "工作");
                    assert_eq!(category_color.as_deref(), Some("#0D9488"));
                    assert_eq!(item_count, 3);
                }
                other => panic!("wrong variant: {other:?}"),
            }
            assert_eq!(payload, None);
            let (msg, payload) = conn.read_message().await.unwrap();
            assert_eq!(msg, LanMessage::CategoryBatchEnd);
            assert_eq!(payload, None);
        });

        let mut client = Connection::new(TcpStream::connect(addr).await.unwrap());
        client
            .write_message(
                &LanMessage::CategoryBatchStart {
                    category_name: "工作".into(),
                    category_color: Some("#0D9488".into()),
                    item_count: 3,
                },
                None,
            )
            .await
            .unwrap();
        client
            .write_message(&LanMessage::CategoryBatchEnd, None)
            .await
            .unwrap();
        server.await.unwrap();
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
