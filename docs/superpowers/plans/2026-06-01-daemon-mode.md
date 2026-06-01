# Daemon Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `lmb daemon` subcommand that runs a long-lived Luau script under supervision: restart on failure with exponential backoff, graceful shutdown on signal.

**Architecture:** A generic cancellation handle is added to `Runner`; the daemon subcommand owns a thin supervisor loop in `src/daemon.rs` that runs each `invoke` in a spawned task, restarts on failure via a `RestartPolicy`, and on a stop signal cancels cooperatively then force-stops after a grace period.

**Tech Stack:** Rust, tokio (multi-thread runtime + signal), mlua/Luau, clap, jiff (durations), bon (builders), snapbox (CLI tests).

**Spec:** `docs/superpowers/specs/2026-06-01-daemon-mode-design.md`

**Conventions (from CLAUDE.md):**
- Run tests with `cargo nextest run`, never `cargo test`.
- Run `cargo fmt` before every commit.
- Commits MUST be GPG-signed (`git commit -S`). Stage files explicitly by name; never `git add -A`/`.`.
- Commit messages end with the `Co-Authored-By` trailer shown in the commit steps.
- All work happens on the existing `feat/daemon-mode` branch.

---

## File Structure

- `Cargo.toml` — add `signal` and `time` to tokio features.
- `src/lib.rs` — add public `Cancellation` and `Cancelled` types, a `LmbError::Cancelled` variant, a `cancellation` field on `Runner`, and wire it into `invoke()` (interrupt + `ctx.cancelled()`).
- `src/daemon.rs` — **new**: `RestartPolicy`, `DaemonConfig`, `shutdown_signal()`, and the `run()` supervisor loop. Owns all daemon-specific orchestration so `main.rs` stays thin.
- `src/main.rs` — declare `mod daemon;`, add the `Daemon` subcommand variant, a `parse_daemon_timeout` helper, and the dispatch arm.
- `src/fixtures/daemon/return-immediately.lua`, `src/fixtures/daemon/always-error.lua` — **new** fixtures for CLI integration tests.
- `tests/cli_test.rs` — add a `daemon` test module (snapbox).
- `README.md` — document the daemon subcommand.

---

## Task 1: Enable tokio `signal` and `time` features

**Files:**
- Modify: `Cargo.toml:65-71`

- [ ] **Step 1: Add the features**

Edit the tokio dependency feature list to include `signal` and `time` (alphabetical order, matching the existing style):

```toml
tokio = { version = "1.52.3", default-features = false, features = [
  "fs",
  "io-std",
  "io-util",
  "macros",
  "rt-multi-thread",
  "signal",
  "time",
] }
```

- [ ] **Step 2: Verify it builds**

Run: `cd /home/nixos/Develop/claude/lmb && cargo build`
Expected: builds successfully (these features are additive).

- [ ] **Step 3: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add Cargo.toml Cargo.lock
git commit -S -m "$(cat <<'EOF'
chore: enable tokio signal and time features for daemon mode

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `Cancellation` type in lib.rs

A shared, cloneable handle: a cooperative `is_cancelled()` flag plus a force deadline computed from a one-shot cancel `Instant` and a grace `Duration`.

**Files:**
- Modify: `src/lib.rs` (imports near line 5-13; add type after the `Timeout` impl block, ~line 63)
- Test: `src/lib.rs` (`#[cfg(test)] mod tests`, ~line 398)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/lib.rs`:

```rust
#[test]
fn cancellation_flag_and_force_deadline() {
    use std::time::Duration;
    let c = Cancellation::new(Duration::from_millis(50));
    // Not cancelled initially.
    assert!(!c.is_cancelled());
    assert!(!c.force_deadline_passed());
    // After cancel(), the flag is set but the grace window has not elapsed.
    c.cancel();
    assert!(c.is_cancelled());
    assert!(!c.force_deadline_passed());
    // After the grace window, the force deadline has passed.
    std::thread::sleep(Duration::from_millis(70));
    assert!(c.force_deadline_passed());
}
```

- [ ] **Step 2: Run it to confirm it fails to compile**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --lib cancellation_flag_and_force_deadline`
Expected: FAIL — `cannot find type Cancellation`.

