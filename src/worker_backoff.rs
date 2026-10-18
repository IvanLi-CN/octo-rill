use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use rand::RngExt;
use tokio::sync::Notify;

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

#[derive(Debug, Default)]
pub(crate) struct ClaimAdmission {
    in_flight: AtomicBool,
    retry_at: Mutex<Option<Instant>>,
    failures: std::sync::atomic::AtomicUsize,
    notify: Notify,
}

pub(crate) enum ClaimAdmissionResult<'a> {
    Admitted(ClaimAdmissionGuard<'a>),
    Coalesced,
    CoolingDown,
}

pub(crate) struct ClaimAdmissionGuard<'a> {
    admission: &'a ClaimAdmission,
}

impl ClaimAdmission {
    pub(crate) const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
            retry_at: Mutex::new(None),
            failures: std::sync::atomic::AtomicUsize::new(0),
            notify: Notify::const_new(),
        }
    }

    pub(crate) fn try_admit(&self) -> ClaimAdmissionResult<'_> {
        let Ok(retry_at) = self.retry_at.lock() else {
            return ClaimAdmissionResult::Coalesced;
        };
        if retry_at.is_some_and(|retry_at| Instant::now() < retry_at) {
            return ClaimAdmissionResult::CoolingDown;
        }
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return ClaimAdmissionResult::Coalesced;
        }
        ClaimAdmissionResult::Admitted(ClaimAdmissionGuard { admission: self })
    }

    pub(crate) fn retry_after(&self, minimum: Duration) -> Duration {
        let remaining = self
            .retry_at
            .lock()
            .ok()
            .and_then(|retry_at| {
                retry_at.map(|retry_at| retry_at.saturating_duration_since(Instant::now()))
            })
            .unwrap_or_default();
        remaining.max(minimum)
    }

    pub(crate) async fn wait(&self, minimum: Duration) {
        let notified = self.notify.notified();
        let delay = self.retry_after(minimum);
        tokio::select! {
            _ = notified => {}
            _ = tokio::time::sleep(delay) => {}
        }
    }

    pub(crate) fn wake(&self) {
        if let Ok(mut retry_at) = self.retry_at.lock() {
            *retry_at = None;
        }
        self.failures.store(0, Ordering::Relaxed);
        self.notify_waiters();
    }

    fn notify_waiters(&self) {
        self.notify.notify_waiters();
    }

    #[cfg(test)]
    pub(crate) fn reset_for_tests(&self) {
        self.in_flight.store(false, Ordering::Release);
        if let Ok(mut retry_at) = self.retry_at.lock() {
            *retry_at = None;
        }
        self.failures.store(0, Ordering::Relaxed);
        self.notify_waiters();
    }

    fn schedule_backoff(&self) {
        let index = self
            .failures
            .fetch_add(1, Ordering::Relaxed)
            .min(BACKOFF_FLOORS.len() - 1);
        let delay = BACKOFF_FLOORS[index]
            + Duration::from_millis(rand::rng().random_range(0..=JITTER_MAX_MS));
        if let Ok(mut retry_at) = self.retry_at.lock() {
            *retry_at = Some(Instant::now() + delay);
        }
    }
}

impl ClaimAdmissionGuard<'_> {
    pub(crate) fn success(&mut self) {
        if let Ok(mut retry_at) = self.admission.retry_at.lock() {
            *retry_at = None;
        }
        self.admission.failures.store(0, Ordering::Relaxed);
        self.admission.notify_waiters();
    }

    pub(crate) fn fail(&mut self) {
        self.admission.schedule_backoff();
    }

    pub(crate) fn empty(&mut self) {
        self.admission.schedule_backoff();
    }
}

impl Drop for ClaimAdmissionGuard<'_> {
    fn drop(&mut self) {
        self.admission.in_flight.store(false, Ordering::Release);
        self.admission.notify_waiters();
    }
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
    use std::sync::Arc;

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

    #[test]
    fn claim_admission_coalesces_in_flight_and_cools_down_after_failure() {
        let admission = ClaimAdmission::new();
        let mut guard = match admission.try_admit() {
            ClaimAdmissionResult::Admitted(guard) => guard,
            _ => panic!("first claim should be admitted"),
        };
        assert!(matches!(
            admission.try_admit(),
            ClaimAdmissionResult::Coalesced
        ));
        guard.fail();
        drop(guard);
        assert!(matches!(
            admission.try_admit(),
            ClaimAdmissionResult::CoolingDown
        ));
    }

    #[test]
    fn ready_signal_clears_empty_claim_cooldown() {
        let admission = ClaimAdmission::new();
        let mut guard = match admission.try_admit() {
            ClaimAdmissionResult::Admitted(guard) => guard,
            _ => panic!("first claim should be admitted"),
        };
        guard.empty();
        drop(guard);
        assert!(matches!(
            admission.try_admit(),
            ClaimAdmissionResult::CoolingDown
        ));
        admission.wake();
        assert!(matches!(
            admission.try_admit(),
            ClaimAdmissionResult::Admitted(_)
        ));
    }

    #[tokio::test]
    async fn ready_signal_interrupts_the_worker_wait() {
        let admission = Arc::new(ClaimAdmission::new());
        let waiter_admission = admission.clone();
        let waiter = tokio::spawn(async move {
            waiter_admission.wait(Duration::from_secs(30)).await;
        });
        tokio::task::yield_now().await;
        admission.wake();
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("ready signal should wake the worker")
            .expect("worker wait task should join");
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
