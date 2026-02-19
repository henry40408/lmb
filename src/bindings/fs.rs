//! File system binding module.
//!
//! This module provides file system operations for reading, writing, and inspecting files.
//! All operations are gated by the permission system (`--allow-fs`, `--deny-fs`).
//! Import via `require("@lmb/fs")`.
//!
//! # Low-level API (UNIX-style)
//!
//! - `open(path, mode)` - Open a file, returns a file handle. Mode: `"r"`, `"w"`, `"a"`.
//! - `stat(path)` - Get file metadata. Returns `{ size, is_file, is_dir }`.
//! - `remove(path)` - Remove a file.
//! - `mkdir(path)` - Create a directory.
//! - `readdir(path)` - List directory entries. Returns an array of names.
//!
//! ## File Handle Methods
//!
//! - `read(fmt)` - Read from file. Format: `"*a"` (all), `"*l"` (line), or number (bytes).
//! - `write(data)` - Write string to file.
//! - `close()` - Close the file handle.
//!
//! # High-level Wrappers
//!
//! - `read_file(path)` - Read entire file as string.
//! - `write_file(path, content)` - Write string to file (overwrite).
//! - `exists(path)` - Check if path exists.
//!
//! # Example
//!
//! ```lua
//! local fs = require("@lmb/fs")
//!
//! -- High-level: read and write files
//! fs:write_file("output.txt", "hello world")
//! local content = fs:read_file("output.txt")
//!
//! -- Low-level: open, read line by line, close
//! local f = fs:open("output.txt", "r")
//! local line = f:read("*l")
//! f:close()
//!
//! -- Check existence and metadata
//! if fs:exists("output.txt") then
//!     local info = fs:stat("output.txt")
//!     print(info.size)
//! end
//!
//! -- Directory operations
//! fs:mkdir("mydir")
//! local entries = fs:readdir(".")
//! fs:remove("output.txt")
//! ```

use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    path::PathBuf,
    sync::Arc,
};

use mlua::prelude::*;
use parking_lot::Mutex;

use crate::Permissions;

/// File handle returned by `fs:open()`.
struct FileHandle {
    inner: Arc<Mutex<Option<FileHandleInner>>>,
}

enum FileHandleInner {
    Reader(BufReader<std::fs::File>),
    Writer(std::fs::File),
}

impl FileHandle {
    fn with_inner<F, T>(inner: &Arc<Mutex<Option<FileHandleInner>>>, f: F) -> LuaResult<T>
    where
        F: FnOnce(&mut FileHandleInner) -> LuaResult<T>,
    {
        let mut guard = inner.lock();
        match guard.as_mut() {
            Some(handle) => f(handle),
            None => Err(LuaError::runtime("file handle is closed")),
        }
    }
}

impl LuaUserData for FileHandle {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |vm, this, fmt: LuaValue| {
            FileHandle::with_inner(&this.inner, |handle| {
                let FileHandleInner::Reader(reader) = handle else {
                    return Err(LuaError::runtime(
                        "cannot read from a file opened for writing",
                    ));
                };

                if let Some(s) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                    match &*s {
                        "*a" | "*all" => {
                            let mut buf = String::new();
                            reader.read_to_string(&mut buf).into_lua_err()?;
                            return vm.to_value(&buf);
                        }
                        "*l" | "*line" => {
                            let mut line = String::new();
                            if reader.read_line(&mut line).into_lua_err()? == 0 {
                                return Ok(LuaNil);
                            }
                            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
                            return vm.to_value(trimmed);
                        }
                        _ => {
                            return Err(LuaError::runtime(format!("invalid read format: {s}")));
                        }
                    }
                }

                if let Some(n) = fmt.as_usize() {
                    let mut buf = vec![0u8; n];
                    let bytes_read = reader.read(&mut buf).into_lua_err()?;
                    if bytes_read == 0 {
                        return Ok(LuaNil);
                    }
                    buf.truncate(bytes_read);
                    let s = String::from_utf8(buf).into_lua_err()?;
                    return vm.to_value(&s);
                }

                Err(LuaError::runtime(format!("invalid read format: {fmt:?}")))
            })
        });

        methods.add_method("write", |_, this, data: String| {
            FileHandle::with_inner(&this.inner, |handle| {
                let FileHandleInner::Writer(file) = handle else {
                    return Err(LuaError::runtime(
                        "cannot write to a file opened for reading",
                    ));
                };
                let bytes = data.as_bytes();
                file.write_all(bytes).into_lua_err()?;
                Ok(bytes.len())
            })
        });

        methods.add_method("close", |_, this, ()| {
            let mut guard = this.inner.lock();
            *guard = None;
            Ok(())
        });
    }
}

