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
| `--restart-reset-after` | `60s` | If the script ran longer than this before failing, reset backoff to the initial value. |
| `--max-restarts` | `0` (unlimited) | Give up after this many consecutive failures; daemon exits non-zero. |
| `--shutdown-grace` | `10s` | After a stop signal, time allowed for the script to finish on its own before force interruption. |

Durations use `jiff::Span` parsing, consistent with the existing `--timeout`
and `--http-timeout` flags.

### Timeout semantics change

The global `--timeout` defaults to a 30s per-invocation execution timeout. In
daemon mode that would kill an infinite loop after 30s, so **daemon mode
defaults `--timeout` to disabled (equivalent to `--timeout 0`)**. The user may
still set it explicitly to bound a single supervised run.

## Architecture

The supervisor is a thin Rust loop. Pseudocode:

```
backoff = initial
restarts = 0
loop {
    start = now
    result = select! {
        r = runner.invoke(state) => r        // run the script
        _ = shutdown_signal()    => Shutdown  // SIGTERM / SIGINT
    }
    match result {
        Shutdown            => graceful_shutdown(); break
        Ok(normal return)   => exit(0)         // task complete, no restart
        Err(script error)   => {
            if now - start >= reset_after { backoff = initial }
            restarts += 1
            if max_restarts != 0 && restarts >= max_restarts { exit(non-zero) }
            log restart with backoff
            sleep(backoff)
            backoff = min(backoff * 2, max_backoff)
            runner = rebuild_runner()           // fresh VM
        }
    }
}
```

- Every restart **rebuilds the `Runner` / Lua VM** to avoid carrying over
  corrupted state from a crash.
- A successful (non-error) return is treated as task completion: the daemon
  exits 0 and does not restart.

## Graceful shutdown

The daemon owns a shared cancellation flag (`tokio_util::sync::CancellationToken`
or `Arc<AtomicBool>`) that is visible to the Lua side.

On `SIGTERM` / `SIGINT`:

1. Set the cancellation flag (Lua can observe it).
2. Wait for the in-flight `invoke` future to complete, up to `--shutdown-grace`.
3. If it has not finished within the grace period, force termination via the
   existing `set_interrupt` hook (inject an interrupt error at the next VM
   checkpoint). `sleep_ms` and other `await` points also unblock when the
   future is cancelled.
4. The daemon exits.

This satisfies both cooperative cancellation (the script can finish its current
iteration and clean up) and hard cancellation (a script that ignores the flag
is still stopped).

## Lua-facing API

In daemon mode only, `ctx` gains a `ctx.cancelled()` function that returns the
current cancellation flag as a boolean. It is injected only for this subcommand,
so it does not appear in `eval` or `serve`.

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
- `--max-restarts` giving up with a non-zero exit.
- Timeout defaulting to disabled in daemon mode.

Integration tests:

- Script that errors immediately -> observe restart count and backoff.
- Script that returns normally -> daemon exits 0, no restart.
- Cancellation delivered -> `ctx.cancelled()` becomes true and the script
  finishes within the grace period.
- Script that ignores the flag -> force-interrupted after the grace period.

## Open questions

None outstanding. Subcommand name (`daemon`), Lua API shape (`ctx.cancelled()`),
and the daemon-mode timeout default (disabled) are all confirmed.
