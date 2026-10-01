use std::time::Duration;

use rand::RngExt;

const BACKOFF_FLOORS: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
    Duration::from_secs(30),
];
const JITTER_MAX_MS: u64 = 250;

#[derive(Debug, Default)]
pub(crate) struct WorkerBackoff {
    consecutive_failures: usize,
}

impl WorkerBackoff {
    pub(crate) fn reset(&mut self) {
        self.consecutive_failures = 0;
    }

    pub(crate) fn failure_count(&self) -> usize {
        self.consecutive_failures
    }

    pub(crate) fn next_delay(&mut self) -> Duration {
        let jitter_ms = rand::rng().random_range(0..=JITTER_MAX_MS);
        self.next_delay_with_jitter(Duration::from_millis(jitter_ms))
    }

    fn next_delay_with_jitter(&mut self, jitter: Duration) -> Duration {
        let floor_index = self
            .consecutive_failures
            .min(BACKOFF_FLOORS.len().saturating_sub(1));
        let floor = BACKOFF_FLOORS[floor_index];
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        floor.saturating_add(jitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_backoff_uses_required_floors_and_caps() {
        let mut backoff = WorkerBackoff::default();
        for expected in BACKOFF_FLOORS {
            assert_eq!(backoff.next_delay_with_jitter(Duration::ZERO), expected);
        }
        assert_eq!(
            backoff.next_delay_with_jitter(Duration::from_millis(250)),
            Duration::from_secs(30) + Duration::from_millis(250)
        );
    }

    #[test]
    fn jitter_only_extends_the_backoff_floor_and_reset_restarts_sequence() {
        let mut backoff = WorkerBackoff::default();
        assert_eq!(
            backoff.next_delay_with_jitter(Duration::from_millis(125)),
            Duration::from_secs(1) + Duration::from_millis(125)
        );
        backoff.reset();
        assert_eq!(
            backoff.next_delay_with_jitter(Duration::ZERO),
            Duration::from_secs(1)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failure_wait_does_not_complete_before_the_floor() {
        let mut backoff = WorkerBackoff::default();
        let jitter = Duration::from_millis(250);

        for floor in BACKOFF_FLOORS {
            let delay = backoff.next_delay_with_jitter(jitter);
            let sleeper = tokio::time::sleep(delay);
            tokio::pin!(sleeper);

            tokio::time::advance(floor - Duration::from_millis(1)).await;
            tokio::task::yield_now().await;
            assert!(!sleeper.is_elapsed());

            tokio::time::advance(Duration::from_millis(1) + jitter).await;
            sleeper.await;
        }
    }
}
