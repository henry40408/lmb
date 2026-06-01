# lmb daemon mode — Design

Date: 2026-06-01

## Summary

Add a `lmb daemon` subcommand that runs a long-lived Luau script under
supervision. Unlike `lmb serve`, it does not handle HTTP requests. The script
itself is a long-running loop; the daemon supervises it and restarts it on
failure with exponential backoff, and shuts it down gracefully on signal.

This is the "supervised loop" model: the loop logic lives in Lua, the Rust
layer is a thin supervisor.

## Goals

- Run a single Luau script that is expected to run indefinitely.
- Restart the script only when it fails (error / crash), with exponential
  backoff and an optional restart cap.
- Exit cleanly (success) when the script returns normally.
- Shut down gracefully on `SIGTERM` / `SIGINT`: let the script cooperatively
  finish, then force-interrupt after a grace period.
- Run as a foreground process; backgrounding is delegated to
  systemd / docker / shell `&`.

## Non-goals

- No HTTP handling (that is `serve`).
- No Unix daemonization (no double-fork, no `setsid`, no PID file).
- No scheduler / cron model (the loop and its sleeps live in Lua).
- No multi-script supervision (single `--file`).

## CLI surface

New subcommand `daemon`, reusing the existing global `--allow-*`,
`--store-*`, `--no-store`, and `--timeout` options.

```
lmb daemon --file loop.lua [--state '{...}']
           [--restart-initial-backoff 1s]
           [--restart-max-backoff 60s]
           [--restart-reset-after 60s]
           [--max-restarts 0]
           [--shutdown-grace 10s]
```

| Flag | Default | Meaning |
|---|---|---|
| `--file` | (required) | Path to the Lua script, `-` for stdin. |
| `--state` | none | JSON state passed to the script as `ctx.state`. |
| `--restart-initial-backoff` | `1s` | Wait after the first failure. |
| `--restart-max-backoff` | `60s` | Upper bound for exponential backoff. |
| `--restart-reset-after` | `60s` | If the script ran longer than this before failing, treat it as having run stably: reset **both** the backoff and the consecutive-failure counter. |
| `--max-restarts` | `0` (unlimited) | Give up after this many consecutive failures; daemon exits non-zero. A stable run (see `--restart-reset-after`) resets this counter, so it only catches rapid crash-loops, not sporadic crashes spread over a long uptime. |
| `--shutdown-grace` | `10s` | After a stop signal, time allowed for the script to finish on its own before force interruption. |

Durations use `jiff::Span` parsing, consistent with the existing `--timeout`
and `--http-timeout` flags.

### Timeout semantics change

The global `--timeout` defaults to a 30s per-invocation execution timeout. In
daemon mode that would kill an infinite loop after 30s, so **daemon mode
defaults `--timeout` to disabled (equivalent to `--timeout 0`)**. The user may
still set it explicitly to bound a single supervised run.

## Architecture

The supervisor is a thin Rust loop. Each run's `invoke` is executed in its own
`tokio::spawn`ed task, **not** awaited inline in the supervisor's `select!`.
This is required: a CPU-bound Lua loop that never yields would block the poll of
an inlined future and starve the signal branch, so the supervisor could never
react to a stop signal. Running `invoke` as a separate task (on the
multi-threaded runtime) keeps the supervisor responsive. Each freshly built `Runner` is moved
into its task (the next iteration builds a new VM anyway, so the supervisor does
not need it back). Pseudocode:

```
backoff = initial
restarts = 0
loop {
    start = now
    runner = rebuild_runner()                   // fresh VM; reused store Arc
    handle = tokio::spawn(async move {          // move owned Runner into the task
        runner.invoke(state.clone()).await      // -> LmbResult<Invoked>
    })
    outcome = select! {
        r = &mut handle => Finished(r)          // script returned or errored
        _ = shutdown_signal() => {              // SIGTERM / SIGINT
            cancellation.cancel()               // sets flag + cancel instant
            select! {
                r = &mut handle  => Shutdown    // cooperative finish, or CPU loop
                                                //   force-interrupted -> task ends
                _ = sleep(grace) => {           // async-parked & ignoring flag:
                    handle.abort()              //   abort the task (cancels await)
                    let _ = handle.await        //   reap; interrupt already forced
                    Shutdown
                }
            }
        }
    }
    match outcome {
        Shutdown                       => break       // graceful stop
        Finished(invoke success)       => exit(0)     // task complete, no restart
        Finished(invoke failure)       => {
            stable = (now - start) >= reset_after
            if stable { backoff = initial; restarts = 0 }
            restarts += 1
            if max_restarts != 0 && restarts >= max_restarts { exit(non-zero) }
            log restart with backoff
            sleep(backoff)                            // interruptible by signal
            backoff = min(backoff * 2, max_backoff)
            // loop: a fresh Runner / VM is built at the top of the next iteration
        }
    }
}
```

- Every restart **rebuilds the `Runner` / Lua VM** (top of the loop) to avoid
  carrying over corrupted state from a crash. The store `Arc` is reused across
  rebuilds, so daemon state persists between restarts.
- A successful (non-error) return is treated as task completion: the daemon
  exits 0 and does not restart.
- A stable run resets both backoff and the consecutive-failure counter before
  counting the current failure.
- The backoff `sleep` is itself interruptible by a stop signal, so shutdown is
  responsive even while waiting to restart.

### Success vs failure mapping

`Runner::invoke` returns `LmbResult<Invoked>`, and `Invoked.result` is itself a
`Result<Value, LmbError>`. The supervisor maps:

- inner `result: Ok(value)` -> **success** -> daemon exits 0.
- inner `result: Err(e)` (including `LmbError::Timeout` when the user set a
  timeout) -> **failure** -> restart.
