//! 剪贴板 watcher 循环（Task 31 从 clipboard.rs 搬出）。
//!
//! 本文件只负责"何时读、读到之后怎么处置"：循环节奏、change-id 前后采样、
//! 纯函数判定（capture/decision.rs）与端口编排（capture/ports.rs）。
//! 平台剪贴板读写、图片编解码、marker 记账等仍在 clipboard.rs。

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::Emitter;

use crate::capture::decision::{
    decide_capture, AppendState, CaptureCursor, CaptureDecision, CaptureSample,
};
use crate::capture::ports::{
    handle_captured, CaptureNotifier, CaptureSink, PeerPusher, PersistOutcome,
};
use crate::clipboard::{
    clipboard_change_id, read_current_clipboard, remember_current_clipboard_marker,
    write_clipboard_text,
};
use crate::events::{ClipboardCaptured, EVENT_CAPTURE_ERROR, EVENT_CLIPBOARD_CAPTURED};
use crate::models::{AppendCopyState, CapturedClipboardItem, ClipItem, ClipboardRead};
use crate::store::Store;
use crate::util::hash_text;

pub(crate) fn spawn_clipboard_watcher(
    app: tauri::AppHandle,
    store: Store,
    is_listening: Arc<Mutex<bool>>,
    append_copy_state: Arc<Mutex<AppendCopyState>>,
    last_clipboard_change_id: Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: Arc<Mutex<Option<String>>>,
    peers: Arc<dyn PeerPusher>,
) {
    let sink = StoreSink {
        store: store.clone(),
        append_copy_state: append_copy_state.clone(),
        last_clipboard_change_id: last_clipboard_change_id.clone(),
        last_clipboard_hash: last_clipboard_hash.clone(),
    };
    let notifier = TauriNotifier { app: app.clone() };
    thread::spawn(move || loop {
        let enabled = is_listening.lock().map(|value| *value).unwrap_or(false);
        if !enabled {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        let before_change_id = clipboard_change_id();

        match read_current_clipboard() {
            Ok(ClipboardRead::Item(item)) => {
                let after_change_id = clipboard_change_id();

                // 判定收成纯函数（capture/decision.rs）：只保留"读光标 → 判定 → 写回"三步，
                // 判定分支本身由表驱动测试覆盖（含 6 条不变量）。
                let mut cursor = CaptureCursor {
                    last_change_id: last_clipboard_change_id
                        .lock()
                        .map(|value| *value)
                        .unwrap_or(None),
                    last_hash: last_clipboard_hash
                        .lock()
                        .map(|value| value.clone())
                        .unwrap_or(None),
                };
                let append = append_copy_state
                    .lock()
                    .map(|state| AppendState {
                        is_enabled: state.is_enabled,
                        has_session: state.session_id.is_some(),
                    })
                    .unwrap_or(AppendState {
                        is_enabled: false,
                        has_session: false,
                    });
                let decision = decide_capture(
                    &mut cursor,
                    CaptureSample {
                        before_change_id,
                        after_change_id,
                        content_hash: &item.content_hash,
                        mergeable: item.clip_type != "image" && !item.text.trim().is_empty(),
                    },
                    append,
                );

                // 写回时机与原 should_capture_clipboard_item 一致：仅捕获类决定会改动光标值。
                if let Ok(mut last) = last_clipboard_change_id.lock() {
                    *last = cursor.last_change_id;
                }
                if let Ok(mut last) = last_clipboard_hash.lock() {
                    *last = cursor.last_hash;
                }

                match decision {
                    CaptureDecision::UnstableChangeId => {
                        thread::sleep(Duration::from_millis(120));
                        continue;
                    }
                    CaptureDecision::AlreadyCaptured => {
                        thread::sleep(Duration::from_millis(700));
                        continue;
                    }
                    // 合并与新建都继续走下面的落库流程：真正的合并判定与写回仍在
                    // capture_append_copy_item 内（含其锁失败即报错的原有语义）。
                    CaptureDecision::MergeAppendCopy | CaptureDecision::Insert => {}
                }

                // 落库 → 广播 → 按条件扇出：编排在 ports::handle_captured（可测），
                // 真实副作用经端口注入；跨设备推送由组合根提供的 PeerPusher 承担，
                // 捕获域不再直接依赖 lan_sync。
                handle_captured(item, &sink, &notifier, peers.as_ref());
            }
            Ok(ClipboardRead::Empty) => {}
            Ok(ClipboardRead::Occupied) => {}
            Err(error) => {
                let _ = app.emit(EVENT_CAPTURE_ERROR, error);
            }
        }

        thread::sleep(Duration::from_millis(700));
    });
}

/// 落库端口的真实实现：追加复制会话优先合并，否则插入新条目。
struct StoreSink {
    store: Store,
    append_copy_state: Arc<Mutex<AppendCopyState>>,
    last_clipboard_change_id: Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: Arc<Mutex<Option<String>>>,
}

impl CaptureSink for StoreSink {
    fn persist(&self, item: CapturedClipboardItem) -> Result<Option<PersistOutcome>, String> {
        // 追加复制路径标记：capture_append_copy_item 返回 Some 表示本次捕获属于
        // 追加会话（首条 INSERT 或后续合并 UPDATE），扇出据此整体跳过。
        let mut from_append_copy = false;
        let persisted = capture_append_copy_item(
            &self.store,
            &self.append_copy_state,
            &self.last_clipboard_change_id,
            &self.last_clipboard_hash,
            &item,
        )
        .and_then(|append_copy_clip| match append_copy_clip {
            Some(result) => {
                from_append_copy = true;
                Ok(Some(result))
            }
            None => self.store.insert_captured_item(item),
        })?;

        Ok(persisted.map(|(clip, clip_total_count, was_inserted)| PersistOutcome {
            clip,
            clip_total_count,
            was_inserted,
            from_append_copy,
        }))
    }
}

/// 广播端口的真实实现：沿用原 watcher 的 fire-and-forget emit。
struct TauriNotifier {
    app: tauri::AppHandle,
}

impl CaptureNotifier for TauriNotifier {
    fn captured(&self, event: ClipboardCaptured) {
        let _ = self.app.emit(EVENT_CLIPBOARD_CAPTURED, event);
    }

    fn error(&self, error: String) {
        let _ = self.app.emit(EVENT_CAPTURE_ERROR, error);
    }
}

fn capture_append_copy_item(
    store: &Store,
    append_copy_state: &Arc<Mutex<AppendCopyState>>,
    last_clipboard_change_id: &Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: &Arc<Mutex<Option<String>>>,
    item: &CapturedClipboardItem,
) -> Result<Option<(ClipItem, usize, bool)>, String> {
    if item.clip_type == "image" || item.text.trim().is_empty() {
        return Ok(None);
    }

    let (clip_id, session_id, next_text) = {
        let append_copy = append_copy_state
            .lock()
            .map_err(|error| error.to_string())?;

        if !append_copy.is_enabled {
            return Ok(None);
        }

        let Some(session_id) = append_copy.session_id.clone() else {
            return Ok(None);
        };

        (
            append_copy.clip_id.clone(),
            session_id,
            append_copy_text(&append_copy.text, &item.text),
        )
    };

    let (clip, clip_total_count, was_inserted) =
        store.upsert_append_copy_item(clip_id, &session_id, next_text.clone())?;
    write_clipboard_text(&next_text)?;
    remember_current_clipboard_marker(
        last_clipboard_change_id,
        last_clipboard_hash,
        Some(hash_text(&next_text)),
    );

    if let Ok(mut append_copy) = append_copy_state.lock() {
        if append_copy.is_enabled && append_copy.session_id.as_deref() == Some(session_id.as_str())
        {
            append_copy.clip_id = Some(clip.id.clone());
            append_copy.text = next_text;
        }
    }

    Ok(Some((clip, clip_total_count, was_inserted)))
}

fn append_copy_text(current: &str, next: &str) -> String {
    let next = next.trim();
    let current = current.trim_end_matches(|value| value == '\r' || value == '\n');

    if current.is_empty() {
        next.to_string()
    } else {
        format!("{current}\n{next}")
    }
}
