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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

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
