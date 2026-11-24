//! Stream tokens the phone acknowledges output against and resumes from: credit window, resume points, one-shot claims.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const WINDOW: u64 = 512 * 1024;
pub const RETENTION: Duration = Duration::from_secs(120);
pub const PER_DEVICE: usize = 8;
const MAX_POINTS: usize = 1024;

/// Units sent and not yet acknowledged; the stream pauses its attach cursor once the window is full.
#[derive(Debug, Default)]
pub struct Credit {
    in_flight: AtomicU64,
}

impl Credit {
    pub fn sent(&self, units: u64) {
        self.in_flight.fetch_add(units, Ordering::SeqCst);
    }

    pub fn ack(&self, units: u64) {
        let _ = self.in_flight.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| Some(n.saturating_sub(units)));
    }

    pub fn can_send(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) < WINDOW
    }
}

/// Counter-based and bound to the device that opened the stream, not a secret.
#[derive(Debug, Clone)]
pub struct Token {
    pub id: String,
    pub credit: Arc<Credit>,
}

#[derive(Debug)]
struct Entry {
    token: String,
    device: String,
    target: (String, String),
    credit: Arc<Credit>,
    /// (cumulative UTF-16 units sent, rca byte offset after them)
    points: Vec<(u64, u64)>,
    ended: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct Resumes {
    next: u64,
    entries: Vec<Entry>,
}

impl Resumes {
    pub fn register(&mut self, connection: &str, device: &str, workspace: &str, terminal: &str) -> Token {
        self.next += 1;
        let id = format!("ts-{connection}-{}", self.next);
        let credit = Arc::new(Credit::default());
        self.entries.push(Entry {
            token: id.clone(),
            device: device.to_string(),
            target: (workspace.to_string(), terminal.to_string()),
            credit: Arc::clone(&credit),
            points: Vec::new(),
            ended: None,
        });
        Token { id, credit }
    }

    pub fn record(&mut self, token: &str, units: u64, offset: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.token == token) {
            entry.points.push((units, offset));
            if entry.points.len() > MAX_POINTS {
                entry.points.remove(0);
            }
        }
    }

    /// False for an unknown, ended or other-device token.
    pub fn ack(&mut self, token: &str, device: &str, units: u64) -> bool {
        match self.entries.iter().find(|e| e.token == token && e.ended.is_none() && e.device == device) {
            Some(entry) => {
                entry.credit.ack(units);
                true
            }
            None => false,
        }
    }

    pub fn release(&mut self, token: &str, now: Instant) {
        let Some(entry) = self.entries.iter_mut().find(|e| e.token == token && e.ended.is_none()) else { return };
        entry.ended = Some(now);
        let device = entry.device.clone();
        self.expire(now);
        let kept: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].ended.is_some() && self.entries[i].device == device).collect();
        for &i in kept.iter().take(kept.len().saturating_sub(PER_DEVICE)).rev() {
            self.entries.remove(i);
        }
    }

    /// Spends an ended token whatever the answer; None sends the phone a snapshot instead.
    pub fn claim(&mut self, token: &str, device: &str, workspace: &str, terminal: &str, units: u64, now: Instant) -> Option<u64> {
        self.expire(now);
        let at = self.entries.iter().position(|e| e.token == token && e.ended.is_some())?;
        let entry = self.entries.remove(at);
        if entry.device != device || entry.target.0 != workspace || entry.target.1 != terminal {
            return None;
        }
        entry.points.iter().rev().find(|(u, _)| *u == units).map(|(_, offset)| *offset)
    }

    fn expire(&mut self, now: Instant) {
        self.entries.retain(|e| e.ended.is_none_or(|at| now.saturating_duration_since(at) <= RETENTION));
    }
}

