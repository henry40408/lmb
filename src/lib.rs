#![deny(missing_debug_implementations, missing_docs)]

//! A library for running Lua scripts.

use std::{
    error::Error,
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use bon::{bon, Builder};
use mlua::{prelude::*, AsChunk};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde_json::{json, Value};
use thiserror::Error;
use tokio::io::AsyncRead;
use tracing::{debug_span, Instrument};

use crate::{
    bindings::{store::StoreBinding, Binding},
    permission::Permissions,
    reader::SharedReader,
    stmt::MIGRATIONS,
};

/// Error handling module
pub mod error;

/// Permission module
pub mod permission;

/// Pool module
pub mod pool;

/// Reader module
pub mod reader;

/// Store module
pub mod store;

mod bindings;
mod stmt;

/// Represents a timeout error when executing a Lua script
#[derive(Clone, Debug)]
pub struct Timeout {
    elapsed: Duration,
    timeout: Duration,
}

impl fmt::Display for Timeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Lua script execution timed out after {:?}, timeout was {:?}",
            self.elapsed, self.timeout
        )
    }
}

impl Error for Timeout {}

/// Represents the state of the Lua script execution
#[derive(Builder, Debug)]
pub struct State {
    request: Option<Value>,
    state: Option<Value>,
}

/// Lua VM error handling
#[derive(Debug, Error)]
pub enum LmbError {
    /// Error reading from the input stream
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Error thrown by the Lua VM
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    /// Error converting a Lua value to a Rust type
    #[error("Lua value as error: {0}")]
    LuaValue(Value),
    /// Error decoding ``MessagePack`` data
    #[error("MessagePack decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    /// Error encoding ``MessagePack`` data
    #[error("MessagePack encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    /// Error from reqwest crate
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    /// Error from rusqlite crate
    #[error("SQLite error: {0}")]
    SQLite(#[from] rusqlite::Error),
    /// Error when the Lua script times out
    #[error("Timeout: {0}")]
    Timeout(#[from] Timeout),
}

/// Type alias for the shared reader used in the library.
pub type LmbInput = Arc<SharedReader>;
type LmbStore = Arc<Mutex<Connection>>;

/// Result type for the library
pub type LmbResult<T> = Result<T, LmbError>;

/// Represents the result of invoking a Lua function
#[derive(Builder, Debug)]
pub struct Invoked {
    /// The elapsed time since the invocation started
    pub elapsed: Duration,
    /// The result of the Lua function invocation
    pub result: LmbResult<Value>,
    /// The amount of memory used in bytes by the Lua VM during the invocation
    pub used_memory: usize,
}

/// A runner for executing Lua scripts with an input stream
#[derive(Debug)]
pub struct Runner {
    func: LuaFunction,
    reader: LmbInput,
    store: Option<LmbStore>,
    timeout: Option<Duration>,
    vm: Lua,
}

static WRAP_FUNC: &str = r"return function(f, ctx) return pcall(f, ctx) end";

#[bon]
impl Runner {
    /// Creates a new Lua runner with the given source code and input reader.
    #[builder]
    pub fn new<S, R>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: R,
        #[builder(into)] default_name: Option<String>,
        http_timeout: Option<Duration>,
        permissions: Option<Permissions>,
        store: Option<Connection>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsChunk + Clone,
        R: AsyncRead + Send + Unpin + 'static,
    {
        let reader = Arc::new(SharedReader::new(reader));
        let store = store.map(|s| Arc::new(Mutex::new(s)));
        Self::from_shared_reader(source, reader)
            .maybe_default_name(default_name)
            .maybe_http_timeout(http_timeout)
            .maybe_permissions(permissions)
            .maybe_store(store)
            .maybe_timeout(timeout)
            .call()
    }

    /// Creates a new Lua runner with the given source code and shared reader.
    #[builder]
    pub fn from_shared_reader<S>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: LmbInput,
        #[builder(into)] default_name: Option<String>,
        http_timeout: Option<Duration>,
        permissions: Option<Permissions>,
        store: Option<LmbStore>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsChunk + Clone,
    {
        let vm = Lua::new();
        vm.sandbox(true)?;

        let default_name = default_name.unwrap_or_else(|| "-".to_string());
        let default_name = format!("={default_name}");
        let source_name = &source.name();
        let (vm, func) = {
            let _ = debug_span!("load_source").entered();
            // First, we try to evaluate the source code as an expression.
            let mut chunk = vm.load(source.clone());
            if source_name.is_none() {
                chunk = chunk.set_name(&default_name);
            }
            if let Ok(LuaValue::Function(func)) = chunk.eval() {
                // If it evaluates to a function, the function will be extracted.
                (vm, func)
            } else {
                // Otherwise, we assume it is a chunk of script and load it as a function.
                // Expressions will fail to load as a function if they call functions,
                // since modules are not registered yet.
                let vm = Lua::new();
                vm.sandbox(true)?;
                let mut chunk = vm.load(source);
                if source_name.is_none() {
                    chunk = chunk.set_name(&default_name);
                }
                let func = chunk.into_function()?;
                (vm, func)
            }
        };
        {
            let _ = debug_span!("register_modules").entered();
            vm.register_module(
                "@lmb",
                Binding::builder(reader.clone())
                    .maybe_permissions(permissions.clone())
                    .build(),
            )?;
            vm.register_module("@lmb/coroutine", bindings::coroutine::CoroutineBinding {})?;
            vm.register_module("@lmb/crypto", bindings::crypto::CryptoBinding {})?;
            vm.register_module(
                "@lmb/http",
                bindings::http::HttpBinding::builder()
                    .maybe_permissions(permissions.clone())
                    .maybe_timeout(http_timeout)
                    .build()?,
            )?;
            vm.register_module("@lmb/json", bindings::json::JsonBinding {})?;
            vm.register_module("@lmb/json-path", bindings::json_path::JsonPathBinding {})?;
            vm.register_module("@lmb/toml", bindings::toml::TomlBinding {})?;
            vm.register_module("@lmb/yaml", bindings::yaml::YamlBinding {})?;
        }
        let func = vm.load(WRAP_FUNC).eval::<LuaFunction>()?.bind(func)?;
        if let Some(store) = &store {
            let conn = store.lock();
            {
                let _ = debug_span!("set_pragmas").entered();
                conn.pragma_update(None, "busy_timeout", 5000)?;
                conn.pragma_update(None, "journal_mode", "WAL")?;
                conn.pragma_update(None, "foreign_keys", "OFF")?;
                conn.pragma_update(None, "synchronous", "NORMAL")?;
            }
            {
                let span = debug_span!("run_migrations", count = MIGRATIONS.len()).entered();
                for migration in MIGRATIONS.iter() {
                    let _ = debug_span!(parent: &span, "run_migration", migration).entered();
                    conn.execute_batch(migration)?;
                }
            }
        }
        let mut runner = Self {
            func,
            reader,
            store,
            timeout,
            vm, // otherwise the Lua VM would be destroyed
        };
        {
            let _ = debug_span!("binding_globals").entered();
            bindings::globals::bind(&mut runner)?;
            bindings::io::bind(&mut runner)?;
        }
        Ok(runner)
    }

    /// Invokes the Lua function with the given state.
    #[builder]
    pub async fn invoke(&self, state: Option<State>) -> LmbResult<Invoked> {
        let used_memory = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        self.vm.set_interrupt({
            let timeout = self.timeout;
            let used_memory = used_memory.clone();
            move |vm| {
                used_memory.fetch_max(vm.used_memory(), Ordering::Relaxed);
                if let Some(t) = timeout {
                    if start.elapsed() > t {
                        return Err(LuaError::external(Timeout {
                            elapsed: start.elapsed(),
                            timeout: t,
                        }));
                    }
                }
                Ok(LuaVmState::Continue)
            }
        });

        let ctx = self.vm.create_table()?;
        if let Some(state) = &state {
            if let Some(state) = &state.state {
                ctx.set("state", self.vm.to_value(state)?)?;
            }
            if let Some(request) = &state.request {
                ctx.set("request", self.vm.to_value(request)?)?;
            }
        }
        if self.store.is_some() {
            ctx.set(
                "store",
                StoreBinding::builder()
                    .maybe_store(self.store.clone())
                    .build(),
            )?;
        }

        let invoked = Invoked::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed));

        let (ok, values) = {
            let span = debug_span!("call");
            match self
                .func
                .call_async::<LuaMultiValue>(ctx)
                .instrument(span)
                .await
            {
                Ok(values) => {
                    let values = values.into_vec();
                    let ok = values
                        .first()
                        .and_then(|b| b.as_boolean())
                        .unwrap_or_default();
                    let values = values[1..].to_vec();
                    (ok, values)
                }
                Err(e) => match &e {
                    LuaError::ExternalError(ee) => {
                        if let Some(timeout) = ee.downcast_ref::<Timeout>() {
                            return Ok(invoked
                                .result(Err(LmbError::Timeout(timeout.clone())))
                                .build());
                        } else {
                            return Ok(invoked.result(Err(LmbError::Lua(e))).build());
                        }
                    }
                    _ => return Ok(invoked.result(Err(LmbError::Lua(e))).build()),
                },
            }
        };

        if !ok {
            if let Some(value) = values.first() {
                if let LuaValue::Error(e) = value {
                    return Ok(invoked.result(Err(LmbError::Lua(*e.clone()))).build());
                } else {
                    let value = self.vm.from_value::<Value>(value.clone())?;
                    return Ok(invoked
                        .result(match value {
                            Value::String(s) => Err(LmbError::Lua(LuaError::runtime(s))),
                            _ => Err(LmbError::LuaValue(value)),
                        })
                        .build());
                }
            } else {
                unreachable!()
            }
        }

        let value = if values.is_empty() {
            json!(null)
        } else if values.len() == 1 {
            let value = values.first().expect("expect a value");
            self.vm.from_value::<Value>(value.clone())?
        } else {
            let mut arr = json!([]);
            let arr_mut = arr.as_array_mut().expect("expect an array");
            for value in values {
                arr_mut.push(self.vm.from_value::<Value>(value.clone())?);
            }
            arr
        };
        Ok(invoked.result(Ok(value)).build())
    }
}

