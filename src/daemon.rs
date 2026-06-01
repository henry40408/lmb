//! Supervised long-running daemon mode.

use std::{future::Future, time::Duration, time::Instant};

use bon::Builder;
use lmb::{Cancellation, LmbStore, Runner, State, permission::Permissions};
use serde_json::Value;
use tokio::io::empty;
use tracing::{info, warn};

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

/// Configuration for a daemon run.
#[derive(Builder)]
pub(crate) struct DaemonConfig {
    #[builder(into)]
    pub source: String,
    #[builder(into)]
    pub name: Option<String>,
    pub state: Option<Value>,
    pub permissions: Option<Permissions>,
    pub store: Option<LmbStore>,
    pub http_timeout: Option<Duration>,
    pub timeout: Option<Duration>,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub reset_after: Duration,
    pub max_restarts: u32,
    pub grace: Duration,
}

/// Builds a fresh `Runner` for one supervised run.
fn build_runner(config: &DaemonConfig, cancellation: Cancellation) -> anyhow::Result<Runner> {
    let runner = Runner::builder(config.source.clone(), empty())
        .cancellation(cancellation)
        .maybe_default_name(config.name.clone())
        .maybe_http_timeout(config.http_timeout)
        .maybe_permissions(config.permissions.clone())
        .maybe_store(config.store.clone())
        .maybe_timeout(config.timeout)
        .build()?;
    Ok(runner)
}

/// Resolves whether a finished run was a clean completion or a failure.
///
/// Only called from the task-completion (non-shutdown) path: a forced cancellation
/// during shutdown lands in the inner select and its result is discarded there.
fn was_failure(joined: Result<lmb::LmbResult<lmb::Invoked>, tokio::task::JoinError>) -> bool {
    match joined {
        Ok(Ok(invoked)) => match invoked.result {
            Ok(_) => false,
            Err(e) => {
                warn!("script failed: {e}");
                true
            }
        },
        Ok(Err(e)) => {
            warn!("invoke error: {e}");
            true
        }
        Err(e) => {
            warn!("daemon task panicked: {e}");
            true
        }
    }
}

/// Future that resolves when a stop signal (SIGTERM/SIGINT) is received.
#[cfg(unix)]
pub(crate) async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => {},
        _ = int.recv() => {},
    }
}

