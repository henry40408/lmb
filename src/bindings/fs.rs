//! Filesystem binding module.
//!
//! This module provides filesystem I/O operations for reading and writing files.
//! Import via `require("@lmb/fs")`.
//!
//! All operations require explicit permissions via `--allow-read` and `--allow-write`
//! CLI flags. Without permissions, all filesystem operations are denied.
//!
//! # Available Methods
//!
//! - `open(path, mode)` - Open a file and return a file handle. Returns `(handle, nil)` on
//!   success or `(nil, error)` on failure.
//! - `lines(path)` - Open a file and return a line iterator.
//! - `type(obj)` - Check if a value is a file handle. Returns `"file"`, `"closed file"`, or `nil`.
//! - `remove(path)` - Remove a file.
//! - `rename(old, new)` - Rename a file.
//! - `exists(path)` - Check if a path exists.
//! - `list(path)` - List directory contents, returning a table of filenames.
//!
//! # File Handle Methods
//!
//! - `read(fmt)` - Read from file. Supports `"*a"` (all), `"*l"` (line), `"*n"` (number),
//!   and `N` (N bytes).
//! - `write(...)` - Write strings to file.
//! - `lines()` - Return a line iterator.
//! - `seek(whence, offset)` - Seek to position. `whence` is `"set"`, `"cur"`, or `"end"`.
//! - `flush()` - Flush buffered writes.
//! - `close()` - Close the file handle.
//!
//! # Modes
//!
//! - `"r"` - Read only (default)
//! - `"w"` - Write only (creates/truncates)
//! - `"a"` - Append only (creates if needed)
//! - `"r+"` - Read and write
//! - `"w+"` - Read and write (creates/truncates)
//! - `"a+"` - Read and append (creates if needed)
//!
//! # Example
//!
//! ```lua
//! local fs = require("@lmb/fs")
//!
//! -- Write to a file
//! local f = fs.open("/tmp/hello.txt", "w")
//! f:write("hello world\n")
//! f:close()
//!
//! -- Read from a file
//! local f = fs.open("/tmp/hello.txt", "r")
//! local content = f:read("*a")
//! f:close()
//!
//! -- Iterate over lines
//! for line in fs.lines("/tmp/hello.txt") do
//!     print(line)
//! end
//!
//! -- Check existence and list directory
//! if fs.exists("/tmp") then
//!     local entries = fs.list("/tmp")
//!     for _, name in ipairs(entries) do
//!         print(name)
//!     end
//! end
//! ```

use std::{
    fmt,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use bon::bon;
use mlua::prelude::*;
use parking_lot::Mutex;

use crate::Permissions;

/// A file handle wrapping a buffered reader/writer.
///
/// The inner `Option` tracks closed state: `None` means the handle has been closed.
pub(crate) struct FileHandleBinding {
    inner: Arc<Mutex<Option<BufReader<File>>>>,
    path: String,
}

impl fmt::Debug for FileHandleBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.inner.lock().is_some() {
            "open"
        } else {
            "closed"
        };
        f.debug_struct("FileHandleBinding")
            .field("path", &self.path)
            .field("state", &state)
            .finish()
    }
}

impl LuaUserData for FileHandleBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |vm, this, fmt: LuaValue| {
            let mut guard = this.inner.lock();
            let reader = guard
                .as_mut()
                .ok_or_else(|| LuaError::runtime("attempt to use a closed file"))?;