impl Runner {
    /// Swaps the underlying reader with a new one.
    ///
    /// This is useful for reusing a Runner across multiple requests
    /// by swapping the reader between invocations.
    pub async fn swap_reader<R>(&self, reader: R)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        self.reader.swap(reader).await;
    }

    /// Returns a reference to the shared reader.
    pub fn shared_reader(&self) -> &LmbInput {
        &self.reader
    }
}

#[cfg(test)]
mod tests {
    use mlua::prelude::*;
    use serde_json::json;
    use test_case::test_case;
    use tokio::io::empty;

    use super::*;

    #[tokio::test]
    async fn test_error_handling() {
        let source = include_str!("fixtures/errors/error.lua");
        let runner = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap();
        let Some(Invoked {
            result: Err(LmbError::Lua(LuaError::RuntimeError(message))),
            ..
        }) = runner.invoke().call().await.ok()
        else {
            panic!("Expected a Lua runtime error");
        };
        assert_eq!("test:2: unknown error", message);
    }

    #[test_case(include_str!("fixtures/hello.lua"), None, json!(true); "hello")]
    #[test_case(include_str!("fixtures/add.lua"), Some(json!(1)), json!(2); "add")]
    #[tokio::test]
    async fn test_invoke(source: &'static str, state: Option<Value>, expected: Value) {
        let runner = Runner::builder(source, empty()).build().unwrap();
        let state = State::builder().maybe_state(state).build();
        let result = runner.invoke().state(state).call().await.unwrap();
        assert_eq!(expected, result.result.unwrap());
    }

