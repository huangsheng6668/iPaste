//! 捕获副作用端口（Task 30）：把 watcher 里的落库 / 广播 / 跨设备扇出收成三个 trait，
//! 编排本身（顺序与"何时不推送"）由 `handle_captured` 表达，并可用假实现直接断言。
//!
//! 端口化的直接收益：捕获域不再 `use crate::lan_sync`——真实扇出实现由组合根
//! （lib.rs）注入；同时把"persist → 广播 → 扇出"的调用顺序变成可测契约。

use crate::events::ClipboardCaptured;
use crate::models::{CapturedClipboardItem, ClipItem};

/// 一次落库的结果。
#[derive(Debug, Clone)]
pub(crate) struct PersistOutcome {
    pub clip: ClipItem,
    pub clip_total_count: usize,
    pub was_inserted: bool,
    /// 本次捕获属于追加复制会话（首条 INSERT 或后续合并 UPDATE）：
    /// 追加会话期间不触发跨设备扇出（对端不应看到半合并内容）。
    pub from_append_copy: bool,
}

/// 落库端口：返回 Ok(None) 表示本次捕获未产生条目。
pub(crate) trait CaptureSink {
    fn persist(&self, item: CapturedClipboardItem) -> Result<Option<PersistOutcome>, String>;
}

/// 事件广播端口。
pub(crate) trait CaptureNotifier {
    fn captured(&self, event: ClipboardCaptured);
    fn error(&self, error: String);
}

/// 跨设备扇出端口（真实实现由组合根注入，捕获域不依赖 lan_sync）。
/// 需要 Send + Sync：watcher 在独立线程持有 Arc<dyn PeerPusher>。
pub(crate) trait PeerPusher: Send + Sync {
    fn fan_out(&self, clip: &ClipItem);
}