/// Process-wide, like the real host's token maps; tokens carry the connection id so parallel tests never collide.
pub fn registry() -> &'static Mutex<Resumes> {
    static REGISTRY: OnceLock<Mutex<Resumes>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(r: &mut Resumes) -> Token {
        r.register("c1", "phone", "main", "t1")
    }

    #[test]
    fn given_a_released_stream_when_claimed_twice_then_it_resumes_once() {
        let mut r = Resumes::default();
        let now = Instant::now();
        let t = live(&mut r);
        r.record(&t.id, 10, 100);
        r.record(&t.id, 25, 240);
        r.release(&t.id, now);
        assert_eq!(r.claim(&t.id, "phone", "main", "t1", 25, now), Some(240));
        assert_eq!(r.claim(&t.id, "phone", "main", "t1", 25, now), None);
    }

    #[test]
    fn given_a_point_that_was_never_sent_when_claimed_then_it_is_declined() {
        let mut r = Resumes::default();
        let now = Instant::now();
        let t = live(&mut r);
        r.record(&t.id, 10, 100);
        r.release(&t.id, now);
        assert_eq!(r.claim(&t.id, "phone", "main", "t1", 11, now), None);
    }

    #[test]
    fn given_another_device_or_terminal_when_claimed_then_none_and_the_token_is_spent() {
        let mut r = Resumes::default();
        let now = Instant::now();
        for (device, terminal) in [("other", "t1"), ("phone", "t2")] {
            let t = live(&mut r);
            r.record(&t.id, 5, 50);
            r.release(&t.id, now);
            assert_eq!(r.claim(&t.id, device, "main", terminal, 5, now), None);
            assert_eq!(r.claim(&t.id, "phone", "main", "t1", 5, now), None);
        }
    }

    #[test]
    fn given_nine_retained_streams_of_a_device_when_the_ninth_ends_then_the_first_is_evicted() {
        let mut r = Resumes::default();
        let now = Instant::now();
        let tokens: Vec<Token> = (0..9).map(|_| live(&mut r)).collect();
        for t in &tokens {
            r.record(&t.id, 1, 1);
            r.release(&t.id, now);
        }
        assert_eq!(r.claim(&tokens[0].id, "phone", "main", "t1", 1, now), None);
        assert_eq!(r.claim(&tokens[1].id, "phone", "main", "t1", 1, now), Some(1));
        assert_eq!(r.claim(&tokens[8].id, "phone", "main", "t1", 1, now), Some(1));
    }

    #[test]
    fn given_other_devices_streams_when_one_device_overflows_then_they_are_kept() {
        let mut r = Resumes::default();
        let now = Instant::now();
        let other = r.register("c1", "tablet", "main", "t1");
        r.record(&other.id, 1, 1);
        r.release(&other.id, now);
        for _ in 0..9 {
            let t = live(&mut r);
            r.release(&t.id, now);
        }
        assert_eq!(r.claim(&other.id, "tablet", "main", "t1", 1, now), Some(1));
    }

    #[test]
    fn given_an_entry_older_than_two_minutes_when_claimed_then_none() {
        let mut r = Resumes::default();
        let now = Instant::now();
        let fresh = live(&mut r);
        let stale = live(&mut r);
        for t in [&fresh, &stale] {
            r.record(&t.id, 1, 1);
            r.release(&t.id, now);
        }
        assert_eq!(r.claim(&fresh.id, "phone", "main", "t1", 1, now + RETENTION), Some(1));
        assert_eq!(r.claim(&stale.id, "phone", "main", "t1", 1, now + RETENTION + Duration::from_secs(1)), None);
    }

    #[test]
    fn given_a_live_token_when_acked_then_credited_and_an_ended_or_unknown_one_is_not() {
        let mut r = Resumes::default();
        let t = live(&mut r);
        assert!(r.ack(&t.id, "phone", 4));
        assert!(!r.ack(&t.id, "other", 4));
        assert!(!r.ack("ts-none-1", "phone", 4));
        r.release(&t.id, Instant::now());
        assert!(!r.ack(&t.id, "phone", 4));
    }

    #[test]
    fn given_a_full_window_when_acked_then_sending_reopens() {
        let mut r = Resumes::default();
        let t = live(&mut r);
        t.credit.sent(WINDOW - 1);
        assert!(t.credit.can_send());
        t.credit.sent(1);
        assert!(!t.credit.can_send());
        assert!(r.ack(&t.id, "phone", 100));
        assert!(t.credit.can_send());
    }

    #[test]
    fn given_two_streams_when_registered_then_tokens_differ_and_name_the_connection() {
        let mut r = Resumes::default();
        let a = live(&mut r);
        let b = live(&mut r);
        assert_ne!(a.id, b.id);
        assert!(a.id.starts_with("ts-c1-"));
    }
}
