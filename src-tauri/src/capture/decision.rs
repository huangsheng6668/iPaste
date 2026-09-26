//! 剪贴板捕获决策（Task 29）：把 watcher 轮询里的"这次内容要不要收、怎么收"抽成纯函数。
//!
//! 纯函数只依赖显式输入（前后采样的 change id、内容哈希、追加会话状态），
//! 不读全局、不碰 IO，因此可以用表驱动测试逐条锁死原本只能靠手测的判定分支。
//! 副作用（读剪贴板、落库、广播、跨设备推送）仍留在 clipboard.rs 的 watcher 里。

/// 捕获光标：上次见到的 change id 与内容哈希（对应 AppState 的两个 Mutex 状态）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureCursor {
    pub last_change_id: Option<u64>,
    pub last_hash: Option<String>,
}

/// 本轮轮询对剪贴板内容的处置决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureDecision {
    /// 采样到的 change id 在读取前后发生变化：内容仍在写入，本轮跳过（不更新光标）。
    UnstableChangeId,
    /// 与上次捕获内容重复：跳过（不更新光标）。
    AlreadyCaptured,
    /// 新内容，且处于追加复制会话中：合并进当前会话条目。
    MergeAppendCopy,
    /// 新内容：作为独立条目入库。
    Insert,
}

/// 本轮采样：change id 在读取前后各取一次；内容哈希来自实际读到的条目。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureSample<'a> {
    pub before_change_id: Option<u64>,
    pub after_change_id: Option<u64>,
    pub content_hash: &'a str,
    /// 该内容是否允许参与追加复制合并（图片与纯空白文本不允许）。
    pub mergeable: bool,
}

/// 追加复制会话状态（只需"开着"与"有会话"两个事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AppendState {
    pub is_enabled: bool,
    pub has_session: bool,
}

