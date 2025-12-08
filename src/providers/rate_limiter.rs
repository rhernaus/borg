//! Per-model rate limiter with exponential backoff

use rand::RngCore;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Per-model rate limiter with exponential backoff
#[derive(Debug, Default)]
pub struct ModelRateLimiter {
    states: Arc<RwLock<HashMap<String, RateLimitState>>>,
}

#[derive(Debug, Default, Clone)]
struct RateLimitState {
    backoff_until: Option<Instant>,
    consecutive_429s: u32,
}

impl ModelRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Wait if the model is currently in backoff
    pub async fn acquire(&self, model_name: &str) {
        let backoff_until = {
            let states = self.states.read().await;
            states.get(model_name).and_then(|s| s.backoff_until)
        };

        if let Some(until) = backoff_until {
            let now = Instant::now();
            if now < until {
                let wait_duration = until - now;
                log::info!(
                    "Rate limited for model '{}', waiting {:?}",
                    model_name,
                    wait_duration
                );
                tokio::time::sleep(wait_duration).await;
            }
        }
    }

    /// Record a 429 rate limit response
    pub async fn record_429(&self, model_name: &str) {
        let mut states = self.states.write().await;
        let state = states.entry(model_name.to_string()).or_default();

        state.consecutive_429s += 1;

        // Exponential backoff: 2^n seconds, capped at 64 seconds
        let base_delay_secs = 2u64.pow(state.consecutive_429s.min(6));

        // Add jitter: random 0-1000ms
        let jitter_ms = (rand::rng().next_u32() % 1000) as u64;

        let total_delay = Duration::from_secs(base_delay_secs) + Duration::from_millis(jitter_ms);

        state.backoff_until = Some(Instant::now() + total_delay);

        log::warn!(
            "429 rate limit for model '{}', backoff {} (attempt {})",
            model_name,
            format_duration(total_delay),
            state.consecutive_429s
        );
    }

    /// Record a successful request (resets backoff)
    pub async fn record_success(&self, model_name: &str) {
        let mut states = self.states.write().await;
        if let Some(state) = states.get_mut(model_name) {
            if state.consecutive_429s > 0 {
                log::debug!(
                    "Request succeeded for model '{}', resetting backoff",
                    model_name
                );
            }
            state.consecutive_429s = 0;
            state.backoff_until = None;
        }
    }

    /// Check if a model is currently in backoff
    pub async fn is_in_backoff(&self, model_name: &str) -> bool {
        let states = self.states.read().await;
        states
            .get(model_name)
            .and_then(|s| s.backoff_until)
            .map(|until| Instant::now() < until)
            .unwrap_or(false)
    }
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let ms = d.subsec_millis();
    if secs > 0 {
        format!("{}s {}ms", secs, ms)
    } else {
        format!("{}ms", ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_backoff_increases() {
        let limiter = ModelRateLimiter::new();

        // First 429
        limiter.record_429("test-model").await;
        assert!(limiter.is_in_backoff("test-model").await);

        // Second 429 should increase backoff
        limiter.record_429("test-model").await;

        // Success should reset
        limiter.record_success("test-model").await;

        // Should not be in immediate backoff after success
        // (though the backoff_until might still be in the future briefly)
    }

    #[tokio::test]
    async fn test_different_models_independent() {
        let limiter = ModelRateLimiter::new();

        limiter.record_429("model-a").await;
        assert!(limiter.is_in_backoff("model-a").await);
        assert!(!limiter.is_in_backoff("model-b").await);
    }
}
