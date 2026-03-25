//! In-memory auth rate limiter that works without Redis.
//!
//! Uses a lock-free concurrent hashmap so multiple requests can be checked
//! in parallel without contention on a single mutex.

use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

struct AuthWindow {
    count: u64,
    window_start: Instant,
}

/// In-memory sliding-window rate limiter for auth endpoints.
///
/// Tracks request counts per IP address with automatic window expiry.
/// Auto-evicts entries when the map exceeds 256 entries.
#[derive(Clone, Default)]
pub struct InMemoryAuthRateLimiter {
    windows: Arc<DashMap<String, AuthWindow>>,
}

impl InMemoryAuthRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check and increment counter for an IP. Returns current count.
    pub fn check(&self, ip: &str, window_secs: u64) -> u64 {
        let now = Instant::now();

        // Evict expired entries periodically
        if self.windows.len() > 256 {
            self.windows
                .retain(|_, w| now.duration_since(w.window_start).as_secs() < window_secs);
        }

        let mut entry = self.windows.entry(ip.to_string()).or_insert(AuthWindow {
            count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(entry.window_start).as_secs() >= window_secs {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count += 1;
        entry.count
    }
}