pub(crate) struct FsBinding {
    permissions: Option<Permissions>,
}

impl FsBinding {
    pub(crate) fn new(permissions: Option<Permissions>) -> Self {
        Self { permissions }
    }

    fn check_permission(&self, path: &std::path::Path) -> LuaResult<()> {
        if let Some(perm) = &self.permissions
            && !perm.is_path_allowed(path)
        {
            return Err(LuaError::runtime(format!(
                "path is not allowed: {}",
                path.display()
            )));
        }
        Ok(())
    }
}

impl LuaUserData for FsBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // -- Low-level API --

        methods.add_method("open", |_, this, (path, mode): (String, String)| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;

            let inner = match mode.as_str() {
                "r" => {
                    let file = std::fs::File::open(&path).into_lua_err()?;
                    FileHandleInner::Reader(BufReader::new(file))
                }
                "w" => {
                    let file = std::fs::File::create(&path).into_lua_err()?;
                    FileHandleInner::Writer(file)
                }
                "a" => {
                    let file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .into_lua_err()?;
                    FileHandleInner::Writer(file)
                }
                _ => {
                    return Err(LuaError::runtime(format!(
                        "invalid file mode: {mode} (expected \"r\", \"w\", or \"a\")"
                    )));
                }
            };

            Ok(FileHandle {
                inner: Arc::new(Mutex::new(Some(inner))),
            })
        });

        methods.add_method("stat", |vm, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;

            let metadata = std::fs::metadata(&path).into_lua_err()?;
            let table = vm.create_table()?;
            table.set("size", metadata.len())?;
            table.set("is_file", metadata.is_file())?;
            table.set("is_dir", metadata.is_dir())?;
            Ok(LuaValue::Table(table))
        });

        methods.add_method("remove", |_, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;
            std::fs::remove_file(&path).into_lua_err()?;
            Ok(())
        });

        methods.add_method("mkdir", |_, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;
            std::fs::create_dir(&path).into_lua_err()?;
            Ok(())
        });

        methods.add_method("readdir", |vm, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;

            let entries = std::fs::read_dir(&path).into_lua_err()?;
            let table = vm.create_table()?;
            let mut idx = 1;
            for entry in entries {
                let entry = entry.into_lua_err()?;
                if let Some(name) = entry.file_name().to_str() {
                    table.set(idx, name.to_string())?;
                    idx += 1;
                }
            }
            Ok(LuaValue::Table(table))
        });

        // -- High-level wrappers --

        methods.add_method("read_file", |_, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;
            std::fs::read_to_string(&path).into_lua_err()
        });

        methods.add_method(
            "write_file",
            |_, this, (path, content): (String, String)| {
                let path = PathBuf::from(&path);
                this.check_permission(&path)?;
                std::fs::write(&path, &content).into_lua_err()?;
                Ok(content.len())
            },
        );

        methods.add_method("exists", |_, this, path: String| {
            let path = PathBuf::from(&path);
            this.check_permission(&path)?;
            Ok(path.exists())
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;
    use crate::permission::{FsPermissions, Permissions};

    fn fs_permissions_for_tmp() -> Permissions {
        use crate::permission::{EnvPermissions, NetPermissions};
        use rustc_hash::FxHashSet;

        let canonical = std::path::PathBuf::from("/tmp")
            .canonicalize()
            .expect("canonicalize /tmp");
        Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: [canonical].into_iter().collect(),
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        }
    }

    #[tokio::test]
    async fn test_fs() {
        let source = include_str!("../fixtures/bindings/fs.lua");
        let permissions = fs_permissions_for_tmp();
        let runner = Runner::builder(source, empty())
            .permissions(permissions)
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
    async fn test_fs_permission_denied() {
        let source = r#"
            local fs = require("@lmb/fs")
            return fs:read_file("/etc/passwd")
        "#;
        use crate::permission::{EnvPermissions, NetPermissions};
        use rustc_hash::FxHashSet;

        let permissions = Permissions::Some {
            env: EnvPermissions::All {
                denied: FxHashSet::default(),
            },
            fs: FsPermissions::Some {
                allowed: FxHashSet::default(),
                denied: FxHashSet::default(),
            },
            net: NetPermissions::All {
                denied: FxHashSet::default(),
            },
        };
        let runner = Runner::builder(source, empty())
            .permissions(permissions)
            .build()
            .expect("build runner");
        let result = runner.invoke().call().await.expect("invoke");
        assert!(result.result.is_err(), "expected permission denied error");
    }
}
