//! Supervised long-running daemon mode.

use std::time::Duration;

/// What the supervisor should do after a failed run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RestartDecision {
    /// Wait this long, then restart.
    Backoff(Duration),
    /// Stop restarting; the daemon should exit non-zero.
    GiveUp,
}

/// Tracks exponential restart backoff and the consecutive-failure count.
pub(crate) struct RestartPolicy {
    initial: Duration,
    max_backoff: Duration,
    reset_after: Duration,
    max_restarts: u32,
    current_backoff: Duration,
    consecutive: u32,
}

impl RestartPolicy {
    pub(crate) fn new(
        initial: Duration,
        max_backoff: Duration,
        reset_after: Duration,
        max_restarts: u32,
    ) -> Self {
        Self {
            initial,
            max_backoff,
            reset_after,
            max_restarts,
            current_backoff: initial,
            consecutive: 0,
        }
    }

    /// Records a failed run of the given duration and returns what to do next.
    pub(crate) fn record_failure(&mut self, run_duration: Duration) -> RestartDecision {
        if run_duration >= self.reset_after {
            self.current_backoff = self.initial;
            self.consecutive = 0;
        }
        self.consecutive += 1;
        if self.max_restarts != 0 && self.consecutive >= self.max_restarts {
            return RestartDecision::GiveUp;
        }
        let wait = self.current_backoff;
        self.current_backoff = (self.current_backoff * 2).min(self.max_backoff);
        RestartDecision::Backoff(wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        // reset_after huge so no run counts as stable; max_restarts unlimited.
        let mut p = RestartPolicy::new(secs(1), secs(8), secs(3600), 0);
        // Short failures (0s) never reset.
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(1)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(2)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(4)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(8)));
        // Capped at max_backoff.
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(8)));
    }

    #[test]
    fn stable_run_resets_backoff_and_counter() {
        let mut p = RestartPolicy::new(secs(1), secs(60), secs(10), 0);
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(1)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(2)));
        // A run that lasted >= reset_after resets backoff to initial.
        assert_eq!(
            p.record_failure(secs(20)),
            RestartDecision::Backoff(secs(1))
        );
    }

    #[test]
    fn gives_up_after_max_consecutive_failures() {
        let mut p = RestartPolicy::new(secs(0), secs(0), secs(3600), 3);
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(0)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(0)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::GiveUp);
    }

    #[test]
    fn stable_run_prevents_giving_up() {
        let mut p = RestartPolicy::new(secs(0), secs(0), secs(10), 2);
        assert_eq!(p.record_failure(secs(0)), RestartDecision::Backoff(secs(0)));
        // Stable run resets the counter, so the next failure is counted as the first.
        assert_eq!(
            p.record_failure(secs(20)),
            RestartDecision::Backoff(secs(0))
        );
        assert_eq!(p.record_failure(secs(0)), RestartDecision::GiveUp);
    }
}