    #[tokio::test]
    async fn test_invoke_closure() {
        let source = include_str!("fixtures/closure.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        for i in 1..=10 {
            let result = runner.invoke().call().await.unwrap();
            assert_eq!(json!(i), result.result.unwrap());
        }
    }

    #[tokio::test]
    async fn test_invoke_timeout() {
        let source = include_str!("fixtures/infinite.lua");
        let runner = Runner::builder(source, empty())
            .timeout(Duration::from_millis(10))
            .build()
            .unwrap();
        let res = runner.invoke().call().await.unwrap();
        let err = res.result.unwrap_err();
        assert!(matches!(err, LmbError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_multi() {
        let source = include_str!("fixtures/multi.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(json!([true, 1]), result.result.unwrap());
    }

    #[tokio::test]
    async fn test_from_shared_reader() {
        let source = include_str!("fixtures/hello.lua");
        let reader = Arc::new(SharedReader::new(empty()));
        Runner::from_shared_reader(source, reader).call().unwrap();
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let source = include_str!("fixtures/errors/syntax-error.lua");
        let err = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap_err();
        let LmbError::Lua(LuaError::SyntaxError { message, .. }) = err else {
            panic!("Expected a Lua syntax error");
        };
        assert_eq!(
            Some("test:2: Incomplete statement: expected assignment or a function call"),
            message.lines().next()
        );
    }

    #[tokio::test]
    async fn test_value_as_error() {
        let source = include_str!("fixtures/value-as-error.lua");
        let runner = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap();
        let Some(Invoked {
            result: Err(LmbError::LuaValue(value)),
            ..
        }) = runner.invoke().call().await.ok()
        else {
            panic!("Expected a Lua value error");
        };
        assert_eq!(json!({"a": 1}), value);
    }
}