            if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                match &*f {
                    "*a" | "*all" => {
                        let mut buf = String::new();
                        reader.read_to_string(&mut buf).into_lua_err()?;
                        return vm.create_string(&buf).map(LuaValue::String);
                    }
                    "*l" | "*line" => {
                        let mut line = String::new();
                        let n = reader.read_line(&mut line).into_lua_err()?;
                        if n == 0 {
                            return Ok(LuaNil);
                        }
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                        return vm.create_string(trimmed).map(LuaValue::String);
                    }
                    "*n" | "*number" => {
                        let mut line = String::new();
                        let n = reader.read_line(&mut line).into_lua_err()?;
                        if n == 0 {
                            return Ok(LuaNil);
                        }
                        return match line.trim().parse::<f64>() {
                            Ok(num) => Ok(LuaValue::Number(num)),
                            Err(_) => Ok(LuaNil),
                        };
                    }
                    _ => {
                        return Err(LuaError::runtime(format!("invalid format '{f}'")));
                    }
                }
            }

            if let Some(n) = fmt.as_usize() {
                let mut buf = vec![0u8; n];
                let read = reader.read(&mut buf).into_lua_err()?;
                if read == 0 {
                    return Ok(LuaNil);
                }
                buf.truncate(read);
                return vm.create_string(&buf).map(LuaValue::String);
            }

            Err(LuaError::runtime(format!("invalid option {fmt:?}")))
        });

        methods.add_method("write", |_, this, values: LuaMultiValue| {
            let mut guard = this.inner.lock();
            let reader = guard
                .as_mut()
                .ok_or_else(|| LuaError::runtime("attempt to use a closed file"))?;
            let file = reader.get_mut();
            for value in &values {
                match value {
                    LuaValue::String(s) => {
                        file.write_all(&s.as_bytes()).into_lua_err()?;
                    }
                    LuaValue::Integer(n) => {
                        write!(file, "{n}").into_lua_err()?;
                    }
                    LuaValue::Number(n) => {
                        write!(file, "{n}").into_lua_err()?;
                    }
                    _ => {
                        return Err(LuaError::runtime(format!(
                            "invalid value for write: {value:?}"
                        )));
                    }
                }
            }
            Ok(())
        });

        methods.add_method("lines", |vm, this, ()| {
            let inner = this.inner.clone();
            let f = vm.create_function(move |vm, ()| {
                let mut guard = inner.lock();
                let reader = guard
                    .as_mut()
                    .ok_or_else(|| LuaError::runtime("attempt to use a closed file"))?;
                let mut line = String::new();
                let n = reader.read_line(&mut line).into_lua_err()?;
                if n == 0 {
                    return Ok(LuaNil);
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                vm.create_string(trimmed).map(LuaValue::String)
            })?;
            Ok(f)
        });

        methods.add_method(
            "seek",
            |_, this, (whence, offset): (Option<String>, Option<i64>)| {
                let mut guard = this.inner.lock();
                let reader = guard
                    .as_mut()
                    .ok_or_else(|| LuaError::runtime("attempt to use a closed file"))?;
                let whence_str = whence.as_deref().unwrap_or("cur");
                let offset = offset.unwrap_or(0);
                let pos = match whence_str {
                    "set" => SeekFrom::Start(u64::try_from(offset).into_lua_err()?),
                    "cur" => SeekFrom::Current(offset),
                    "end" => SeekFrom::End(offset),
                    _ => return Err(LuaError::runtime(format!("invalid whence '{whence_str}'"))),
                };
                let new_pos = reader.seek(pos).into_lua_err()?;
                Ok(new_pos)
            },
        );

        methods.add_method("flush", |_, this, ()| {
            let mut guard = this.inner.lock();
            let reader = guard
                .as_mut()
                .ok_or_else(|| LuaError::runtime("attempt to use a closed file"))?;
            reader.get_mut().flush().into_lua_err()?;
            Ok(())
        });

        methods.add_method("close", |_, this, ()| {
            let mut guard = this.inner.lock();
            if guard.is_none() {
                return Err(LuaError::runtime("attempt to use a closed file"));
            }
            *guard = None;
            Ok(())
        });
    }
}

/// Filesystem binding providing file I/O operations.
pub(crate) struct FsBinding {
    permissions: Option<Permissions>,
}

impl fmt::Debug for FsBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FsBinding")
            .field("permissions", &self.permissions)
            .finish()
    }
}

#[bon]
impl FsBinding {
    #[builder]
    pub(crate) fn new(permissions: Option<Permissions>) -> Self {
        Self { permissions }
    }

    /// Canonicalize a path for permission checking.
    /// For existing paths, uses `std::fs::canonicalize`.
    /// For non-existing paths, canonicalizes the parent directory and appends the file name.
    fn canonicalize_for_check(path: &Path) -> std::io::Result<PathBuf> {
        if path.exists() {
            std::fs::canonicalize(path)
        } else if let Some(parent) = path.parent() {
            let canonical_parent = if parent.as_os_str().is_empty() {
                std::fs::canonicalize(".")?
            } else {
                std::fs::canonicalize(parent)?
            };
            if let Some(file_name) = path.file_name() {
                Ok(canonical_parent.join(file_name))
            } else {
                Ok(canonical_parent)
            }
        } else {
            std::fs::canonicalize(path)
        }
    }

