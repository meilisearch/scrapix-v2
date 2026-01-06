//! Politeness scheduling for respectful crawling
//!
//! Ensures crawlers respect rate limits and don't overwhelm servers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tracing::{debug, warn};

/// Configuration for politeness scheduling
#[derive(Debug, Clone)]
pub struct PolitenessConfig {
    /// Default delay between requests to the same domain (milliseconds)
    pub default_delay_ms: u64,
    /// Minimum delay between requests (milliseconds)
    pub min_delay_ms: u64,
    /// Maximum delay between requests (milliseconds)
    pub max_delay_ms: u64,
    /// Whether to respect robots.txt crawl-delay
    pub respect_robots_delay: bool,
    /// Multiplier for robots.txt delay (e.g., 1.5 to be extra polite)
    pub robots_delay_multiplier: f64,
    /// Number of concurrent requests per domain
    pub concurrent_per_domain: usize,
}

impl Default for PolitenessConfig {
    fn default() -> Self {
        Self {
            default_delay_ms: 1000, // 1 second
            min_delay_ms: 100,      // 100ms
            max_delay_ms: 30_000,   // 30 seconds
            respect_robots_delay: true,
            robots_delay_multiplier: 1.0,
            concurrent_per_domain: 2,
        }
    }
}

/// Per-domain scheduling state
struct DomainState {
    /// Last request time
    last_request: Instant,
    /// Configured delay for this domain
    delay_ms: u64,
    /// Currently in-flight requests
    in_flight: usize,
    /// Whether this domain is paused
    paused: bool,
    /// Consecutive errors (for adaptive rate limiting)
    consecutive_errors: u32,
}

impl DomainState {
    fn new(delay_ms: u64) -> Self {
        Self {
            last_request: Instant::now() - Duration::from_secs(60), // Allow immediate first request
            delay_ms,
            in_flight: 0,
            paused: false,
            consecutive_errors: 0,
        }
    }
}

/// Politeness scheduler for managing per-domain rate limits
pub struct PolitenessScheduler {
    config: PolitenessConfig,
    domains: RwLock<HashMap<String, DomainState>>,
}

impl PolitenessScheduler {
    /// Create a new politeness scheduler
    pub fn new(config: PolitenessConfig) -> Self {
        Self {
            config,
            domains: RwLock::new(HashMap::new()),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PolitenessConfig::default())
    }

    /// Get or create domain state
    #[allow(dead_code)]
    fn get_or_create_state(&self, domain: &str) -> DomainState {
        let domains = self.domains.read();
        if let Some(state) = domains.get(domain) {
            return DomainState {
                last_request: state.last_request,
                delay_ms: state.delay_ms,
                in_flight: state.in_flight,
                paused: state.paused,
                consecutive_errors: state.consecutive_errors,
            };
        }
        drop(domains);

        DomainState::new(self.config.default_delay_ms)
    }

    /// Check if a request to a domain can be made
    pub fn can_fetch(&self, domain: &str) -> bool {
        let domains = self.domains.read();

        if let Some(state) = domains.get(domain) {
            if state.paused {
                return false;
            }

            if state.in_flight >= self.config.concurrent_per_domain {
                return false;
            }

            let elapsed = state.last_request.elapsed();
            elapsed >= Duration::from_millis(state.delay_ms)
        } else {
            true // No state means first request is allowed
        }
    }

    /// Get the wait time until a domain can be fetched
    pub fn wait_time(&self, domain: &str) -> Duration {
        let domains = self.domains.read();

        if let Some(state) = domains.get(domain) {
            if state.paused {
                return Duration::from_secs(60); // Long wait for paused domains
            }

            if state.in_flight >= self.config.concurrent_per_domain {
                return Duration::from_millis(100); // Short poll interval
            }

            let elapsed = state.last_request.elapsed();
            let required = Duration::from_millis(state.delay_ms);

            if elapsed >= required {
                Duration::ZERO
            } else {
                required - elapsed
            }
        } else {
            Duration::ZERO
        }
    }

