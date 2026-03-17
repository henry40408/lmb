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
| `break` in for loop | Exit loop normally, no cleanup needed |
| Permission denied | Error at call time (before iterator starts), consistent with `fs.lines()` |

## Rotation Detection

On each poll cycle (when EOF is reached and before sleeping):

1. Stat the path to get current inode and file size
2. If inode differs from the last known inode → file has been rotated → reopen and read from beginning
3. If file size is smaller than current read position → file has been truncated → seek to beginning
4. If stat fails (file gone) → enter waiting mode, keep polling until file reappears

This matches the behavior of `tail -F` (capital F) in GNU coreutils.

## Implementation

### Approach: Polling with Sleep

Uses a synchronous polling loop inside the iterator closure. This matches the existing `fs.lines()` pattern (synchronous `add_method`, closure returning lines).

**Why polling over inotify/notify:**
- No new dependencies, no impact on binary size (LMB targets lightweight deployments)
- Consistent behavior across all platforms and filesystems (including NFS where inotify does not work)
- 100ms polling interval has negligible CPU cost (10 syscalls/second when idle)
- GNU `tail -f` itself uses polling as its default strategy

### Rust Implementation

New method added to `FsBinding::add_methods` in `src/bindings/fs.rs`:

```rust
methods.add_method("tail", |vm, this, (path, options): (String, Option<LuaTable>)| {
    this.check_read_permission(&path).map_err(LuaError::runtime)?;

    let poll_interval = /* extract from options, default 100 */;
    let from_end = /* extract from options, default true */;

    let state = Arc::new(Mutex::new(TailState::new(path, poll_interval, from_end)));

    vm.create_function(move |vm, ()| {
        let mut state = state.lock();
        loop {
            // 1. Ensure file is open (wait if not exists)
            state.ensure_open();

            // 2. Check for rotation (inode/size change)
            state.check_rotation();

            // 3. Try to read a line
            match state.read_line() {
                Some(line) => return vm.create_string(&line).map(LuaValue::String),
                None => {
                    // EOF — sleep and retry
                    std::thread::sleep(Duration::from_millis(state.poll_interval));
                }
            }
        }
    })
});
```

### `TailState` Struct

```rust
struct TailState {
    path: PathBuf,
    reader: Option<BufReader<File>>,
    inode: u64,              // Last known inode (via std::os::unix::fs::MetadataExt on Unix)
    position: u64,           // Current read position
    poll_interval: u64,      // Milliseconds
    from_end: bool,          // Whether to seek to end on first open
}
```

**Platform note:** Inode tracking uses `std::os::unix::fs::MetadataExt::ino()` which is Unix-only. On non-Unix platforms, rotation detection falls back to size-only checks (file size shrinking). This is acceptable since LMB's primary target is Linux.

### Integration with Existing Code

| Aspect | Approach |
|--------|----------|
| **Permission check** | Reuses `check_read_permission()` at call time, same as `fs.lines()` |
| **Method registration** | `add_method("tail", ...)` alongside existing methods in `FsBinding::add_methods` |
| **Line trimming** | Strips trailing `\n` and `\r`, consistent with `fs.lines()` and `FileHandleBinding::read("*l")` |
| **Error handling** | IO errors during read are reported via `LuaError`, consistent with other fs methods |
| **Thread blocking** | Uses `std::thread::sleep` in the polling loop. This blocks the current thread, which is acceptable because LMB runs Lua scripts on dedicated threads (or blocking tasks). The Tokio async runtime is not blocked. |

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
