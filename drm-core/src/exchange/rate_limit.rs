use std::time::{Duration, Instant};
use tokio::time::sleep;

pub struct RateLimiter {
    last_request: Instant,
    min_interval: Duration,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32) -> Self {
        let min_interval = if requests_per_second > 0 {
            Duration::from_secs_f64(1.0 / requests_per_second as f64)
        } else {
            Duration::ZERO
        };

        Self {
            last_request: Instant::now() - min_interval,
            min_interval,
        }
    }

    pub async fn wait(&mut self) {
        let elapsed = self.last_request.elapsed();
        if elapsed < self.min_interval {
            let wait_time = self.min_interval - elapsed;
            sleep(wait_time).await;
        }
        self.last_request = Instant::now();
    }
}

pub async fn retry_with_backoff<T, E, F, Fut>(
    max_attempts: u32,
    initial_delay: Duration,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut delay = initial_delay;

    for attempt in 0..max_attempts {
        match f().await {
            Ok(result) => return Ok(result),
            Err(_) if attempt + 1 < max_attempts => {
                sleep(delay).await;
                delay *= 2;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_respects_interval() {
        let mut limiter = RateLimiter::new(10);
        let start = Instant::now();

        limiter.wait().await;
        limiter.wait().await;

        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(90));
    }
}
