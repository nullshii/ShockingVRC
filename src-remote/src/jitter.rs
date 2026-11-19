use std::collections::VecDeque;

use crate::protocol::RemoteFrame;

pub const DEFAULT_DELAY_MS: i64 = 100;
const CAPACITY: usize = 32;
const OFFSET_WINDOW_MS: i64 = 10_000;

#[derive(Debug)]
pub struct JitterBuffer {
    delay_ms: i64,
    queue: VecDeque<(i64, RemoteFrame)>,
    offset: Option<i64>,
    offset_since: i64,
    next_offset: Option<i64>,
    last_seq: Option<u64>,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_DELAY_MS)
    }
}

impl JitterBuffer {
    pub fn new(delay_ms: i64) -> Self {
        Self {
            delay_ms: delay_ms.clamp(0, 1000),
            queue: VecDeque::new(),
            offset: None,
            offset_since: 0,
            next_offset: None,
            last_seq: None,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.delay_ms);
    }

    pub fn push(&mut self, frame: RemoteFrame, now_ms: i64) -> bool {
        if self.last_seq.is_some_and(|s| frame.seq <= s) {
            return false;
        }
        self.last_seq = Some(frame.seq);
        let sample = now_ms - frame.ts as i64;
        self.offset = Some(self.offset.map_or(sample, |o| o.min(sample)));
        self.next_offset = Some(self.next_offset.map_or(sample, |o| o.min(sample)));
        if now_ms - self.offset_since > OFFSET_WINDOW_MS {
            self.offset = self.next_offset.take();
            self.offset_since = now_ms;
        }
        let due = frame.ts as i64 + self.offset.unwrap_or(sample) + self.delay_ms;
        self.queue.push_back((due, frame));
        while self.queue.len() > CAPACITY {
            self.queue.pop_front();
        }
        true
    }

    pub fn pop_due(&mut self, now_ms: i64) -> Option<RemoteFrame> {
        let mut out = None;
        while self.queue.front().is_some_and(|(due, _)| *due <= now_ms) {
            out = self.queue.pop_front().map(|(_, f)| f);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u64, ts: u64) -> RemoteFrame {
        RemoteFrame {
            seq,
            ts,
            slots: Vec::new(),
        }
    }

    #[test]
    fn frames_play_after_the_delay_in_order() {
        let mut j = JitterBuffer::new(100);
        assert!(j.push(frame(1, 1000), 5000));
        assert!(j.push(frame(2, 1025), 5040));
        assert_eq!(j.pop_due(5050), None, "not due yet");
        assert_eq!(j.pop_due(5100).map(|f| f.seq), Some(1));
        assert_eq!(j.pop_due(5125).map(|f| f.seq), Some(2), "jitter of 15 ms absorbed");
    }

    #[test]
    fn duplicates_and_stale_frames_are_dropped() {
        let mut j = JitterBuffer::new(100);
        assert!(j.push(frame(5, 1000), 0));
        assert!(!j.push(frame(5, 1000), 1));
        assert!(!j.push(frame(4, 990), 2));
        assert_eq!(j.len(), 1);
    }

    #[test]
    fn a_burst_skips_to_the_newest_due_frame() {
        let mut j = JitterBuffer::new(0);
        for k in 0..5 {
            j.push(frame(k + 1, k * 25), 0);
        }
        assert_eq!(j.pop_due(200).map(|f| f.seq), Some(5));
        assert!(j.is_empty());
    }
}
