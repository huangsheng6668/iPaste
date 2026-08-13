use std::sync::Arc;

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::clipboard::{
    captured_item_from_payload, read_current_clipboard, write_clipboard_image, write_clipboard_text,
};
use crate::lan_sync::crypto::SecureConnection;
use crate::lan_sync::protocol::*;
use crate::lan_sync::{ControlMsg, LanSessionManager};
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
fn apply_received(
    manager: &LanSessionManager,
    store: &Store,
    clip_type: &str,
    payload: &[u8],
    category_name: Option<String>,
    category_color: Option<String>,
) {
    // 图片 = data url（utf-8）；文本 = 原文 utf-8
    let text = String::from_utf8_lossy(payload).to_string();
    let Ok(Some(item)) = captured_item_from_payload(clip_type, &text) else {
        return;
    };

    match category_name {
        // 分组条目：不触碰系统剪贴板，直接落到 category_items
        Some(name) => {
            match store.insert_received_category_item(
                item.clip_type,
                item.content_hash,
                item.preview_text,
                item.text,
                name.clone(),
                category_color,
            ) {
                Ok(_) => manager.emit_clip_received(clip_type.to_string(), Some(name)),
                Err(_) => return,
            }
        }
        // 历史/无分组：保持旧行为
        None => {
            let write_result = if clip_type == "image" {
                write_clipboard_image(&text)
            } else {
                write_clipboard_text(item.text.trim())
            };
            if write_result.is_err() {
                return;
            }
            let _ = store.insert_captured_item(item);
            manager.emit_clip_received(clip_type.to_string(), None);
        }
    }
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

/// 会话主循环：在控制指令与入站帧之间 `select!`，自动响应 `ClipRequest`。
///
/// 调用方（Task 4/5）保证在进入前已调用 `set_hosting`/`set_joining`，因此 `role` 已设置。
pub(crate) async fn run_session_loop(
    mut conn: SecureConnection,
    manager: Arc<LanSessionManager>,
    store: Store,
    peer_device_name: String,
    mut control_rx: mpsc::Receiver<ControlMsg>,
) {
    manager.set_connected(peer_device_name);

    loop {
        tokio::select! {
            biased;
            control = control_rx.recv() => match control {
                Some(ControlMsg::SendClip { clip_type, payload, category_name, category_color }) => {
                    let empty = payload.is_empty();
                    let msg = LanMessage::ClipPush {
                        clip_type: clip_type.clone(),
                        empty,
                        category_name,
                        category_color,
                    };
                    if conn.write_message(&msg, if empty { None } else { Some(&payload) }).await.is_err() {
                        manager.reset_to_idle("连接已断开".to_string());
                        return;
                    }
                }
                Some(ControlMsg::RequestClip) => {
                    if conn.write_message(&LanMessage::ClipRequest, None).await.is_err() {
                        manager.reset_to_idle("连接已断开".to_string());
                        return;
                    }
                }
                // 所有 sender dropped（如 reset_to_idle 后）视作干净关闭
                Some(ControlMsg::Disconnect) | None => {
                    let _ = conn.write_message(&LanMessage::Disconnect, None).await;
                    manager.reset_to_idle("已断开".to_string());
                    return;
                }
            },
            read_result = conn.read_message() => match read_result {
                Ok((LanMessage::ClipPush { clip_type, empty, category_name, category_color }, payload)) => {
                    if !empty {
                        if let Some(data) = payload {
                            apply_received(&manager, &store, &clip_type, &data, category_name, category_color);
                        }
                    }
                }
                Ok((LanMessage::ClipRequest, _)) => {
                    match read_current_payload() {
                        Ok(Some((ct, data))) => {
                            let empty = false;
                            let msg = LanMessage::ClipResponse {
                                clip_type: ct,
                                empty,
                                category_name: None,
                                category_color: None,
                            };
                            if conn.write_message(&msg, Some(&data)).await.is_err() {
                                manager.reset_to_idle("连接已断开".to_string());
                                return;
                            }
                        }
                        Ok(None) => {
                            let msg = LanMessage::ClipResponse {
                                clip_type: "text".into(),
                                empty: true,
                                category_name: None,
                                category_color: None,
                            };
                            let _ = conn.write_message(&msg, None).await;
                        }
                        Err(_) => {
                            let msg = LanMessage::ClipResponse {
                                clip_type: "text".into(),
                                empty: true,
                                category_name: None,
                                category_color: None,
                            };
                            let _ = conn.write_message(&msg, None).await;
                        }
                    }
                }
                Ok((LanMessage::ClipResponse { clip_type, empty, category_name, category_color }, payload)) => {
                    if !empty {
                        if let Some(data) = payload {
                            apply_received(&manager, &store, &clip_type, &data, category_name, category_color);
                        }
                    }
                }
                Ok((LanMessage::Disconnect, _)) => {
                    manager.reset_to_idle("对方已断开".to_string());
                    return;
                }
                Ok(_) => { /* Handshake/Pair* 不应在会话期出现，忽略 */ }
                Err(_) => {
                    manager.reset_to_idle("连接已断开".to_string());
                    return;
                }
            }
        }
    }
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
                },
                None,
            )
            .await
            .unwrap();
        server.await.unwrap();
    }
}
