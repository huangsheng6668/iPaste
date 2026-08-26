//! 会话帧编解码：`[u32 header_len LE][JSON header][u32 payload_len LE][payload]`。
//!
//! 与 iroh 无耦合：Reader/Writer 泛型于 `AsyncRead`/`AsyncWrite`，测试用
//! `tokio::io::duplex`，生产端接 iroh 的 `RecvStream`/`SendStream`。

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::lan_sync::protocol::{LanMessage, LAN_MAX_PAYLOAD};

pub(crate) struct FrameReader<R> {
    read: R,
}

pub(crate) struct FrameWriter<W> {
    write: W,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(crate) fn new(read: R) -> Self {
        Self { read }
    }

    pub(crate) async fn read_message(&mut self) -> Result<(LanMessage, Option<Vec<u8>>), String> {
        let header_len = self.read_u32().await? as usize;
        if header_len > LAN_MAX_PAYLOAD {
            return Err("帧头超限".to_string());
        }
        let mut header = vec![0u8; header_len];
        self.read.read_exact(&mut header).await.map_err(|e| e.to_string())?;
        let msg: LanMessage = serde_json::from_slice(&header).map_err(|e| e.to_string())?;

        let has_payload = matches!(
            &msg,
            LanMessage::ClipPush { empty: false, .. } | LanMessage::ClipResponse { empty: false, .. }
        );
        if !has_payload {
            return Ok((msg, None));
        }
        let payload_len = self.read_u32().await? as usize;
        if payload_len > LAN_MAX_PAYLOAD {
            return Err("payload 超限".to_string());
        }
        let mut payload = vec![0u8; payload_len];
        self.read.read_exact(&mut payload).await.map_err(|e| e.to_string())?;
        Ok((msg, Some(payload)))
    }

    async fn read_u32(&mut self) -> Result<u32, String> {
        let mut buf = [0u8; 4];
        self.read.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
        Ok(u32::from_le_bytes(buf))
    }
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub(crate) fn new(write: W) -> Self {
        Self { write }
    }