- [ ] **Step 3: Implement `Cancellation`**

In `src/lib.rs`, extend the std imports to include `OnceLock` and `AtomicBool`:

```rust
use std::{
    error::Error,
    fmt,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
```

Then add this type after the `impl Error for Timeout {}` line (~line 63):

```rust
/// A cooperative cancellation handle shared between a supervisor and the Lua VM.
///
/// `is_cancelled` lets a script poll for shutdown and stop on its own. If it does
/// not, `force_deadline_passed` reports when the grace period after [`cancel`](Self::cancel)
/// has elapsed, which the VM interrupt hook uses to forcibly stop a runaway script.
#[derive(Clone, Debug)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    cancel_at: Arc<OnceLock<Instant>>,
    grace: Duration,
}

impl Cancellation {
    /// Creates a new, un-cancelled handle with the given force grace period.
    pub fn new(grace: Duration) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel_at: Arc::new(OnceLock::new()),
            grace,
        }
    }

    /// Signals cancellation and records the instant. Idempotent; the first call wins.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _ = self.cancel_at.set(Instant::now());
    }

    /// Returns whether cancellation has been signalled (cooperative check).
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Returns whether the grace period has elapsed since cancellation began.
    pub fn force_deadline_passed(&self) -> bool {
        self.cancel_at.get().is_some_and(|t| t.elapsed() >= self.grace)
    }
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --lib cancellation_flag_and_force_deadline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/lib.rs
git commit -S -m "$(cat <<'EOF'
feat: add Cancellation handle for cooperative + forced cancellation

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `Cancelled` error type + `LmbError` variant

Mirrors the existing `Timeout` marker so a forced cancellation surfaces as a distinct, recognizable error.

**Files:**
- Modify: `src/lib.rs` (add marker after `Timeout`, ~line 63; add enum variant, ~line 101)

- [ ] **Step 1: Add the `Cancelled` marker type**

In `src/lib.rs`, after the `Cancellation` type from Task 2, add:

```rust
/// Marker error raised by the VM interrupt hook when a script is forcibly cancelled.
#[derive(Clone, Debug)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lua script execution was cancelled")
    }
}

