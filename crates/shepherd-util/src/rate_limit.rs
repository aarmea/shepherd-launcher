//! Rate limiting utilities

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::ClientId;

/// Simple token-bucket rate limiter
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum tokens (requests) per bucket
    max_tokens: u32,
    /// How often tokens are replenished
    refill_interval: Duration,
    /// Per-client state
    clients: HashMap<ClientId, ClientBucket>,
}

#[derive(Debug)]
struct ClientBucket {
    tokens: u32,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    ///
    /// # Arguments
    /// * `max_requests` - Maximum requests allowed per interval
    /// * `interval` - Time interval for the limit
    pub fn new(max_requests: u32, interval: Duration) -> Self {
        Self {
            max_tokens: max_requests,
            refill_interval: interval,
            clients: HashMap::new(),
        }
    }

    /// Check if a request should be allowed for the given client
    ///
    /// Returns `true` if allowed, `false` if rate limited
    pub fn check(&mut self, client_id: &ClientId) -> bool {
        let now = Instant::now();

        let bucket = self.clients.entry(client_id.clone()).or_insert(ClientBucket {
            tokens: self.max_tokens,
            last_refill: now,
        });

        // Refill tokens if interval has passed
        let elapsed = now.duration_since(bucket.last_refill);
        if elapsed >= self.refill_interval {
            let intervals = (elapsed.as_millis() / self.refill_interval.as_millis()) as u32;
            bucket.tokens = (bucket.tokens + intervals * self.max_tokens).min(self.max_tokens);
            bucket.last_refill = now;
        }

        // Try to consume a token
        if bucket.tokens > 0 {
            bucket.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Remove a client's rate limit state
    pub fn remove_client(&mut self, client_id: &ClientId) {
        self.clients.remove(client_id);
    }

    /// Clean up stale client entries
    pub fn cleanup(&mut self, stale_after: Duration) {
        let now = Instant::now();
        self.clients.retain(|_, bucket| {
            now.duration_since(bucket.last_refill) < stale_after
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let mut limiter = RateLimiter::new(5, Duration::from_secs(1));
        let client = ClientId::new();

        for _ in 0..5 {
            assert!(limiter.check(&client));
        }

        // 6th request should be denied
        assert!(!limiter.check(&client));
    }

    #[test]
    fn test_rate_limiter_different_clients() {
        let mut limiter = RateLimiter::new(2, Duration::from_secs(1));
        let client1 = ClientId::new();
        let client2 = ClientId::new();

        assert!(limiter.check(&client1));
        assert!(limiter.check(&client1));
        assert!(!limiter.check(&client1));

        // Client 2 should have its own bucket
        assert!(limiter.check(&client2));
        assert!(limiter.check(&client2));
    }
}