    /// Check read permission for a path.
    /// Returns the canonicalized path on success.
    fn check_read_permission(&self, path: &str) -> Result<PathBuf, String> {
        let canonical =
            Self::canonicalize_for_check(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        match &self.permissions {
            None => Err(format!(
                "{path}: permission denied (no permissions granted)"
            )),
            Some(perm) => {
                if perm.is_read_allowed(&canonical) {
                    Ok(canonical)
                } else {
                    Err(format!("{path}: read permission denied"))
                }
            }
        }
    }

    /// Check write permission for a path.
    /// Returns the canonicalized path on success.
    fn check_write_permission(&self, path: &str) -> Result<PathBuf, String> {
        let canonical =
            Self::canonicalize_for_check(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        match &self.permissions {
            None => Err(format!(
                "{path}: permission denied (no permissions granted)"
            )),
            Some(perm) => {
                if perm.is_write_allowed(&canonical) {
                    Ok(canonical)
                } else {
                    Err(format!("{path}: write permission denied"))
                }
            }
        }
    }
}

/// Determine if a mode string requires read permission
fn mode_needs_read(mode: &str) -> bool {
    matches!(mode, "r" | "r+" | "w+" | "a+")
}

/// Determine if a mode string requires write permission
fn mode_needs_write(mode: &str) -> bool {
    matches!(mode, "w" | "a" | "r+" | "w+" | "a+")
}

impl LuaUserData for FsBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "open",
            |vm, this, (path, mode): (String, Option<String>)| {
                let mode = mode.as_deref().unwrap_or("r");

                // Check permissions based on mode
                if mode_needs_read(mode)
                    && let Err(e) = this.check_read_permission(&path)
                {
                    return Ok((LuaNil, LuaValue::String(vm.create_string(&e)?)));
                }
                if mode_needs_write(mode)
                    && let Err(e) = this.check_write_permission(&path)
                {
                    return Ok((LuaNil, LuaValue::String(vm.create_string(&e)?)));
                }

                let file = match mode {
                    "r" => File::open(&path),
                    "w" => File::create(&path),
                    "a" => OpenOptions::new().append(true).create(true).open(&path),
                    "r+" => OpenOptions::new().read(true).write(true).open(&path),
                    "w+" => OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&path),
                    "a+" => OpenOptions::new()
                        .read(true)
                        .append(true)
                        .create(true)
                        .open(&path),
                    _ => {
                        let msg = format!("invalid mode '{mode}'");
                        return Ok((LuaNil, LuaValue::String(vm.create_string(&msg)?)));
                    }
                };

                match file {
                    Ok(f) => {
                        let handle = FileHandleBinding {
                            inner: Arc::new(Mutex::new(Some(BufReader::new(f)))),
                            path: path.clone(),
                        };
                        Ok((LuaValue::UserData(vm.create_userdata(handle)?), LuaNil))
                    }
                    Err(e) => {
                        let msg = format!("{path}: {e}");
                        Ok((LuaNil, LuaValue::String(vm.create_string(&msg)?)))
                    }
                }
            },
        );

        methods.add_method("lines", |vm, this, path: String| {
            let canonical = this
                .check_read_permission(&path)
                .map_err(LuaError::runtime)?;
            let file = File::open(&canonical).into_lua_err()?;
            let inner = Arc::new(Mutex::new(Some(BufReader::new(file))));
            let f = vm.create_function(move |vm, ()| {
                let mut guard = inner.lock();
                let reader = match guard.as_mut() {
                    Some(r) => r,
                    None => return Ok(LuaNil),
                };
                let mut line = String::new();
                let n = reader.read_line(&mut line).into_lua_err()?;
                if n == 0 {
                    // Close the file when we reach EOF
                    *guard = None;
                    return Ok(LuaNil);
                }
                let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                vm.create_string(trimmed).map(LuaValue::String)
            })?;
            Ok(f)
        });

        methods.add_function("type", |_, obj: LuaValue| match obj {
            LuaValue::UserData(ud) => {
                if ud.is::<FileHandleBinding>() {
                    let handle = ud.borrow::<FileHandleBinding>().into_lua_err()?;
                    let guard = handle.inner.lock();
                    if guard.is_some() {
                        Ok(Some("file".to_string()))
                    } else {
                        Ok(Some("closed file".to_string()))
                    }
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        });

        methods.add_method("remove", |_, this, path: String| {
            this.check_write_permission(&path)
                .map_err(LuaError::runtime)?;
            std::fs::remove_file(&path).into_lua_err()?;
            Ok(())
        });

        methods.add_method("rename", |_, this, (old, new): (String, String)| {
            this.check_write_permission(&old)
                .map_err(LuaError::runtime)?;
            this.check_write_permission(&new)
                .map_err(LuaError::runtime)?;
            std::fs::rename(&old, &new).into_lua_err()?;
            Ok(())
        });

        methods.add_method("exists", |_, this, path: String| {
            match this.check_read_permission(&path) {
                Ok(canonical) => Ok(canonical.exists()),
                // If the file doesn't exist, canonicalize may fail, but that's ok
                Err(_) => Ok(false),
            }
        });

        methods.add_method("list", |vm, this, path: String| {
            let canonical = this
                .check_read_permission(&path)
                .map_err(LuaError::runtime)?;
            let entries = std::fs::read_dir(&canonical).into_lua_err()?;
            let table = vm.create_table()?;
            let mut i = 1;
            for entry in entries {
                let entry = entry.into_lua_err()?;
                let name = entry.file_name().to_string_lossy().to_string();
                table.set(i, name)?;
                i += 1;
            }
            Ok(table)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use serde_json::json;
    use tempfile::{NamedTempFile, TempDir};
    use tokio::io::empty;

    use crate::{
        Runner, State,
        permission::{
            EnvPermissions, NetPermissions, Permissions, ReadPermissions, WritePermissions,
        },
    };

    /// Create a `Permissions::Some` with specific read/write permissions and permissive env/net.
    fn fs_permissions(read: ReadPermissions, write: WritePermissions) -> Permissions {
        Permissions::Some {
            env: EnvPermissions::All {
                denied: Default::default(),
            },
            net: NetPermissions::All {
                denied: Default::default(),
            },
            read,
            write,
        }
    }

    #[tokio::test]
    async fn test_open_read() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "hello world").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-read.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("hello world"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_open_write() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-write.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("hello from lua"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_open_append() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "first").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-append.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("firstsecond"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_read_line() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "line1\nline2\nline3").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-line.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(
            json!(["line1", "line2", "line3"]),
            result.result.expect("result")
        );
    }

    #[tokio::test]
    async fn test_read_bytes() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "abcdefghij").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-bytes.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("abcde"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_lines() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "a\nb\nc").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/lines.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(["a", "b", "c"]), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_lines_shorthand() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "x\ny\nz").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/lines-shorthand.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(["x", "y", "z"]), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_seek() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "abcdefghij").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/seek.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("fghij"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_type() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "test").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/type.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
    }

    #[tokio::test]
    async fn test_remove() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();
        // Keep the path but drop the handle so the file still exists
        let path_buf = tmp.path().to_path_buf();
        drop(tmp);
        // Recreate file since NamedTempFile deletes on drop
        std::fs::write(&path_buf, "to delete").expect("write file");

        let source = include_str!("../fixtures/bindings/fs/remove.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
        assert!(!path_buf.exists(), "file should be deleted");
    }

    #[tokio::test]
    async fn test_rename() {
        let dir = TempDir::new().expect("create temp dir");
        let old_path = dir.path().join("old.txt");
        let new_path = dir.path().join("new.txt");
        std::fs::write(&old_path, "content").expect("write file");

        let source = include_str!("../fixtures/bindings/fs/rename.lua");
        let canonical_dir = std::fs::canonicalize(dir.path()).expect("canonicalize");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [canonical_dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder()
            .state(json!({
                "old": old_path.to_string_lossy(),
                "new": new_path.to_string_lossy(),
            }))
            .build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
        assert!(!old_path.exists(), "old file should not exist");
        assert!(new_path.exists(), "new file should exist");
        assert_eq!(std::fs::read_to_string(&new_path).expect("read"), "content");
    }

    #[tokio::test]
    async fn test_exists() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/exists.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
    }

    #[tokio::test]
    async fn test_list() {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::write(dir.path().join("a.txt"), "").expect("write a");
        std::fs::write(dir.path().join("b.txt"), "").expect("write b");
        let path = dir.path().to_string_lossy().to_string();
        let canonical_dir = std::fs::canonicalize(dir.path()).expect("canonicalize");

        let source = include_str!("../fixtures/bindings/fs/list.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [canonical_dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
    }

    #[tokio::test]
    async fn test_permission_denied() {
        let source = include_str!("../fixtures/bindings/fs/permission-denied.lua");
        // No permissions granted
        let runner = Runner::builder(source, empty())
            .build()
            .expect("build runner");
        runner
            .invoke()
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
    }

    #[tokio::test]
    async fn test_closed_file() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "test").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/closed-file.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        runner
            .invoke()
            .state(state)
            .call()
            .await
            .expect("invoke")
            .result
            .expect("result");
    }

    #[tokio::test]
    async fn test_read_number() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "42.5\n").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-number.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(42.5), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_read_invalid_format() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "test").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-invalid-format.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(msg.as_str().expect("string").contains("invalid format"));
    }

    #[tokio::test]
    async fn test_write_number() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/write-number.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("423.14"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_write_invalid() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/write-invalid.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(msg.as_str().expect("string").contains("invalid value"));
    }

    #[tokio::test]
    async fn test_flush() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/flush.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("flushed"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_seek_cur_end() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "abcdefghij").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/seek-cur-end.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(
            json!({"size": 10, "rest": "cdefghij"}),
            result.result.expect("result")
        );
    }

    #[tokio::test]
    async fn test_open_rw() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "abcde").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-rw.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(
            json!({"original": "abcde", "modified": "XXcde"}),
            result.result.expect("result")
        );
    }

    #[tokio::test]
    async fn test_open_wp() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-wp.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("hello w+"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_open_ap() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "initial").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-ap.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir.clone()].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("initial appended"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_open_invalid_mode() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/open-invalid-mode.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(msg.as_str().expect("string").contains("invalid mode"));
    }

    #[tokio::test]
    async fn test_open_nonexistent() {
        let dir = TempDir::new().expect("create temp dir");
        let path = dir.path().to_string_lossy().to_string();
        let canonical_dir = std::fs::canonicalize(dir.path()).expect("canonicalize");

        let source = include_str!("../fixtures/bindings/fs/open-nonexistent.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [canonical_dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(true), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_write_permission_denied() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/write-permission-denied.lua");
        // Grant read but not write
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(msg.as_str().expect("string").contains("permission denied"));
    }

    #[tokio::test]
    async fn test_write_no_permissions() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();

        let source = include_str!("../fixtures/bindings/fs/write-no-permissions.lua");
        // No permissions at all
        let runner = Runner::builder(source, empty())
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(
            msg.as_str()
                .expect("string")
                .contains("no permissions granted")
        );
    }

    #[tokio::test]
    async fn test_read_number_eof() {
        let tmp = NamedTempFile::new().expect("create temp file");
        // Empty file - no content written
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-number-eof.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(true), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_read_number_nan() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "not a number\n").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-number-nan.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(true), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_read_bytes_eof() {
        let tmp = NamedTempFile::new().expect("create temp file");
        // Empty file
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-bytes-eof.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!(true), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_seek_invalid_whence() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "test").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/seek-invalid.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(msg.as_str().expect("string").contains("invalid whence"));
    }

    #[tokio::test]
    async fn test_read_bytes_partial() {
        let mut tmp = NamedTempFile::new().expect("create temp file");
        write!(tmp, "abc").expect("write temp file");
        let path = tmp.path().to_string_lossy().to_string();
        let dir = tmp.path().parent().expect("parent dir").to_path_buf();

        let source = include_str!("../fixtures/bindings/fs/read-bytes-partial.lua");
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: [dir].into_iter().collect(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        assert_eq!(json!("abc"), result.result.expect("result"));
    }

    #[tokio::test]
    async fn test_read_permission_denied_some() {
        let tmp = NamedTempFile::new().expect("create temp file");
        let path = tmp.path().to_string_lossy().to_string();

        let source = include_str!("../fixtures/bindings/fs/read-permission-denied-some.lua");
        // Has permissions but the path is not in allowed set
        let perm = fs_permissions(
            ReadPermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
            WritePermissions::Some {
                allowed: Default::default(),
                denied: Default::default(),
            },
        );
        let runner = Runner::builder(source, empty())
            .permissions(perm)
            .build()
            .expect("build runner");
        let state = State::builder().state(json!(path)).build();
        let result = runner.invoke().state(state).call().await.expect("invoke");
        let msg = result.result.expect("result");
        assert!(
            msg.as_str()
                .expect("string")
                .contains("read permission denied")
        );
    }
}