    /// Record that a request is starting
    pub fn start_request(&self, domain: &str) {
        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.in_flight += 1;
            state.last_request = Instant::now();
        } else {
            let mut state = DomainState::new(self.config.default_delay_ms);
            state.in_flight = 1;
            state.last_request = Instant::now();
            domains.insert(domain.to_string(), state);
        }
    }

    /// Record that a request completed successfully
    pub fn complete_request(&self, domain: &str) {
        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.in_flight = state.in_flight.saturating_sub(1);
            state.consecutive_errors = 0;
        }
    }

    /// Record that a request failed
    pub fn failed_request(&self, domain: &str, is_rate_limited: bool) {
        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.in_flight = state.in_flight.saturating_sub(1);
            state.consecutive_errors += 1;

            // Adaptive backoff
            if is_rate_limited || state.consecutive_errors >= 3 {
                let new_delay = (state.delay_ms as f64 * 1.5) as u64;
                state.delay_ms = new_delay.min(self.config.max_delay_ms);
                warn!(
                    domain,
                    new_delay_ms = state.delay_ms,
                    consecutive_errors = state.consecutive_errors,
                    "Increasing delay due to errors"
                );
            }

            // Pause domain if too many errors
            if state.consecutive_errors >= 10 {
                state.paused = true;
                warn!(domain, "Domain paused due to excessive errors");
            }
        }
    }

    /// Set delay for a specific domain (e.g., from robots.txt)
    pub fn set_delay(&self, domain: &str, delay_ms: u64) {
        let adjusted = if self.config.respect_robots_delay {
            ((delay_ms as f64) * self.config.robots_delay_multiplier) as u64
        } else {
            delay_ms
        };

        let clamped = adjusted
            .max(self.config.min_delay_ms)
            .min(self.config.max_delay_ms);

        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.delay_ms = clamped;
        } else {
            let state = DomainState::new(clamped);
            domains.insert(domain.to_string(), state);
        }

        debug!(domain, delay_ms = clamped, "Set domain delay");
    }

    /// Pause crawling for a domain
    pub fn pause_domain(&self, domain: &str) {
        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.paused = true;
        } else {
            let mut state = DomainState::new(self.config.default_delay_ms);
            state.paused = true;
            domains.insert(domain.to_string(), state);
        }
    }

    /// Resume crawling for a domain
    pub fn resume_domain(&self, domain: &str) {
        let mut domains = self.domains.write();

        if let Some(state) = domains.get_mut(domain) {
            state.paused = false;
            state.consecutive_errors = 0;
        }
    }

    /// Get stats for a domain
    pub fn domain_stats(&self, domain: &str) -> Option<DomainStats> {
        let domains = self.domains.read();

        domains.get(domain).map(|state| DomainStats {
            delay_ms: state.delay_ms,
            in_flight: state.in_flight,
            paused: state.paused,
            consecutive_errors: state.consecutive_errors,
            time_since_last_request_ms: state.last_request.elapsed().as_millis() as u64,
        })
    }

    /// Get all tracked domains
    pub fn tracked_domains(&self) -> Vec<String> {
        let domains = self.domains.read();
        domains.keys().cloned().collect()
    }

    /// Clear state for a domain
    pub fn clear_domain(&self, domain: &str) {
        let mut domains = self.domains.write();
        domains.remove(domain);
    }

    /// Clear all domain states
    pub fn clear_all(&self) {
        let mut domains = self.domains.write();
        domains.clear();
    }
}

/// Statistics for a domain
#[derive(Debug, Clone)]
pub struct DomainStats {
    /// Current delay between requests (ms)
    pub delay_ms: u64,
    /// Number of in-flight requests
    pub in_flight: usize,
    /// Whether the domain is paused
    pub paused: bool,
    /// Number of consecutive errors
    pub consecutive_errors: u32,
    /// Time since last request (ms)
    pub time_since_last_request_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_first_request_allowed() {
        let scheduler = PolitenessScheduler::with_defaults();
        assert!(scheduler.can_fetch("example.com"));
    }

    #[test]
    fn test_delay_enforced() {
        let config = PolitenessConfig {
            default_delay_ms: 100,
            ..Default::default()
        };
        let scheduler = PolitenessScheduler::new(config);

        scheduler.start_request("example.com");
        scheduler.complete_request("example.com");

        // Should not be able to fetch immediately
        assert!(!scheduler.can_fetch("example.com"));

        // Wait for delay
        sleep(Duration::from_millis(120));
        assert!(scheduler.can_fetch("example.com"));
    }

    #[test]
    fn test_concurrent_limit() {
        let config = PolitenessConfig {
            default_delay_ms: 0, // No delay
            concurrent_per_domain: 2,
            ..Default::default()
        };
        let scheduler = PolitenessScheduler::new(config);

        scheduler.start_request("example.com");
        assert!(scheduler.can_fetch("example.com")); // 1 in flight, limit is 2

        scheduler.start_request("example.com");
        assert!(!scheduler.can_fetch("example.com")); // 2 in flight, at limit

        scheduler.complete_request("example.com");
        assert!(scheduler.can_fetch("example.com")); // Back to 1 in flight
    }

    #[test]
    fn test_pause_resume() {
        let scheduler = PolitenessScheduler::with_defaults();

        scheduler.pause_domain("example.com");
        assert!(!scheduler.can_fetch("example.com"));

        scheduler.resume_domain("example.com");
        assert!(scheduler.can_fetch("example.com"));
    }

    #[test]
    fn test_adaptive_backoff() {
        let config = PolitenessConfig {
            default_delay_ms: 100,
            max_delay_ms: 1000,
            ..Default::default()
        };
        let scheduler = PolitenessScheduler::new(config);

        scheduler.start_request("example.com");

        // Simulate rate limiting
        scheduler.failed_request("example.com", true);

        let stats = scheduler.domain_stats("example.com").unwrap();
        assert!(stats.delay_ms > 100); // Delay should have increased
    }

    #[test]
    fn test_set_delay_from_robots() {
        let scheduler = PolitenessScheduler::with_defaults();

        scheduler.set_delay("example.com", 5000);

        let stats = scheduler.domain_stats("example.com").unwrap();
        assert_eq!(stats.delay_ms, 5000);
    }
}