    pub(crate) async fn write_message(
        &mut self,
        msg: &LanMessage,
        payload: Option<&[u8]>,
    ) -> Result<(), String> {
        // 发送侧上限（v0.9.2 A4）：与 reader 的 LAN_MAX_PAYLOAD 对齐。此前只有
        // 接收侧拒收——>8MB 的手动发送会完整写出、再被对端 reader 判超限并拆掉
        // 整条会话。这里是所有发送方（手动/整组/auto）的唯一咽喉，超限在写出前拒绝。
        if payload.map_or(0, |p| p.len()) > LAN_MAX_PAYLOAD {
            return Err("payload 超限".to_string());
        }
        let header = serde_json::to_vec(msg).map_err(|e| e.to_string())?;
        if header.len() > LAN_MAX_PAYLOAD {
            return Err("帧头过大".to_string());
        }
        self.write
            .write_all(&(header.len() as u32).to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.write.write_all(&header).await.map_err(|e| e.to_string())?;
        if let Some(data) = payload {
            self.write
                .write_all(&(data.len() as u32).to_le_bytes())
                .await
                .map_err(|e| e.to_string())?;
            self.write.write_all(data).await.map_err(|e| e.to_string())?;
        }
        self.write.flush().await.map_err(|e| e.to_string())
    }
}

/// 读一帧并保留完整线格式字节（v0.9.2 A2 首帧路由用）。
///
/// registry 的入站分流需要先读首帧判断走向（`PairRequest` → 配对门；其余 →
/// 会话），而会话路径的 reader 必须能重新看到这一帧——本函数把已消费的线格式
/// 字节原样返回，供回放适配器（registry 的 `PrefixedFrame`）拼回流头部。
/// 帧格式与各项上限校验同 `FrameReader::read_message`；两条路径刻意分开，
/// 会话热路径（read_message）不必为回放字节做逐帧拷贝。
pub(crate) async fn read_message_with_raw<R: AsyncRead + Unpin>(
    read: &mut R,
) -> Result<(LanMessage, Vec<u8>), String> {
    let mut raw: Vec<u8> = Vec::new();
    let mut len_buf = [0u8; 4];
    read.read_exact(&mut len_buf).await.map_err(|e| e.to_string())?;
    raw.extend_from_slice(&len_buf);
    let header_len = u32::from_le_bytes(len_buf) as usize;
    if header_len > LAN_MAX_PAYLOAD {
        return Err("帧头超限".to_string());
    }
    let mut header = vec![0u8; header_len];
    read.read_exact(&mut header).await.map_err(|e| e.to_string())?;
    raw.extend_from_slice(&header);
    let msg: LanMessage = serde_json::from_slice(&header).map_err(|e| e.to_string())?;
    let has_payload = matches!(
        &msg,
        LanMessage::ClipPush { empty: false, .. } | LanMessage::ClipResponse { empty: false, .. }
    );
    if has_payload {
        let mut payload_len_buf = [0u8; 4];
        read.read_exact(&mut payload_len_buf)
            .await
            .map_err(|e| e.to_string())?;
        raw.extend_from_slice(&payload_len_buf);
        let payload_len = u32::from_le_bytes(payload_len_buf) as usize;
        if payload_len > LAN_MAX_PAYLOAD {
            return Err("payload 超限".to_string());
        }
        let mut payload = vec![0u8; payload_len];
        read.read_exact(&mut payload).await.map_err(|e| e.to_string())?;
        raw.extend_from_slice(&payload);
    }
    Ok((msg, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// 发送侧超限（v0.9.2 A4）：>LAN_MAX_PAYLOAD 的 payload 在写出前被拒，
    /// 对端读半甚至收不到任何字节（而不是收到半帧后被 reader 拆会话）。
    #[tokio::test]
    async fn writer_rejects_oversized_payload_before_writing() {
        let (a, mut b) = duplex(64 * 1024);
        let mut writer = FrameWriter::new(a);
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: None,
            category_color: None,
            display_name: None,
            auto: false,
            origin_node_id: None,
        };
        let oversized = vec![0u8; LAN_MAX_PAYLOAD + 1];
        let err = writer
            .write_message(&msg, Some(&oversized))
            .await
            .expect_err("超限 payload 必须在写出前被拒绝");
        assert_eq!(err, "payload 超限");
        // 拒绝发生在首字节写出之前：对端只观察到干净的 EOF
        drop(writer);
        let mut sink = Vec::new();
        use tokio::io::AsyncReadExt as _;
        let n = b.read_to_end(&mut sink).await.unwrap();
        assert_eq!(n, 0, "超限拒绝不得写出任何字节，实际：{sink:?}");
    }

    #[tokio::test]
    async fn frame_roundtrips_with_payload() {
        let (a, b) = duplex(64 * 1024);
        let mut writer = FrameWriter::new(a);
        let mut reader = FrameReader::new(b);
        let msg = LanMessage::ClipPush {
            clip_type: "text".into(),
            empty: false,
            category_name: Some("工作".into()),
            category_color: None,
            display_name: None,
            auto: false,
            origin_node_id: None,
        };
        writer.write_message(&msg, Some(b"hello")).await.unwrap();
        let (back, payload) = reader.read_message().await.unwrap();
        assert_eq!(back, msg);
        assert_eq!(payload.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn empty_push_has_no_payload() {
        let (a, b) = duplex(4096);
        let mut writer = FrameWriter::new(a);
        let mut reader = FrameReader::new(b);
        writer
            .write_message(&LanMessage::Ping, None)
            .await
            .unwrap();
        let (msg, payload) = reader.read_message().await.unwrap();
        assert_eq!(msg, LanMessage::Ping);
        assert_eq!(payload, None);
    }

    #[tokio::test]
    async fn oversize_payload_length_is_rejected() {
        // 手工构造「header empty=false + 恶意 payload_len 超限」的帧，走裸 duplex 喂给 reader
        let header = br#"{"kind":"clipPush","clip_type":"text","empty":false}"#;
        let mut raw = Vec::new();
        raw.extend_from_slice(&(header.len() as u32).to_le_bytes());
        raw.extend_from_slice(header);
        raw.extend_from_slice(&((LAN_MAX_PAYLOAD as u32) + 1).to_le_bytes());
        let (mut a, b) = duplex(64 * 1024);
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            a.write_all(&raw).await.unwrap();
        });
        let mut reader = FrameReader::new(b);
        assert!(reader.read_message().await.is_err());
    }
}