- outer `Err` from `invoke` (e.g. I/O while building state) -> **failure** ->
  restart.

### Source buffering

Because each restart rebuilds the VM, the script source is read into a `String`
**once** at startup and reused for every rebuild — mirroring how `serve`
buffers its source. This is required for `--file -` (stdin), which cannot be
re-read.

## Graceful shutdown

The daemon owns a shared cancellation flag (`Arc<AtomicBool>`) plus the
`Instant` at which cancellation began, both visible to the Lua side and to the
`set_interrupt` hook.

On `SIGTERM` / `SIGINT`:

1. Set the cancellation flag and record the cancel `Instant` (Lua can observe
   the flag via `ctx.cancelled()`; the interrupt hook uses the instant as its
   force deadline).
2. Keep awaiting the spawned `invoke` task (`JoinHandle`), up to
   `--shutdown-grace`.
3. Force termination uses two complementary mechanisms, because `set_interrupt`
   only fires while Lua bytecode is executing:
   - **CPU-bound Lua loop** (no `await`): the `set_interrupt` hook self-checks
     "cancelled and past cancel-instant + grace" and returns an interrupt error,
     so the task ends. The supervisor stays responsive because `invoke` runs in a
     separate task (see Architecture), not inline in the `select!`.
   - **Parked at an `await`** (`sleep_ms`, `http.fetch`, ...): `set_interrupt`
     never fires, so the daemon's grace timer elapses and the supervisor calls
     `handle.abort()` — tokio sleeps and I/O are cancel-on-drop.
4. The daemon exits.

This satisfies both cooperative cancellation (the script can finish its current
iteration and clean up) and hard cancellation (a script that ignores the flag
is still stopped within the grace period regardless of where it is blocked).

Only `SIGTERM` and `SIGINT` are handled, both via `tokio::signal::unix`, which
is Unix-only. On Windows only `Ctrl-C` (`SIGINT`) is available; full Windows
support is a non-goal.

### Known limitation

Force-interrupting a CPU-bound, never-yielding Lua loop relies on the
multi-threaded runtime having a free worker to run the supervisor while the loop
occupies another (the binary's `#[tokio::main]` defaults to one worker per core).
On a strictly single-core host, such a script that also ignores
`ctx.cancelled()` cannot be force-stopped on shutdown. Cooperative cancellation
via `ctx.cancelled()` always works and is the recommended pattern.

## Runner cancellation capability

Cancellation is a **generic `Runner` capability**, not a daemon-specific hack.
`lib.rs` (the library) must not learn about "daemon". Instead the `Runner`
builder gains an optional cancellation handle: an `Arc<AtomicBool>` flag plus a
shared cancel `Instant` and grace duration that together form the interrupt's
force deadline. The daemon command in `main.rs` owns the handle and wires it in.

Two consequences for `invoke()`:

- `ctx.cancelled()` is injected into `ctx` **only when a cancellation handle is
  provided**, so it appears in daemon mode but not in `eval` / `serve`.
- The VM `set_interrupt` hook can only hold **one** closure. The existing
  timeout check and the new force-cancel check must live in the **same**
  closure: it returns an interrupt error if the timeout elapsed *or* if
  cancellation is active and the grace deadline has passed.

## Lua-facing API

In daemon mode, `ctx` gains a `ctx.cancelled()` function that returns the
current cancellation flag as a boolean (see "Runner cancellation capability").

```lua
-- loop.lua
return function(ctx)
    while not ctx.cancelled() do      -- cooperative check
        local rows = poll_new_rows()
        for _, r in ipairs(rows) do handle(r) end
        sleep_ms(60000)               -- unblocks during the grace period
    end
    cleanup()                         -- normal return -> daemon exits cleanly
    return
end
```

If the script never checks `ctx.cancelled()` and never returns, it runs until a
stop signal forces it down after the grace period.

## Error handling & observability

- Reuse the existing `tracing` setup. Log (structured) on: start, each restart
  with the chosen backoff, backoff reset, giving up after `--max-restarts`, and
  on receiving a stop signal. Restarts/backoff at `warn`; lifecycle at `info`.
- Reuse the existing `report_error` rendering for script errors before a
  restart.

## Testing (TDD)

Unit tests:

- Backoff computation: exponential growth, capped at `--restart-max-backoff`,
  reset after `--restart-reset-after`.
- `--restart-reset-after` resets both backoff and the consecutive-failure
  counter (a slow flapping script does not hit `--max-restarts`).
- `--max-restarts` giving up with a non-zero exit (rapid crash-loop).
- Timeout defaulting to disabled in daemon mode.
- Success vs failure mapping: inner `Ok` -> exit 0; inner/outer `Err` ->
  restart.

Integration tests:

- Script that errors immediately -> observe restart count and backoff.
- Script that returns normally -> daemon exits 0, no restart.
- Cancellation delivered -> `ctx.cancelled()` becomes true and the script
  finishes within the grace period.
- Script that ignores the flag, CPU-bound loop -> interrupt-forced, daemon
  exits within ~grace.
- Script that ignores the flag, parked in a long `sleep_ms` -> future abandoned,
  daemon exits within ~grace (asserts a bounded shutdown time, not an
  open-ended wait).

## Open questions

None outstanding. Confirmed decisions:

- Subcommand name: `daemon`.
- Lua API shape: `ctx.cancelled()`.
- Daemon-mode timeout default: disabled.
- `--restart-reset-after` resets both backoff and the consecutive-failure
  counter.
- Restart only on failure; clean return exits 0.
- Graceful shutdown: cooperative flag plus grace-period force interrupt.
