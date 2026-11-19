use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const FREE_FAILURES: u32 = 3;
pub const MAX_LOCKOUT: Duration = Duration::from_secs(900);
const FORGET_AFTER: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy)]
struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    last: Instant,
}

#[derive(Debug, Default)]
pub struct FailLimiter {
    entries: HashMap<String, Entry>,
}

impl FailLimiter {
    pub fn check(&self, key: &str, now: Instant) -> Result<(), Duration> {
        match self.entries.get(key).and_then(|e| e.locked_until) {
            Some(until) if until > now => Err(until - now),
            _ => Ok(()),
        }
    }

    pub fn fail(&mut self, key: &str, now: Instant) -> Option<Duration> {
        self.entries
            .retain(|_, e| now.saturating_duration_since(e.last) < FORGET_AFTER);
        let e = self.entries.entry(key.to_string()).or_insert(Entry {
            failures: 0,
            locked_until: None,
            last: now,
        });
        e.failures = e.failures.saturating_add(1);
        e.last = now;
        if e.failures < FREE_FAILURES {
            return None;
        }
        let exp = (e.failures - FREE_FAILURES).min(20);
        let wait = Duration::from_secs(1u64 << exp).min(MAX_LOCKOUT);
        e.locked_until = Some(now + wait);
        Some(wait)
    }

    pub fn succeed(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockout_grows_exponentially_and_caps() {
        let mut l = FailLimiter::default();
        let t = Instant::now();
        assert_eq!(l.fail("ip", t), None);
        assert_eq!(l.fail("ip", t), None);
        assert_eq!(l.fail("ip", t), Some(Duration::from_secs(1)));
        assert_eq!(l.fail("ip", t), Some(Duration::from_secs(2)));
        assert_eq!(l.fail("ip", t), Some(Duration::from_secs(4)));
        assert!(l.check("ip", t).is_err());
        assert!(l.check("ip", t + Duration::from_secs(5)).is_ok());
        assert!(l.check("other", t).is_ok());
        for _ in 0..30 {
            l.fail("ip", t);
        }
        assert_eq!(l.fail("ip", t), Some(MAX_LOCKOUT));
        l.succeed("ip");
        assert!(l.check("ip", t).is_ok());
    }
}