/// 捕获落库后的统一编排：持久化 → 广播 → 按条件扇出。
///
/// 顺序与条件与原 watcher 内联实现一致：
/// 1. `persist` 出错 → 只广播错误，不广播条目、不扇出；
/// 2. `Ok(None)` → 什么都不做；
/// 3. `Ok(Some)` → 先广播 `ClipboardCaptured`，再在
///    `was_inserted && !from_append_copy` 时扇出（非新建、追加会话都不推送）。
pub(crate) fn handle_captured(
    item: CapturedClipboardItem,
    sink: &dyn CaptureSink,
    notifier: &dyn CaptureNotifier,
    peers: &dyn PeerPusher,
) {
    match sink.persist(item) {
        Ok(Some(outcome)) => {
            notifier.captured(ClipboardCaptured {
                clip: outcome.clip.clone(),
                clip_total_count: outcome.clip_total_count,
                was_inserted: outcome.was_inserted,
            });
            if outcome.was_inserted && !outcome.from_append_copy {
                peers.fan_out(&outcome.clip);
            }
        }
        Ok(None) => {}
        Err(error) => notifier.error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn clip(id: &str) -> ClipItem {
        ClipItem {
            id: id.to_string(),
            clip_type: "text".to_string(),
            content_hash: id.to_string(),
            display_name: None,
            preview_text: id.to_string(),
            text: id.to_string(),
            source_app: None,
            last_captured_at: "2026-01-01T00:00:00Z".to_string(),
            favorite_count: 0,
            is_pinned: false,
        }
    }

    fn item(id: &str) -> CapturedClipboardItem {
        CapturedClipboardItem {
            clip_type: "text".to_string(),
            content_hash: id.to_string(),
            preview_text: id.to_string(),
            text: id.to_string(),
            image_bytes: None,
            display_name: None,
        }
    }

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
    }

    impl Recorder {
        fn record(&self, entry: impl Into<String>) {
            self.calls.lock().unwrap().push(entry.into());
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    struct FakeSink {
        recorder: std::sync::Arc<Recorder>,
        result: Mutex<Option<Result<Option<PersistOutcome>, String>>>,
    }

    impl CaptureSink for FakeSink {
        fn persist(&self, _item: CapturedClipboardItem) -> Result<Option<PersistOutcome>, String> {
            self.recorder.record("persist");
            self.result.lock().unwrap().take().unwrap_or(Ok(None))
        }
    }

    struct FakeNotifier {
        recorder: std::sync::Arc<Recorder>,
    }

    impl CaptureNotifier for FakeNotifier {
        fn captured(&self, event: ClipboardCaptured) {
            self.recorder.record(format!("captured:{}", event.clip.id));
        }
        fn error(&self, error: String) {
            self.recorder.record(format!("error:{error}"));
        }
    }

    struct FakePeers {
        recorder: std::sync::Arc<Recorder>,
    }

    impl PeerPusher for FakePeers {
        fn fan_out(&self, clip: &ClipItem) {
            self.recorder.record(format!("fan_out:{}", clip.id));
        }
    }

    fn setup(result: Result<Option<PersistOutcome>, &str>) -> (std::sync::Arc<Recorder>, FakeSink, FakeNotifier, FakePeers) {
        let recorder = std::sync::Arc::new(Recorder::default());
        let sink = FakeSink {
            recorder: recorder.clone(),
            result: Mutex::new(Some(match result {
                Ok(value) => Ok(value),
                Err(message) => Err(message.to_string()),
            })),
        };
        let notifier = FakeNotifier {
            recorder: recorder.clone(),
        };
        let peers = FakePeers {
            recorder: recorder.clone(),
        };
        (recorder, sink, notifier, peers)
    }

    fn outcome(from_append_copy: bool, was_inserted: bool) -> PersistOutcome {
        PersistOutcome {
            clip: clip("c1"),
            clip_total_count: 7,
            was_inserted,
            from_append_copy,
        }
    }

    /// 不变量：正常新建 → 顺序必须是 persist → 广播 → 扇出。
    #[test]
    fn inserts_then_notifies_then_fans_out() {
        let (recorder, sink, notifier, peers) = setup(Ok(Some(outcome(false, true))));

        handle_captured(item("c1"), &sink, &notifier, &peers);

        assert_eq!(recorder.calls(), vec!["persist", "captured:c1", "fan_out:c1"]);
    }

    /// 不变量：非新建（重复内容被合并进已有条目）→ 广播但不扇出。
    #[test]
    fn skips_fan_out_when_not_inserted() {
        let (recorder, sink, notifier, peers) = setup(Ok(Some(outcome(false, false))));

        handle_captured(item("c1"), &sink, &notifier, &peers);

        assert_eq!(recorder.calls(), vec!["persist", "captured:c1"]);
    }

    /// 不变量：追加复制会话（含首条 INSERT）→ 一律不扇出，避免对端收到半合并内容。
    #[test]
    fn skips_fan_out_for_append_session_even_when_inserted() {
        let (recorder, sink, notifier, peers) = setup(Ok(Some(outcome(true, true))));

        handle_captured(item("c1"), &sink, &notifier, &peers);

        assert_eq!(recorder.calls(), vec!["persist", "captured:c1"]);
    }

    /// 不变量：落库失败 → 只广播错误，既不广播条目也不扇出。
    #[test]
    fn persist_error_only_notifies_error() {
        let (recorder, sink, notifier, peers) = setup(Err("db locked"));

        handle_captured(item("c1"), &sink, &notifier, &peers);

        assert_eq!(recorder.calls(), vec!["persist", "error:db locked"]);
    }

    /// 不变量：未产生条目（Ok(None)）→ 静默，无任何下游调用。
    #[test]
    fn no_entry_is_silent() {
        let (recorder, sink, notifier, peers) = setup(Ok(None));

        handle_captured(item("c1"), &sink, &notifier, &peers);

        assert_eq!(recorder.calls(), vec!["persist"]);
    }
}