impl Error for Cancelled {}
```

- [ ] **Step 2: Add the `LmbError::Cancelled` variant**

In the `LmbError` enum, add this variant after `Timeout` (~line 101):

```rust
    /// Error when the Lua script is forcibly cancelled during shutdown
    #[error("Cancelled: {0}")]
    Cancelled(#[from] Cancelled),
```

- [ ] **Step 3: Verify it builds**

Run: `cd /home/nixos/Develop/claude/lmb && cargo build`
Expected: builds successfully.

- [ ] **Step 4: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/lib.rs
git commit -S -m "$(cat <<'EOF'
feat: add Cancelled marker error and LmbError::Cancelled variant

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Wire cancellation into `Runner`

Add an optional `cancellation` field to `Runner`, thread it through both builders, and use it in `invoke()` for (a) the interrupt force-stop and (b) the `ctx.cancelled()` Lua function.

**Files:**
- Modify: `src/lib.rs` — `Runner` struct (~line 125), `new` builder (~line 160), `from_shared_reader` builder (~line 184) + struct literal (~line 262), `invoke()` interrupt block (~line 282-304), ctx build (~line 306-320), error branch (~line 345-356)
- Test: `src/lib.rs` tests module

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/lib.rs`:

```rust
#[tokio::test]
async fn ctx_cancelled_is_observable_and_cooperative() {
    use std::time::Duration;
    use tokio::io::empty;
    let cancellation = Cancellation::new(Duration::from_secs(10));
    // Script loops until ctx.cancelled() becomes true, then returns "stopped".
    let source = r#"return function(ctx)
        while not ctx.cancelled() do sleep_ms(5) end
        return "stopped"
    end"#;
    let runner = Runner::builder(source, empty())
        .cancellation(cancellation.clone())
        .build()
        .unwrap();
    let handle = tokio::spawn(async move { runner.invoke().call().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    cancellation.cancel();
    let invoked = handle.await.unwrap().unwrap();
    assert_eq!(invoked.result.unwrap(), json!("stopped"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cpu_loop_is_force_interrupted_after_grace() {
    use std::time::Duration;
    use tokio::io::empty;
    let cancellation = Cancellation::new(Duration::from_millis(100));
    // Tight CPU loop that ignores ctx.cancelled().
    let source = r#"return function(ctx) while true do end end"#;
    let runner = Runner::builder(source, empty())
        .cancellation(cancellation.clone())
        .build()
        .unwrap();
    let handle = tokio::spawn(async move { runner.invoke().call().await });
    cancellation.cancel();
    let invoked = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("invoke should be force-interrupted, not hang")
        .unwrap()
        .unwrap();
    assert!(matches!(invoked.result, Err(LmbError::Cancelled(_))));
}
```

- [ ] **Step 2: Run them to confirm they fail to compile**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --lib ctx_cancelled_is_observable_and_cooperative cpu_loop_is_force_interrupted_after_grace`
Expected: FAIL — no `cancellation` builder method.

- [ ] **Step 3: Add the `cancellation` field to `Runner`**

In the `Runner` struct (~line 125):

```rust
/// A runner for executing Lua scripts with an input stream
#[derive(Debug)]
pub struct Runner {
    cancellation: Option<Cancellation>,
    func: LuaFunction,
    reader: LmbInput,
    store: Option<LmbStore>,
    timeout: Option<Duration>,
    vm: Lua,
}
```

- [ ] **Step 4: Thread `cancellation` through both builders**

In `new` (~line 160), add the parameter and forward it. The full updated function:

```rust
    #[builder]
    pub fn new<S, R>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: R,
        cancellation: Option<Cancellation>,
        #[builder(into)] default_name: Option<String>,
        http_timeout: Option<Duration>,
        permissions: Option<Permissions>,
        store: Option<Arc<dyn StoreBackend>>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsChunk + Clone,
        R: AsyncRead + Send + Unpin + 'static,
    {
        let reader = Arc::new(SharedReader::new(reader));
        Self::from_shared_reader(source, reader)
            .maybe_cancellation(cancellation)
            .maybe_default_name(default_name)
            .maybe_http_timeout(http_timeout)
            .maybe_permissions(permissions)
            .maybe_store(store)
            .maybe_timeout(timeout)
            .call()
    }
```

In `from_shared_reader` (~line 184), add the parameter:

```rust
    #[builder]
    pub fn from_shared_reader<S>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: LmbInput,
        cancellation: Option<Cancellation>,
        #[builder(into)] default_name: Option<String>,
        http_timeout: Option<Duration>,
        permissions: Option<Permissions>,
        store: Option<LmbStore>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsChunk + Clone,
    {
```

And in the struct literal that builds the `Runner` (~line 262), add the field:

```rust
        let mut runner = Self {
            cancellation,
            func,
            reader,
            store,
            timeout,
            vm, // otherwise the Lua VM would be destroyed
        };
```

- [ ] **Step 5: Use cancellation in the `invoke()` interrupt hook**

Replace the entire `if let Some(timeout) = self.timeout { ... } else { ... }` interrupt block (~line 282-304) with a single closure that checks both timeout and cancellation:

```rust
        let timeout = self.timeout;
        let cancellation = self.cancellation.clone();
        self.vm.set_interrupt({
            let used_memory = used_memory.clone();
            move |vm| {
                used_memory.fetch_max(vm.used_memory(), Ordering::Relaxed);
                if let Some(timeout) = timeout {
                    if start.elapsed() > timeout {
                        return Err(LuaError::external(Timeout {
                            elapsed: start.elapsed(),
                            timeout,
                        }));
                    }
                }
                if let Some(cancellation) = &cancellation {
                    if cancellation.force_deadline_passed() {
                        return Err(LuaError::external(Cancelled));
                    }
                }
                Ok(LuaVmState::Continue)
            }
        });
```

- [ ] **Step 6: Inject `ctx.cancelled()` when a handle is present**

After the store block that sets `ctx.set("store", ...)` (~line 320), add:

```rust
        if let Some(cancellation) = &self.cancellation {
            let cancellation = cancellation.clone();
            ctx.set(
                "cancelled",
                self.vm
                    .create_function(move |_, ()| Ok(cancellation.is_cancelled()))?,
            )?;
        }
```

- [ ] **Step 7: Recognize `Cancelled` in the error branch**

In the `call_async` error branch, extend the `LuaError::ExternalError` match (~line 345-356) to downcast `Cancelled`:

```rust
                Err(e) => match &e {
                    LuaError::ExternalError(ee) => {
                        if let Some(timeout) = ee.downcast_ref::<Timeout>() {
                            return Ok(invoked
                                .result(Err(LmbError::Timeout(timeout.clone())))
                                .build());
                        } else if ee.downcast_ref::<Cancelled>().is_some() {
                            return Ok(invoked
                                .result(Err(LmbError::Cancelled(Cancelled)))
                                .build());
                        } else {
                            return Ok(invoked.result(Err(LmbError::Lua(e))).build());
                        }
                    }
                    _ => return Ok(invoked.result(Err(LmbError::Lua(e))).build()),
                },
```

- [ ] **Step 8: Run the tests to confirm they pass**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --lib`
Expected: PASS (including the two new tests and all existing tests — the new builder field is optional, so existing call sites are unaffected).

- [ ] **Step 9: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/lib.rs
git commit -S -m "$(cat <<'EOF'
feat: wire optional cancellation into Runner invoke

Adds ctx.cancelled() for cooperative shutdown and a force-interrupt path
in the VM interrupt hook once the grace deadline passes.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `RestartPolicy` in `src/daemon.rs`

Pure, synchronous restart accounting: exponential backoff with a cap, plus a consecutive-failure counter that resets after a stable run.

**Files:**
- Create: `src/daemon.rs`
- Modify: `src/main.rs` (add `mod daemon;` near line 28-29)

- [ ] **Step 1: Create `src/daemon.rs` with the failing test**

Create `src/daemon.rs`:

```rust
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
        assert_eq!(p.record_failure(secs(20)), RestartDecision::Backoff(secs(1)));
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
        assert_eq!(p.record_failure(secs(20)), RestartDecision::Backoff(secs(0)));
        assert_eq!(p.record_failure(secs(0)), RestartDecision::GiveUp);
    }
}
```

Add `mod daemon;` to `src/main.rs` next to the other module declarations (~line 28):

```rust
mod daemon;
mod serve;
mod tour;
```

- [ ] **Step 2: Run the tests to confirm they pass**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --bin lmb restart`
Expected: PASS for all four `RestartPolicy` tests.

(Note: `mod daemon;` will trigger dead-code warnings for `DaemonConfig`/`run` until later tasks; that is expected and resolved by Task 7. If warnings are denied in CI, they only fire on unused items — added next tasks.)

- [ ] **Step 3: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/daemon.rs src/main.rs
git commit -S -m "$(cat <<'EOF'
feat: add RestartPolicy for daemon backoff and give-up accounting

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `DaemonConfig`, `shutdown_signal()`, and the `run()` supervisor loop

The supervisor: build a fresh `Runner` each iteration, spawn `invoke` as a task, select between completion and shutdown, restart on failure, force-stop after grace. `run()` takes the shutdown trigger as a parameter so it is testable without OS signals.

**Files:**
- Modify: `src/daemon.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/daemon.rs`:

```rust
    use std::future::pending;

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
```

- [ ] **Step 2: Run them to confirm they fail to compile**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --bin lmb exits_success_when_script_returns`
Expected: FAIL — `DaemonConfig`, `run` not found.

- [ ] **Step 3: Implement `DaemonConfig`, `shutdown_signal()`, and `run()`**

At the top of `src/daemon.rs`, update imports and add the implementation (place above the `#[cfg(test)]` module):

```rust
//! Supervised long-running daemon mode.

use std::{future::Future, sync::Arc, time::Duration, time::Instant};

use bon::Builder;
use lmb::{Cancellation, LmbStore, Runner, State, permission::Permissions};
use serde_json::Value;
use tokio::io::empty;
use tracing::{info, warn};
```

```rust
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
            joined = &mut handle => {
                if !was_failure(joined) {
                    info!("script returned, daemon exiting");
                    return Ok(0);
                }
                false
            }
            _ = &mut shutdown => {
                info!("received stop signal, shutting down");
                cancellation.cancel();
                tokio::select! {
                    _ = &mut handle => {}
                    _ = tokio::time::sleep(config.grace) => {
                        warn!("grace period elapsed, aborting script");
                        handle.abort();
                        let _ = handle.await;
                    }
                }
                true
            }
        };

        if shutting_down {
            return Ok(0);
        }

        match policy.record_failure(start.elapsed()) {
            RestartDecision::GiveUp => {
                warn!("max restarts reached, giving up");
                return Ok(1);
            }
            RestartDecision::Backoff(wait) => {
                info!("restarting in {wait:?}");
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = &mut shutdown => {
                        info!("stop signal during backoff, exiting");
                        return Ok(0);
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --bin lmb`
Expected: PASS for all daemon tests (`exits_success_when_script_returns`, `exits_failure_after_max_restarts`, `graceful_shutdown_cooperative_returns_success`, `graceful_shutdown_abandons_async_parked_script`, `graceful_shutdown_force_interrupts_cpu_loop`) plus the `RestartPolicy` tests.

- [ ] **Step 5: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/daemon.rs
git commit -S -m "$(cat <<'EOF'
feat: add daemon supervisor loop with graceful shutdown

Spawns each invoke as a task so a CPU-bound loop cannot starve the signal
branch; cancels cooperatively then force-stops (interrupt or abort) after
the grace period.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: CLI wiring in `src/main.rs`

Add the `Daemon` subcommand, a daemon-specific timeout resolver (default disabled), and the dispatch arm.

**Files:**
- Modify: `src/main.rs` — imports (~line 1-26), `parse_daemon_timeout` helper (near `parse_timeout`, ~line 189), `Command` enum (~line 187), dispatch match (~line 499)

- [ ] **Step 1: Add the `parse_daemon_timeout` helper**

In `src/main.rs`, after the existing `parse_timeout` function (~line 195), add:

```rust
/// Like `parse_timeout`, but defaults to disabled (None) when unspecified, since
/// a supervised loop is expected to run indefinitely.
fn parse_daemon_timeout(span: Option<jiff::Span>) -> anyhow::Result<Option<Duration>> {
    match span {
        None => Ok(None),
        Some(t) if t.is_zero() => Ok(None),
        Some(t) => Ok(Some(Duration::try_from(t)?)),
    }
}
```

Add a unit test at the end of `src/main.rs` (create the module if it does not exist):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_timeout_defaults_to_disabled() {
        // Unspecified -> disabled (unlike eval/serve which default to 30s).
        assert_eq!(parse_daemon_timeout(None).unwrap(), None);
        // Explicit zero -> disabled.
        assert_eq!(
            parse_daemon_timeout(Some("0s".parse().unwrap())).unwrap(),
            None
        );
        // Explicit non-zero -> that duration.
        assert_eq!(
            parse_daemon_timeout(Some("5s".parse().unwrap())).unwrap(),
            Some(Duration::from_secs(5))
        );
    }
}
```

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --bin lmb daemon_timeout_defaults_to_disabled`
Expected: PASS.

- [ ] **Step 2: Add the `Daemon` variant to the `Command` enum**

In the `Command` enum, after the `Serve { ... }` variant (~line 186), add:

```rust
    /// Run a Lua script as a supervised long-running daemon (no HTTP)
    #[clap(after_help = "\
EXAMPLES:
    lmb daemon --file loop.lua
    lmb daemon --file loop.lua --shutdown-grace 30s
    lmb daemon --file loop.lua --restart-max-backoff 2m --max-restarts 10")]
    Daemon {
        /// Path to the Lua script file, use '-' for stdin
        #[clap(long, value_parser, env = "FILE_PATH")]
        file: Input,
        /// JSON state passed to the Lua script as ctx.state
        #[clap(long, env = "STATE")]
        state: Option<String>,
        /// Initial backoff after a failure (e.g., 1s)
        #[clap(long, default_value = "1s", env = "RESTART_INITIAL_BACKOFF")]
        restart_initial_backoff: jiff::Span,
        /// Maximum backoff between restarts (e.g., 60s)
        #[clap(long, default_value = "60s", env = "RESTART_MAX_BACKOFF")]
        restart_max_backoff: jiff::Span,
        /// Reset backoff and failure count after the script runs stably this long
        #[clap(long, default_value = "60s", env = "RESTART_RESET_AFTER")]
        restart_reset_after: jiff::Span,
        /// Give up after this many consecutive failures (0 = unlimited)
        #[clap(long, default_value_t = 0, env = "MAX_RESTARTS")]
        max_restarts: u32,
        /// Grace period for the script to stop after a signal (e.g., 10s)
        #[clap(long, default_value = "10s", env = "SHUTDOWN_GRACE")]
        shutdown_grace: jiff::Span,
    },
```

- [ ] **Step 3: Add the dispatch arm**

In the `match opts.command` block, after the `Command::Serve { ... } => { ... }` arm (~line 498), add:

```rust
        Command::Daemon {
            mut file,
            state,
            restart_initial_backoff,
            restart_max_backoff,
            restart_reset_after,
            max_restarts,
            shutdown_grace,
        } => {
            let state = state
                .as_ref()
                .map(|s| match serde_json::from_str::<Value>(s) {
                    Ok(value) => value,
                    Err(_) => json!(s.clone()), // treat invalid value as string
                });
            debug!("State: {state:?}");

            let http_timeout = parse_timeout(opts.http_timeout)?;
            let timeout = parse_daemon_timeout(opts.timeout)?;
            debug!("Using timeout: {timeout:?}");

            let mut source = String::new();
            file.read_to_string(&mut source)?;

            let name = if file.is_local() {
                file.path().to_string_lossy().to_string()
            } else if file.is_std() {
                "(stdin)".to_string()
            } else {
                bail!("Expected a local file or a stdin input, but got: {file}");
            };

            if opts.store_path.is_none() && !opts.no_store {
                #[cfg(feature = "postgres")]
                if opts.store_url.is_none() {
                    warn!("No store path specified, using in-memory store");
                }
                #[cfg(not(feature = "postgres"))]
                warn!("No store path specified, using in-memory store");
            }
            let store = open_store_connection(
                opts.store_path,
                #[cfg(feature = "postgres")]
                opts.store_url,
                opts.no_store,
            )?;

            let config = daemon::DaemonConfig::builder()
                .source(source)
                .maybe_name(Some(name))
                .maybe_state(state)
                .permissions(permissions)
                .maybe_store(store)
                .maybe_http_timeout(http_timeout)
                .maybe_timeout(timeout)
                .initial_backoff(Duration::try_from(restart_initial_backoff)?)
                .max_backoff(Duration::try_from(restart_max_backoff)?)
                .reset_after(Duration::try_from(restart_reset_after)?)
                .max_restarts(max_restarts)
                .grace(Duration::try_from(shutdown_grace)?)
                .build();

            let code = daemon::run(config, daemon::shutdown_signal()).await?;
            if code != 0 {
                std::process::exit(i32::from(code));
            }
        }
```

- [ ] **Step 4: Verify it builds**

Run: `cd /home/nixos/Develop/claude/lmb && cargo build`
Expected: builds successfully with no dead-code warnings (all daemon items are now used).

- [ ] **Step 5: Manual smoke test**

Run a short cooperative script and interrupt it:

```bash
cd /home/nixos/Develop/claude/lmb
printf 'return function(ctx)\n  while not ctx.cancelled() do sleep_ms(200) end\n  return\nend\n' > /tmp/loop.lua
cargo run -- --no-store daemon --file /tmp/loop.lua &
sleep 1; kill -TERM %1; wait %1; echo "exit: $?"
```
Expected: process exits cleanly (`exit: 0`) within the grace period.

- [ ] **Step 6: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/main.rs
git commit -S -m "$(cat <<'EOF'
feat: add `lmb daemon` subcommand wiring

Timeout defaults to disabled in daemon mode; reuses the global permission
and store options; SIGTERM/SIGINT drive graceful shutdown.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: CLI integration tests (snapbox)

**Files:**
- Create: `src/fixtures/daemon/return-immediately.lua`, `src/fixtures/daemon/always-error.lua`
- Modify: `tests/cli_test.rs` (add a `daemon` module)

- [ ] **Step 1: Create the fixtures**

`src/fixtures/daemon/return-immediately.lua`:

```lua
return function(ctx)
    return true
end
```

`src/fixtures/daemon/always-error.lua`:

```lua
return function(ctx)
    error("boom")
end
```

- [ ] **Step 2: Write the failing tests**

Add to `tests/cli_test.rs`, after the existing top-level test modules:

```rust
mod daemon {
    use super::*;

    #[test]
    fn exits_success_when_script_returns() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "--no-store",
                "daemon",
                "--file",
                "src/fixtures/daemon/return-immediately.lua",
            ])
            .assert()
            .success()
            .stdout_eq(str![]);
    }

    #[test]
    fn exits_failure_after_max_restarts() {
        Command::new(cmd::cargo_bin!("lmb"))
            .env("NO_COLOR", "true")
            .args([
                "--no-store",
                "daemon",
                "--file",
                "src/fixtures/daemon/always-error.lua",
                "--restart-initial-backoff",
                "0s",
                "--restart-max-backoff",
                "0s",
                "--max-restarts",
                "1",
            ])
            .assert()
            .failure();
    }
}
```

- [ ] **Step 3: Run them to confirm they pass**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run --test cli_test daemon`
Expected: PASS. The success case produces no stdout (the daemon does not print the script's return value); the failure case exits non-zero after one failed run.

- [ ] **Step 4: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add src/fixtures/daemon/return-immediately.lua src/fixtures/daemon/always-error.lua tests/cli_test.rs
git commit -S -m "$(cat <<'EOF'
test: add daemon CLI integration tests

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Documentation

**Files:**
- Modify: `README.md` (Features list ~line 31-36; add a usage example near the existing eval example)
- Modify: `src/main.rs` (`AFTER_HELP` constant ~line 40-61)

- [ ] **Step 1: Update README features and add an example**

In `README.md`, add a bullet to the Features list:

```markdown
- Long-running: Run a script as a supervised daemon (`lmb daemon`) that restarts on failure and shuts down gracefully on `SIGTERM`/`SIGINT`.
```

After the existing `lmb eval` example block, add:

```markdown
You can also run a script as a supervised daemon that loops until it is told to stop:

```bash
$ cat > loop.lua <<EOF
> return function(ctx)
>     while not ctx.cancelled() do
>         print("tick")
>         sleep_ms(1000)
>     end
> end
> EOF

$ lmb daemon --file loop.lua   # Ctrl-C to stop
tick
tick
```
```

- [ ] **Step 2: Add a daemon example to `AFTER_HELP`**

In `src/main.rs`, add to the `AFTER_HELP` string after the "Start an HTTP server" example:

```rust
    Run a supervised daemon:
        lmb daemon --file loop.lua
```

- [ ] **Step 3: Verify the help renders**

Run: `cd /home/nixos/Develop/claude/lmb && cargo run -- --help` and `cargo run -- daemon --help`
Expected: the daemon subcommand and example appear; daemon help lists all the restart/grace flags.

- [ ] **Step 4: Commit**

```bash
cd /home/nixos/Develop/claude/lmb && cargo fmt
git add README.md src/main.rs
git commit -S -m "$(cat <<'EOF'
docs: document the daemon subcommand

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Final verification

- [ ] **Step 1: Full test suite**

Run: `cd /home/nixos/Develop/claude/lmb && cargo nextest run`
Expected: all tests pass.

- [ ] **Step 2: Lint**

Run: `cd /home/nixos/Develop/claude/lmb && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Format check**

Run: `cd /home/nixos/Develop/claude/lmb && cargo fmt --check`
Expected: clean.