/// Future that resolves when Ctrl-C is received (non-Unix fallback).
#[cfg(not(unix))]
pub(crate) async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Runs the supervised loop until the script completes, the restart policy gives
/// up, or a shutdown is requested. Returns the process exit code (0 = success).
pub(crate) async fn run<F>(config: DaemonConfig, shutdown: F) -> anyhow::Result<u8>
where
    F: Future<Output = ()>,
{
    info!("daemon starting");
    let cancellation = Cancellation::new(config.grace);
    let mut policy = RestartPolicy::new(
        config.initial_backoff,
        config.max_backoff,
        config.reset_after,
        config.max_restarts,
    );
    tokio::pin!(shutdown);

    loop {
        let runner = build_runner(&config, cancellation.clone())?;
        let state = config.state.clone();
        let start = Instant::now();
        let mut handle = tokio::spawn(async move {
            let state = State::builder().maybe_state(state).build();
            runner.invoke().state(state).call().await
        });

        let shutting_down = tokio::select! {
            // `biased` + shutdown-first: whenever a stop signal is ready it is always
            // selected, so the (reused) shutdown future is never polled after it
            // completes — avoids a Poll::Ready-after-completion contract violation.
            biased;
            _ = &mut shutdown => {
                info!("received stop signal, shutting down");
                cancellation.cancel();
                tokio::select! {
                    biased;
                    _ = &mut handle => {}
                    _ = tokio::time::sleep(config.grace) => {
                        warn!("grace period elapsed, aborting script");
                        handle.abort();
                        let _ = handle.await;
                    }
                }
                true
            }
            joined = &mut handle => {
                if !was_failure(joined) {
                    info!("script returned, daemon exiting");
                    return Ok(0);
                }
                false
            }
        };

        if shutting_down {
            return Ok(0);
        }

        // A run that lasted at least `reset_after` is considered stable;
        // `RestartPolicy::record_failure` resets the backoff and failure counter
        // in that case. Log it here (the policy itself is pure / log-free).
        let ran = start.elapsed();
        if ran >= config.reset_after {
            info!("script ran stably for {ran:?}, resetting backoff and failure count");
        }
        match policy.record_failure(ran) {
            RestartDecision::GiveUp => {
                warn!("max restarts reached, giving up");
                return Ok(1);
            }
            RestartDecision::Backoff(wait) => {
                info!("restarting in {wait:?}");
                tokio::select! {
                    biased;
                    _ = &mut shutdown => {
                        info!("stop signal during backoff, exiting");
                        return Ok(0);
                    }
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::future::pending;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn config(source: &str, max_restarts: u32, grace: Duration) -> DaemonConfig {
        DaemonConfig::builder()
            .source(source.to_string())
            .initial_backoff(Duration::ZERO)
            .max_backoff(Duration::ZERO)
            .reset_after(secs(3600))
            .max_restarts(max_restarts)
            .grace(grace)
            .build()
    }

    #[tokio::test]
    async fn exits_success_when_script_returns() {
        let cfg = config("return function(ctx) return 1 end", 0, secs(1));
        let code = run(cfg, pending::<()>()).await.unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn exits_failure_after_max_restarts() {
        let cfg = config("return function(ctx) error('boom') end", 2, secs(1));
        let code = run(cfg, pending::<()>()).await.unwrap();
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn graceful_shutdown_cooperative_returns_success() {
        let cfg = config(
            "return function(ctx) while not ctx.cancelled() do sleep_ms(5) end return end",
            0,
            secs(1),
        );
        let shutdown = async { tokio::time::sleep(Duration::from_millis(40)).await };
        let code = tokio::time::timeout(secs(5), run(cfg, shutdown))
            .await
            .expect("should not hang")
            .unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn graceful_shutdown_abandons_async_parked_script() {
        let cfg = config(
            "return function(ctx) sleep_ms(600000) end",
            0,
            Duration::from_millis(150),
        );
        let shutdown = async { tokio::time::sleep(Duration::from_millis(40)).await };
        let code = tokio::time::timeout(secs(5), run(cfg, shutdown))
            .await
            .expect("should not hang")
            .unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn graceful_shutdown_force_interrupts_cpu_loop() {
        let cfg = config(
            "return function(ctx) while true do end end",
            0,
            Duration::from_millis(150),
        );
        let shutdown = async { tokio::time::sleep(Duration::from_millis(40)).await };
        let code = tokio::time::timeout(secs(5), run(cfg, shutdown))
            .await
            .expect("should not hang")
            .unwrap();
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn was_failure_flags_invoke_error_and_panic() {
        // Outer invoke error: returning a function cannot be serialized into a
        // Value, so `invoke` yields `Ok(Err(_))`.
        let cfg = config("return function(ctx) return function() end end", 0, secs(1));
        let runner = build_runner(&cfg, Cancellation::new(secs(1))).unwrap();
        let state = State::builder().build();
        let invoke_err: Result<lmb::LmbResult<lmb::Invoked>, tokio::task::JoinError> =
            Ok(runner.invoke().state(state).call().await);
        assert!(matches!(invoke_err, Ok(Err(_))));
        assert!(was_failure(invoke_err));

        // A panic in the spawned task surfaces as a `JoinError`.
        let panicked: Result<lmb::LmbResult<lmb::Invoked>, tokio::task::JoinError> =
            tokio::spawn(async { panic!("boom") })
                .await
                .map(|()| unreachable!());
        assert!(was_failure(panicked));
    }

    #[tokio::test]
    async fn shutdown_during_backoff_exits_success() {
        // First run fails fast, entering a long backoff; the stop signal lands
        // during that wait and must exit cleanly (code 0).
        let cfg = DaemonConfig::builder()
            .source("return function(ctx) error('boom') end")
            .initial_backoff(Duration::from_millis(500))
            .max_backoff(Duration::from_millis(500))
            .reset_after(secs(3600))
            .max_restarts(0)
            .grace(secs(1))
            .build();
        let shutdown = async { tokio::time::sleep(Duration::from_millis(60)).await };
        let code = tokio::time::timeout(secs(5), run(cfg, shutdown))
            .await
            .expect("should not hang")
            .unwrap();
        assert_eq!(code, 0);
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
