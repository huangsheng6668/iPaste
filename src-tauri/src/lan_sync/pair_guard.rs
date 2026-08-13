//! 配对防爆破：按来源 IP 记录握手失败次数，指数退避 + 暂时封禁。

use std::collections::HashMap;
use std::net::IpAddr;
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
    states: Mutex<HashMap<IpAddr, AttemptState>>,
}

impl PairGuard {
    pub(crate) fn new() -> Self {
        Self { states: Mutex::new(HashMap::new()) }
    }

    /// 该 IP 当前是否处于封禁期。
    pub(crate) fn is_blocked(&self, ip: IpAddr, now: Instant) -> bool {
        let states = self.states.lock().expect("pair guard poisoned");
        states
            .get(&ip)
            .and_then(|s| s.blocked_until)
            .map(|until| now < until)
            .unwrap_or(false)
    }

    /// 记录一次失败，返回调用方应等待的退避时长。达到封禁阈值时内部置封禁并返回 0
    /// （调用方应直接拒绝连接）。
    pub(crate) fn record_failure(&self, ip: IpAddr, now: Instant) -> Duration {
        let mut states = self.states.lock().expect("pair guard poisoned");
        let state = states.entry(ip).or_insert_with(|| AttemptState {
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

    /// 配对成功，清除该 IP 的失败记录。
    pub(crate) fn record_success(&self, ip: IpAddr) {
        self.states.lock().expect("pair guard poisoned").remove(&ip);
    }

    /// 清理过期条目：超过 `STALE_AFTER` 无活动且从未/不再封禁的 IP。
    /// 注意：曾被封禁（`blocked_until` 非空）的条目即便活动时间久也保留，
    /// 需经 `record_success` 或封禁到期由调用方清理。
    pub(crate) fn prune(&self, now: Instant) {
        self.states.lock().expect("pair guard poisoned").retain(|_, s| {
            s.blocked_until.is_some() || now.duration_since(s.last_activity) < STALE_AFTER
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip() -> IpAddr { "192.168.1.50".parse().unwrap() }
    fn t0() -> Instant { Instant::now() }

    #[test]
    fn no_delay_before_threshold_and_block_after_many_failures() {
        let g = PairGuard::new();
        let t = t0();
        for i in 0..4 {
            assert_eq!(g.record_failure(ip(), t), Duration::ZERO, "failure {i}");
        }
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(2)); // 第 5 次
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(4)); // 第 6 次
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(8)); // 第 7 次
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(16)); // 第 8 次
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(32)); // 第 9 次
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(32)); // 封顶
        for _ in 10..20 {
            let _ = g.record_failure(ip(), t);
        }
        // 第 20 次起封禁，返回 0 让调用方直接拒绝
        assert_eq!(g.record_failure(ip(), t), Duration::ZERO);
        assert!(g.is_blocked(ip(), t));
        // 封禁期内仍 blocked；到期解除
        assert!(g.is_blocked(ip(), t + BLOCK_DURATION - Duration::from_secs(1)));
        assert!(!g.is_blocked(ip(), t + BLOCK_DURATION + Duration::from_secs(1)));
    }

    #[test]
    fn success_clears_failures() {
        let g = PairGuard::new();
        let t = t0();
        for _ in 0..5 {
            let _ = g.record_failure(ip(), t);
        }
        assert_eq!(g.record_failure(ip(), t), Duration::from_secs(4));
        g.record_success(ip());
        assert_eq!(g.record_failure(ip(), t), Duration::ZERO); // 计数已清零
        assert!(!g.is_blocked(ip(), t));
    }

    #[test]
    fn prune_removes_stale_entries_but_keeps_blocked() {
        let g = PairGuard::new();
        let t = t0();
        for _ in 0..20 {
            let _ = g.record_failure(ip(), t);
        }
        g.prune(t + STALE_AFTER + Duration::from_secs(1));
        // 被封禁的条目即使活动时间久也必须保留
        assert!(g.is_blocked(ip(), t));
        // 未封禁的过期条目被清除
        let ip2: IpAddr = "192.168.1.60".parse().unwrap();
        let _ = g.record_failure(ip2, t);
        g.prune(t + STALE_AFTER + Duration::from_secs(1));
        assert_eq!(g.record_failure(ip2, t + STALE_AFTER + Duration::from_secs(2)), Duration::ZERO);
    }
}
