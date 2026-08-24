//! 配对防爆破：按来源 node_id（EndpointId hex）记录握手失败次数，指数退避 + 暂时封禁。
//!
//! v4 以来源 IP 为 key（TCP 连接的 peer addr）；v5 的 iroh 连接没有稳定的
//! 「来源 IP」语义（可能经中继），改用对端 EndpointId hex——语义不变：同一来源
//! 反复配对失败即退避/封禁。

// Task 7 的配对门（DeviceLinkRegistry）接线前暂无生产调用方，测试先行为其
// 锁定退避/封禁语义。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 失败 5 次起开始退避（2s 起指数增长封顶 32s）；20 次后封禁 10 分钟。
const DELAY_START_FAILURES: u32 = 5;
const BLOCK_FAILURES: u32 = 20;
const BLOCK_DURATION: Duration = Duration::from_secs(600);
const STALE_AFTER: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct AttemptState {
    failures: u32,
    blocked_until: Option<Instant>,
    last_activity: Instant,
}

pub(crate) struct PairGuard {
    states: Mutex<HashMap<String, AttemptState>>,
}

impl PairGuard {
    pub(crate) fn new() -> Self {
        Self { states: Mutex::new(HashMap::new()) }
    }

    /// 该 node_id 当前是否处于封禁期。
    pub(crate) fn is_blocked(&self, node_id: &str, now: Instant) -> bool {
        let states = self.states.lock().expect("pair guard poisoned");
        states
            .get(node_id)
            .and_then(|s| s.blocked_until)
            .map(|until| now < until)
            .unwrap_or(false)
    }

    /// 记录一次失败，返回调用方应等待的退避时长。达到封禁阈值时内部置封禁并返回 0
    /// （调用方应直接拒绝连接）。
    pub(crate) fn record_failure(&self, node_id: &str, now: Instant) -> Duration {
        let mut states = self.states.lock().expect("pair guard poisoned");
        let state = states.entry(node_id.to_string()).or_insert_with(|| AttemptState {
            failures: 0,
            blocked_until: None,
            last_activity: now,
        });
        state.failures += 1;
        state.last_activity = now;
        if state.failures >= BLOCK_FAILURES {
            state.blocked_until = Some(now + BLOCK_DURATION);
            return Duration::ZERO;
        }
        if state.failures >= DELAY_START_FAILURES {
            let exp = (state.failures - DELAY_START_FAILURES).min(4); // 2^1..2^5 秒
            return Duration::from_secs(1 << (exp + 1));
        }
        Duration::ZERO
    }

    /// 配对成功，清除该 node_id 的失败记录。
    pub(crate) fn record_success(&self, node_id: &str) {
        self.states.lock().expect("pair guard poisoned").remove(node_id);
    }

    /// 清理过期条目：仅保留「封禁仍在生效」或「`STALE_AFTER` 内有活动」的 node_id。
    /// 封禁到期且无活动的条目会被清除——否则每个曾被封禁过的 node_id 都会
    /// 永久留在内存里（攻击者可借此线性堆积条目）。持续攻击者的
    /// `last_activity` 会不断刷新，其条目不会因此被提前清理。
    pub(crate) fn prune(&self, now: Instant) {
        self.states.lock().expect("pair guard poisoned").retain(|_, s| {
            s.blocked_until.map(|until| now < until).unwrap_or(false)
                || now.duration_since(s.last_activity) < STALE_AFTER
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> &'static str { "a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8" }
    fn t0() -> Instant { Instant::now() }

    #[test]
    fn no_delay_before_threshold_and_block_after_many_failures() {
        let g = PairGuard::new();
        let t = t0();
        for i in 0..4 {
            assert_eq!(g.record_failure(node(), t), Duration::ZERO, "failure {i}");
        }
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(2)); // 第 5 次
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(4)); // 第 6 次
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(8)); // 第 7 次
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(16)); // 第 8 次
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(32)); // 第 9 次
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(32)); // 封顶
        for _ in 10..20 {
            let _ = g.record_failure(node(), t);
        }
        // 第 20 次起封禁，返回 0 让调用方直接拒绝
        assert_eq!(g.record_failure(node(), t), Duration::ZERO);
        assert!(g.is_blocked(node(), t));
        // 封禁期内仍 blocked；到期解除
        assert!(g.is_blocked(node(), t + BLOCK_DURATION - Duration::from_secs(1)));
        assert!(!g.is_blocked(node(), t + BLOCK_DURATION + Duration::from_secs(1)));
    }

    #[test]
    fn success_clears_failures() {
        let g = PairGuard::new();
        let t = t0();
        for _ in 0..5 {
            let _ = g.record_failure(node(), t);
        }
        assert_eq!(g.record_failure(node(), t), Duration::from_secs(4));
        g.record_success(node());
        assert_eq!(g.record_failure(node(), t), Duration::ZERO); // 计数已清零
        assert!(!g.is_blocked(node(), t));
    }

    #[test]
    fn prune_removes_stale_entries_but_keeps_blocked() {
        let g = PairGuard::new();
        let t = t0();
        for _ in 0..20 {
            let _ = g.record_failure(node(), t);
        }
        g.prune(t + Duration::from_secs(590));
        // 封禁期内（blocked_until 未到）的条目必须保留
        assert!(g.is_blocked(node(), t));
        // 未封禁的过期条目被清除
        let node2 = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let _ = g.record_failure(node2, t);
        g.prune(t + STALE_AFTER + Duration::from_secs(1));
        assert_eq!(g.record_failure(node2, t + STALE_AFTER + Duration::from_secs(2)), Duration::ZERO);
    }

    #[test]
    fn prune_drops_entries_with_expired_block() {
        let g = PairGuard::new();
        let t = t0();
        for _ in 0..20 {
            let _ = g.record_failure(node(), t);
        }
        // 封禁到期 + 超过 STALE_AFTER 无活动：条目应被清理，
        // 否则每个曾被封禁过的 node_id 都会永久占用内存（慢性泄漏）
        let later = t + BLOCK_DURATION + STALE_AFTER + Duration::from_secs(1);
        g.prune(later);
        // 清理后重新失败应从零计数：单次失败不应再次触发封禁
        let _ = g.record_failure(node(), later);
        assert!(!g.is_blocked(node(), later));
    }
}
