# `fs.tail` — File Tail Follow for `@lmb/fs`

## Overview

Add a `tail(path, options?)` method to the `@lmb/fs` module that returns a line iterator which follows a file indefinitely, similar to `tail -F`. The iterator yields new lines as they are appended and automatically follows file rotations (e.g., logrotate).

## API

```lua
local fs = require("@lmb/fs")

-- Basic usage: follow from end of file
for line in fs.tail("/var/log/nginx/access.log") do
    if line:match("500") then
        -- handle error line
    end
end

-- With options
for line in fs.tail("/var/log/nginx/access.log", {
    poll_interval = 200,    -- milliseconds, default: 100
    from = "end",           -- "end" (default) = start from file tail / "start" = read from beginning
}) do
    print(line)
end
```

### Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `path` | string | required | File path to tail |
| `options.poll_interval` | number | `100` | Milliseconds to sleep when no new data is available |
| `options.from` | string | `"end"` | Where to start reading: `"end"` skips existing content, `"start"` reads from beginning |

### Return Value

Returns a Lua iterator function for use in `for...in` loops. The iterator yields one line (string) per call, with trailing newlines stripped (consistent with `fs.lines()`).

## Behavior

| Scenario | Behavior |
|----------|----------|
| New line available | Return immediately, no sleep |
| At EOF (no new data) | Sleep `poll_interval` ms, then retry |
| File rotated (inode change or size shrink) | Reopen the file at the same path, read from beginning |
| File does not exist yet | Wait (polling) until the file appears, then start reading |
| File temporarily missing (rotate gap) | Keep waiting, resume when file reappears |
| `break` in for loop | Exit loop normally. The `TailState` (including open `File` handle) is held by `Arc<Mutex<...>>` inside the closure; it is released when the closure is garbage-collected. This is non-deterministic but safe — consistent with how `fs.lines()` handles its file handle. |
| Permission denied | Call `check_read_permission(path)` at call time (before iterator starts). The existing `canonicalize_for_check` already handles non-existent files by canonicalizing the parent directory and appending the filename. |

## Rotation Detection

On each poll cycle (when EOF is reached and before sleeping):

1. Stat the path to get current inode and file size
2. If inode differs from the last known inode → file has been rotated → reopen and read from beginning
3. If file size is smaller than current read position → file has been truncated → seek to beginning
4. If stat fails (file gone) → enter waiting mode, keep polling until file reappears

This matches the behavior of `tail -F` (capital F) in GNU coreutils.

**Known limitation (TOCTOU race):** There is a small window between stat and read where rotation could occur. If the file is rotated between these two operations, we may briefly read from the old file descriptor. This is the same race condition that GNU `tail -F` has and is acceptable — the next poll cycle will detect the inode change and correct itself. After reopening, we re-stat to verify the inode matches the newly opened file.

## Implementation

### Approach: Polling with Async Sleep

Uses `add_async_function` with `tokio::time::sleep` for the polling wait. This is critical because LMB runs Lua scripts directly on the Tokio runtime via `call_async` — there is no `spawn_blocking` or dedicated thread pool. Using `std::thread::sleep` would block a Tokio worker thread indefinitely.

The iterator closure is registered as an async function so that `tokio::time::sleep` yields control back to the runtime between poll cycles. The existing `fs.lines()` uses synchronous `add_method` because it terminates at EOF; `fs.tail` runs indefinitely and must not monopolize a runtime thread.

**Why polling over inotify/notify:**
- No new dependencies, no impact on binary size (LMB targets lightweight deployments)
- Consistent behavior across all platforms and filesystems (including NFS where inotify does not work)
- 100ms polling interval has negligible CPU cost (10 syscalls/second when idle)
- GNU `tail -f` itself uses polling as its default strategy

### Rust Implementation

New method added to `FsBinding::add_methods` in `src/bindings/fs.rs`:

```rust
methods.add_method("tail", |vm, this, (path, options): (String, Option<LuaTable>)| {
    // Permission check: pass full file path. check_read_permission -> canonicalize_for_check
    // already handles non-existent files by canonicalizing the parent dir and appending filename.
    this.check_read_permission(&path).map_err(LuaError::runtime)?;

    let poll_interval = /* extract from options, default 100 */;
    let from_end = /* extract from options, default true */;

    let state = Arc::new(Mutex::new(TailState::new(path, poll_interval, from_end)));

    // Use async function so tokio::time::sleep yields the runtime thread
    vm.create_async_function(move |vm, ()| {
        let state = state.clone();
        async move {
            loop {
                let result = {
                    let mut state = state.lock();

                    // 1. Ensure file is open (if not exists, will return None)
                    state.ensure_open();

                    // 2. Check for rotation (inode/size change)
                    state.check_rotation();

                    // 3. Try to read a line
                    state.read_line()
                };
                // Lock released before await point

                match result {
                    Some(line) => return vm.create_string(&line).map(LuaValue::String),
                    None => {
                        // EOF or file not ready — async sleep, yield runtime thread
                        tokio::time::sleep(Duration::from_millis(poll_interval)).await;
                    }
                }
            }
        }
    })
});
```

**Important:**
- The `parking_lot::Mutex` lock (consistent with the rest of `fs.rs`) is released before the `.await` point to avoid holding it across the async sleep. This prevents deadlocks and allows other Lua coroutines to make progress.
- `fs.tail()` requires the Lua script to be executed via `call_async` (the normal execution path in LMB). The async iterator function will panic if called from a synchronous Lua context. This is the same precondition that applies to `io.read` and other async bindings in LMB.

### `TailState` Struct

```rust
struct TailState {
    path: PathBuf,
    reader: Option<BufReader<File>>,
    inode: Option<u64>,      // Last known inode (None until first successful open)
                             // Uses std::os::unix::fs::MetadataExt::ino() on Unix
    position: u64,           // Current read position
    poll_interval: u64,      // Milliseconds
    from_end: bool,          // Whether to seek to end on first open
}
```

**Initial state:** `inode` starts as `None`. The first successful `open` records the inode without triggering rotation detection. Subsequent opens compare against the stored value — a mismatch means rotation occurred.

**Platform note:** Inode tracking uses `std::os::unix::fs::MetadataExt::ino()` which is Unix-only. On non-Unix platforms, rotation detection falls back to size-only checks (file size shrinking). This is acceptable since LMB's primary target is Linux.

### Integration with Existing Code

| Aspect | Approach |
|--------|----------|
| **Permission check** | Reuses `check_read_permission(path)` at call time. `canonicalize_for_check` already handles non-existent files by canonicalizing the parent and appending the filename. |
| **Method registration** | `add_method("tail", ...)` returns an async closure via `vm.create_async_function`, alongside existing methods in `FsBinding::add_methods` |
| **Line trimming** | Strips trailing `\n` and `\r`, consistent with `fs.lines()` and `FileHandleBinding::read("*l")` |
| **Error handling** | IO errors during read are reported via `LuaError`, consistent with other fs methods |
| **Async runtime** | Uses `tokio::time::sleep` (not `std::thread::sleep`) because LMB runs Lua via `call_async` directly on Tokio worker threads. The Mutex lock is released before each await point. |

### Documentation Update

Add to the module doc comment at top of `fs.rs`:

```
//! - `tail(path, options)` - Follow a file like `tail -F`, returning a line iterator that
//!   yields new lines as they are appended. Automatically follows file rotations.
```

## Testing

All tests in `src/bindings/fs.rs` `mod tests`, using `tempfile` for test fixtures:

1. **Basic read** — Write lines to a file, `tail` with `from = "start"`, verify all lines are yielded
2. **Wait for new lines** — Tail an existing file (hits EOF), spawn a thread that writes new lines after a delay, verify the new lines are received
3. **Rotation detection** — Tail a file, rename it, create a new file at the same path, write to the new file, verify tail follows the new file
4. **File does not exist** — Tail a non-existent path, spawn a thread that creates the file after a delay, verify lines are received once the file appears
5. **Break exits cleanly** — Tail a file, break after N lines, verify no panic or resource leak
6. **Permission denied** — Tail a path without read permission, verify error is returned

## Configuration Reference

No new CLI flags or environment variables. Configuration is per-call via the options table.
