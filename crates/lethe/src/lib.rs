//! lethe — the excretion.
//!
//! A memory store where forgetting is a first-class, *designed* faculty:
//!
//! - Every memory is born with a TTL. Immortality is opt-in, never default.
//! - Salience decays exponentially unless a memory is actually recalled.
//!   Use it or lose it — Nietzsche's active forgetting as a half-life.
//! - [`Lethe::sweep`] is the excretory organ: expired and faded memories
//!   are released on schedule, not on panic.
//! - [`Lethe::forget`] is erasure on demand, and it returns a receipt:
//!   deletion you can point to. Right-to-be-forgotten as an API call,
//!   not a fire drill.
//!
//! Time is injected (`now: Instant`) everywhere, so the whole lifecycle is
//! testable without sleeping.

use std::fmt;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: u64,
    pub subject: String,
    pub content: String,
    pub born: Instant,
    pub last_touch: Instant,
    pub ttl: Duration,
    salience: f64,
}

impl Memory {
    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.born) >= self.ttl
    }
}

/// What a sweep released, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swept {
    pub expired: usize,
    pub faded: usize,
}

/// Proof of deletion. The hash covers the ids and contents of everything
/// erased, so the receipt commits to *what* was forgotten without
/// retaining it — a tombstone, not a backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasureReceipt {
    pub erased: usize,
    pub receipt: String,
}

impl fmt::Display for ErasureReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} memories released — {}", self.erased, self.receipt)
    }
}

pub struct Lethe {
    entries: Vec<Memory>,
    next_id: u64,
    /// Salience half-life: after this long untouched, a memory matters half
    /// as much.
    pub half_life: Duration,
    /// Below this effective salience, `sweep` lets a memory go even before
    /// its TTL. The garden is pruned, not just fenced.
    pub floor: f64,
}

impl Lethe {
    pub fn new(half_life: Duration, floor: f64) -> Self {
        Lethe {
            entries: Vec::new(),
            next_id: 1,
            half_life,
            floor,
        }
    }

    /// Store a memory. TTL is mandatory: nothing enters without knowing how
    /// it will leave.
    pub fn remember(&mut self, subject: &str, content: &str, ttl: Duration, now: Instant) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Memory {
            id,
            subject: subject.to_string(),
            content: content.to_string(),
            born: now,
            last_touch: now,
            ttl,
            salience: 1.0,
        });
        id
    }

    /// Effective salience right now: stored salience decayed by elapsed
    /// half-lives since last touch.
    pub fn effective_salience(&self, m: &Memory, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(m.last_touch).as_secs_f64();
        let half_life = self.half_life.as_secs_f64().max(f64::EPSILON);
        m.salience * 0.5_f64.powf(elapsed / half_life)
    }

    /// Recall memories matching `query` (substring on subject or content).
    /// Recall is use, and use is what keeps a memory alive: matches are
    /// touched and their salience reinforced.
    pub fn recall(&mut self, query: &str, now: Instant) -> Vec<Memory> {
        let q = query.to_ascii_lowercase();
        let mut hits = Vec::new();
        for m in &mut self.entries {
            if m.subject.to_ascii_lowercase().contains(&q)
                || m.content.to_ascii_lowercase().contains(&q)
            {
                // decay first, then reinforce: recall fights the fade
                let elapsed = now.saturating_duration_since(m.last_touch).as_secs_f64();
                let hl = self.half_life.as_secs_f64().max(f64::EPSILON);
                m.salience = (m.salience * 0.5_f64.powf(elapsed / hl) + 0.5).min(2.0);
                m.last_touch = now;
                hits.push(m.clone());
            }
        }
        hits
    }

    /// The scheduled act of forgetting. Releases everything expired (TTL)
    /// or faded (effective salience below the floor).
    pub fn sweep(&mut self, now: Instant) -> Swept {
        let mut expired = 0;
        let mut faded = 0;
        let half_life = self.half_life;
        let floor = self.floor;
        self.entries.retain(|m| {
            if m.expired(now) {
                expired += 1;
                return false;
            }
            let elapsed = now.saturating_duration_since(m.last_touch).as_secs_f64();
            let hl = half_life.as_secs_f64().max(f64::EPSILON);
            let eff = m.salience * 0.5_f64.powf(elapsed / hl);
            if eff < floor {
                faded += 1;
                return false;
            }
            true
        });
        Swept { expired, faded }
    }

    /// Erasure on demand. Everything matching the predicate is released,
    /// and the caller gets a receipt hashing what was erased.
    pub fn forget<F: Fn(&Memory) -> bool>(&mut self, pred: F) -> ErasureReceipt {
        let mut hash: u64 = FNV_OFFSET;
        let mut erased = 0;
        self.entries.retain(|m| {
            if pred(m) {
                hash = fnv1a_extend(hash, &m.id.to_le_bytes());
                hash = fnv1a_extend(hash, m.subject.as_bytes());
                hash = fnv1a_extend(hash, m.content.as_bytes());
                erased += 1;
                false
            } else {
                true
            }
        });
        ErasureReceipt {
            erased,
            receipt: format!("lethe://era/{hash:016x}"),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// FNV-1a, 64-bit. Fine for demo receipts; a production Lethe would use a
// cryptographic hash — swap this function, keep the receipt shape.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_extend(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn ttl_expiry_is_swept() {
        let t0 = Instant::now();
        let mut store = Lethe::new(secs(3600), 0.05);
        store.remember("user:1", "prefers oat milk", secs(60), t0);
        store.remember("user:1", "long-lived note", secs(10_000), t0);

        let swept = store.sweep(t0 + secs(61));
        assert_eq!(
            swept,
            Swept {
                expired: 1,
                faded: 0
            }
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn unused_memories_fade_below_the_floor() {
        let t0 = Instant::now();
        // half-life 10s, floor 0.05 → ~4.4 half-lives to fade
        let mut store = Lethe::new(secs(10), 0.05);
        store.remember("scratch", "ephemeral working note", secs(100_000), t0);

        let swept = store.sweep(t0 + secs(60)); // 6 half-lives → 1/64 < 0.05
        assert_eq!(
            swept,
            Swept {
                expired: 0,
                faded: 1
            }
        );
        assert!(store.is_empty());
    }

    #[test]
    fn recall_is_use_and_use_keeps_memories_alive() {
        let t0 = Instant::now();
        let mut store = Lethe::new(secs(10), 0.05);
        store.remember("user:1", "prefers oat milk", secs(100_000), t0);

        // touch it at t+50 → reinforced, clock reset
        let hits = store.recall("oat", t0 + secs(50));
        assert_eq!(hits.len(), 1);

        // at t+60 only 10s have passed since touch → survives
        let swept = store.sweep(t0 + secs(60));
        assert_eq!(
            swept,
            Swept {
                expired: 0,
                faded: 0
            }
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn forget_erases_and_receipts() {
        let t0 = Instant::now();
        let mut store = Lethe::new(secs(3600), 0.05);
        store.remember("user:8842", "address: …", secs(100_000), t0);
        store.remember("user:8842", "order history: …", secs(100_000), t0);
        store.remember("user:1", "unrelated", secs(100_000), t0);

        let receipt = store.forget(|m| m.subject == "user:8842");
        assert_eq!(receipt.erased, 2);
        assert!(receipt.receipt.starts_with("lethe://era/"));
        assert_eq!(store.len(), 1);

        // forgetting the same predicate again erases nothing new
        let again = store.forget(|m| m.subject == "user:8842");
        assert_eq!(again.erased, 0);
    }
}
