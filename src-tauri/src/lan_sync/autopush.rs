//! 自动推送基座：类型过滤矩阵 + 回环抑制滑窗（spec §1/§3）。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::models::AutoSyncMode;

pub(crate) const RECENT_TTL: Duration = Duration::from_secs(60);
pub(crate) const RECENT_CAP: usize = 100;

/// 三态偏好 × 内容类型的自动推送准入矩阵（spec §1）。
/// text_only：文本四类放行，image/file 仅手动；all：全放行；off：全拒。
pub(crate) fn type_allowed(mode: AutoSyncMode, clip_type: &str) -> bool {
    match mode {
        AutoSyncMode::All => true,
        AutoSyncMode::Off => false,
        AutoSyncMode::TextOnly => matches!(clip_type, "text" | "link" | "color" | "html"),
    }
}

/// 最近接收哈希滑窗：发送侧查（防回推）、接收侧插（auto 路径专用）。
/// 惰性清理：每次访问先剔除过期项；重复插入刷新时间戳并挪到队尾。
pub(crate) struct RecentReceived {
    entries: Mutex<VecDeque<(String, Instant)>>,
    ttl: Duration,
}

impl RecentReceived {
    pub(crate) fn new() -> Self {
        Self::with_ttl(RECENT_TTL)
    }

    pub(crate) fn with_ttl(ttl: Duration) -> Self {
        Self { entries: Mutex::new(VecDeque::new()), ttl }
    }

    fn prune(&self, entries: &mut VecDeque<(String, Instant)>, now: Instant) {
        while let Some((_, at)) = entries.front() {
            if now.duration_since(*at) > self.ttl { entries.pop_front(); } else { break; }
        }
    }

    pub(crate) fn insert(&self, hash: &str) {
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("recent 锁中毒");
        self.prune(&mut entries, now);
        entries.retain(|(h, _)| h != hash);
        entries.push_back((hash.to_string(), now));
        while entries.len() > RECENT_CAP {
            entries.pop_front();
        }
    }

    pub(crate) fn contains(&self, hash: &str) -> bool {
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("recent 锁中毒");
        self.prune(&mut entries, now);
        entries.iter().any(|(h, _)| h == hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_allowed_matrix() {
        let text_types = ["text", "link", "color", "html"];
        for t in text_types {
            assert!(type_allowed(AutoSyncMode::TextOnly, t), "{t} 应自动推送");
            assert!(type_allowed(AutoSyncMode::All, t));
            assert!(!type_allowed(AutoSyncMode::Off, t));
        }
        for t in ["image", "file"] {
            assert!(!type_allowed(AutoSyncMode::TextOnly, t), "{t} 默认仅手动");
            assert!(type_allowed(AutoSyncMode::All, t));
            assert!(!type_allowed(AutoSyncMode::Off, t));
        }
    }

    #[test]
    fn recent_hit_and_miss() {
        let recent = RecentReceived::new();
        assert!(!recent.contains("h1"));
        recent.insert("h1");
        assert!(recent.contains("h1"));
        assert!(!recent.contains("h2"));
    }

    #[test]
    fn recent_expires() {
        let recent = RecentReceived::with_ttl(Duration::from_millis(20));
        recent.insert("h1");
        assert!(recent.contains("h1"));
        std::thread::sleep(Duration::from_millis(40));
        assert!(!recent.contains("h1"), "过期后不再命中");
    }

    #[test]
    fn recent_cap_and_refresh() {
        let recent = RecentReceived::new();
        for i in 0..RECENT_CAP { recent.insert(&format!("h{i}")); }
        recent.insert("h0"); // 刷新最老的，挪到队尾
        recent.insert("new"); // 超容量，挤出当前最老（h1）
        assert!(recent.contains("h0"), "刷新过的不会被挤出");
        assert!(!recent.contains("h1"), "最老的被挤出");
        assert!(recent.contains("new"));
    }
}