/// 判定本轮捕获；决定为捕获类（Insert / MergeAppendCopy）时按原实现时机更新光标。
///
/// 光标写入语义与原 `should_capture_clipboard_item` 完全一致：
/// - 采样的 change id 与上次相同**且**哈希相同 → 重复，不写；
/// - 否则写入本次 change id 与哈希后判定为可捕获；
/// - 平台不支持 change id（None）时退化为仅比较哈希。
pub(crate) fn decide_capture(
    cursor: &mut CaptureCursor,
    sample: CaptureSample<'_>,
    append: AppendState,
) -> CaptureDecision {
    if sample.before_change_id.is_some()
        && sample.after_change_id.is_some()
        && sample.before_change_id != sample.after_change_id
    {
        return CaptureDecision::UnstableChangeId;
    }

    let change_id = sample.after_change_id.or(sample.before_change_id);
    let same_hash = cursor.last_hash.as_deref() == Some(sample.content_hash);

    let fresh = match change_id {
        Some(id) => {
            if cursor.last_change_id == Some(id) && same_hash {
                false
            } else {
                cursor.last_change_id = Some(id);
                cursor.last_hash = Some(sample.content_hash.to_string());
                true
            }
        }
        None => {
            if same_hash {
                false
            } else {
                cursor.last_hash = Some(sample.content_hash.to_string());
                true
            }
        }
    };

    if !fresh {
        return CaptureDecision::AlreadyCaptured;
    }

    if sample.mergeable && append.is_enabled && append.has_session {
        return CaptureDecision::MergeAppendCopy;
    }

    CaptureDecision::Insert
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>(
        before: Option<u64>,
        after: Option<u64>,
        content_hash: &'a str,
        mergeable: bool,
    ) -> CaptureSample<'a> {
        CaptureSample {
            before_change_id: before,
            after_change_id: after,
            content_hash,
            mergeable,
        }
    }

    const NO_APPEND: AppendState = AppendState {
        is_enabled: false,
        has_session: false,
    };

    /// 不变量 1：读取前后 change id 变化 → 内容不稳定，跳过且不更新光标。
    #[test]
    fn skips_when_change_id_changes_during_read() {
        let mut cursor = CaptureCursor {
            last_change_id: Some(1),
            last_hash: Some("old".into()),
        };

        let decision = decide_capture(&mut cursor, sample(Some(7), Some(8), "new", true), NO_APPEND);

        assert_eq!(decision, CaptureDecision::UnstableChangeId);
        assert_eq!(cursor.last_change_id, Some(1));
        assert_eq!(cursor.last_hash.as_deref(), Some("old"));
    }

    /// 不变量 2：change id 与哈希都未变 → 重复捕获，跳过。
    #[test]
    fn skips_duplicate_when_change_id_and_hash_match() {
        let mut cursor = CaptureCursor {
            last_change_id: Some(5),
            last_hash: Some("same".into()),
        };

        let decision = decide_capture(&mut cursor, sample(Some(5), Some(5), "same", true), NO_APPEND);

        assert_eq!(decision, CaptureDecision::AlreadyCaptured);
        assert_eq!(cursor.last_change_id, Some(5));
    }

    /// 不变量 3：change id 相同但内容哈希不同 → 新内容（同一次复制序列内内容被改写）。
    #[test]
    fn inserts_when_change_id_same_but_hash_differs() {
        let mut cursor = CaptureCursor {
            last_change_id: Some(5),
            last_hash: Some("old".into()),
        };

        let decision = decide_capture(&mut cursor, sample(Some(5), Some(5), "new", true), NO_APPEND);

        assert_eq!(decision, CaptureDecision::Insert);
        assert_eq!(cursor.last_hash.as_deref(), Some("new"));
    }

    /// 不变量 4：平台无 change id（None）→ 退化为仅比较哈希。
    #[test]
    fn falls_back_to_hash_only_without_change_id() {
        let mut cursor = CaptureCursor::default();
        assert_eq!(
            decide_capture(&mut cursor, sample(None, None, "first", true), NO_APPEND),
            CaptureDecision::Insert
        );
        assert_eq!(cursor.last_hash.as_deref(), Some("first"));

        assert_eq!(
            decide_capture(&mut cursor, sample(None, None, "first", true), NO_APPEND),
            CaptureDecision::AlreadyCaptured
        );
        assert_eq!(
            decide_capture(&mut cursor, sample(None, None, "second", true), NO_APPEND),
            CaptureDecision::Insert
        );
    }

    /// 不变量 5：追加会话开启且内容可合并 → 合并而非新建条目。
    #[test]
    fn merges_into_append_session_when_enabled() {
        let mut cursor = CaptureCursor::default();
        let append = AppendState {
            is_enabled: true,
            has_session: true,
        };

        assert_eq!(
            decide_capture(&mut cursor, sample(Some(1), Some(1), "chunk", true), append),
            CaptureDecision::MergeAppendCopy
        );
    }

    /// 不变量 6：追加会话未开启 / 无会话 / 内容不可合并 → 一律新建独立条目。
    #[test]
    fn inserts_when_append_session_not_applicable() {
        let enabled_with_session = AppendState {
            is_enabled: true,
            has_session: true,
        };

        // 未开启
        let mut cursor = CaptureCursor::default();
        assert_eq!(
            decide_capture(&mut cursor, sample(Some(1), Some(1), "a", true), NO_APPEND),
            CaptureDecision::Insert
        );

        // 已开启但没有会话（未进入追加复制）
        let no_session = AppendState {
            is_enabled: true,
            has_session: false,
        };
        assert_eq!(
            decide_capture(&mut cursor, sample(Some(2), Some(2), "b", true), no_session),
            CaptureDecision::Insert
        );

        // 图片 / 空文本：不允许合并
        assert_eq!(
            decide_capture(&mut cursor, sample(Some(3), Some(3), "c", false), enabled_with_session),
            CaptureDecision::Insert
        );
    }

    /// 补充：重复内容优先于追加合并（重复判定在前，避免把同一段内容重复并入）。
    #[test]
    fn duplicate_wins_over_append_merge() {
        let mut cursor = CaptureCursor {
            last_change_id: Some(9),
            last_hash: Some("dup".into()),
        };
        let append = AppendState {
            is_enabled: true,
            has_session: true,
        };

        assert_eq!(
            decide_capture(&mut cursor, sample(Some(9), Some(9), "dup", true), append),
            CaptureDecision::AlreadyCaptured
        );
    }
}
