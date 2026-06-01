#![deny(missing_debug_implementations, missing_docs)]

//! A library for running Lua scripts.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bon::{Builder, bon};
use mlua::{AsChunk, prelude::*};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::AsyncRead;
use tracing::{Instrument, debug_span};

use crate::{
    bindings::{Binding, store::StoreBinding},
    permission::Permissions,
    reader::SharedReader,
    store::StoreBackend,
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
        let _ = self.cancel_at.set(Instant::now());
        self.cancelled.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been signalled (cooperative check).
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Returns whether the grace period has elapsed since cancellation began.
    ///
    /// Returns `false` if [`cancel`](Self::cancel) has not yet been called.
    pub fn force_deadline_passed(&self) -> bool {
        self.cancel_at
            .get()
            .is_some_and(|t| t.elapsed() >= self.grace)
    }
}

/// Marker error raised by the VM interrupt hook when a script is forcibly cancelled.
#[derive(Clone, Debug)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Lua script execution was cancelled")
    }
}

impl Error for Cancelled {}

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
    /// Generic store backend error
    #[error("Store error: {0}")]
    Store(Box<dyn std::error::Error + Send + Sync>),
    /// Error when the Lua script times out
    #[error("Timeout: {0}")]
    Timeout(#[from] Timeout),
    /// Error when the Lua script is forcibly cancelled during shutdown
    #[error("Cancelled: {0}")]
    Cancelled(#[from] Cancelled),
}

/// Type alias for the shared reader used in the library.
pub type LmbInput = Arc<SharedReader>;
/// Type alias for the shared store backend.
pub type LmbStore = Arc<dyn StoreBackend>;

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
    cancellation: Option<Cancellation>,
    func: LuaFunction,
    reader: LmbInput,
    store: Option<LmbStore>,
    timeout: Option<Duration>,
    vm: Lua,
}

static WRAP_FUNC: &str = r"return function(f, ctx) return pcall(f, ctx) end";

/// Processes the error value(s) returned by pcall when it fails.
///
/// When pcall returns `(false, ...)`, this function extracts and converts the error
/// into an appropriate `LmbError`.
fn process_pcall_error(values: &[LuaValue], vm: &Lua) -> LmbResult<LmbError> {
    if let Some(value) = values.first() {
        if let LuaValue::Error(e) = value {
            Ok(LmbError::Lua(*e.clone()))
        } else {
            let value = vm.from_value::<Value>(value.clone())?;
            Ok(match value {
                Value::String(s) => LmbError::Lua(LuaError::runtime(s)),
                _ => LmbError::LuaValue(value),
            })
        }
    } else {
        debug_assert!(false, "pcall should always return an error on failure");
        Ok(LmbError::Lua(LuaError::runtime("pcall failed")))
    }
}

#[bon]
impl Runner {
    /// Creates a new Lua runner with the given source code and input reader.
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

    /// Creates a new Lua runner with the given source code and shared reader.
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
                "@lmb/fs",
                bindings::fs::FsBinding::builder()
                    .maybe_permissions(permissions.clone())
                    .build(),
            )?;
            vm.register_module(
                "@lmb/http",
                bindings::http::HttpBinding::builder()
                    .maybe_permissions(permissions.clone())
                    .maybe_timeout(http_timeout)
                    .build(),
            )?;
            vm.register_module("@lmb/json", bindings::json::JsonBinding {})?;
            vm.register_module("@lmb/json-path", bindings::json_path::JsonPathBinding {})?;
            vm.register_module("@lmb/logging", bindings::logging::LoggingBinding {})?;
            vm.register_module("@lmb/regex", bindings::regex::RegexBinding {})?;
            vm.register_module("@lmb/time", bindings::time::TimeBinding {})?;
            vm.register_module("@lmb/toml", bindings::toml::TomlBinding {})?;
            vm.register_module("@lmb/yaml", bindings::yaml::YamlBinding {})?;
        }
        let func = vm.load(WRAP_FUNC).eval::<LuaFunction>()?.bind(func)?;
        if let Some(store) = &store {
            store.migrate()?;
        }
        let mut runner = Self {
            cancellation,
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
    ///
    /// The returned [`Invoked`] always carries the run metrics; whether the Lua
    /// function itself succeeded or failed is captured by its `result` field.
    #[builder]
    pub async fn invoke(&self, state: Option<State>) -> Invoked {
        let used_memory = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        let timeout = self.timeout;
        let cancellation = self.cancellation.clone();
        self.vm.set_interrupt({
            let used_memory = used_memory.clone();
            move |vm| {
                used_memory.fetch_max(vm.used_memory(), Ordering::Relaxed);
                if let Some(timeout) = timeout
                    && start.elapsed() > timeout
                {
                    return Err(LuaError::external(Timeout {
                        elapsed: start.elapsed(),
                        timeout,
                    }));
                }
                if let Some(cancellation) = &cancellation
                    && cancellation.force_deadline_passed()
                {
                    return Err(LuaError::external(Cancelled));
                }
                Ok(LuaVmState::Continue)
            }
        });

        // The fallible setup-and-call logic lives in an inner async block that
        // yields `LmbResult<Value>`; the outer body then wraps that single result
        // together with the run metrics into an `Invoked`. This keeps the `?`
        // ergonomics while exposing only one layer of `Result` to callers.
        let result: LmbResult<Value> = async {
            let ctx = self.vm.create_table()?;
            if let Some(state) = &state {
                if let Some(state) = &state.state {
                    ctx.set("state", self.vm.to_value(state)?)?;
                }
                if let Some(request) = &state.request {
                    ctx.set("request", self.vm.to_value(request)?)?;
                }
            }
            if let Some(lmb_store) = &self.store {
                ctx.set(
                    "store",
                    StoreBinding::builder().store(lmb_store.clone()).build(),
                )?;
            }
            if let Some(cancellation) = &self.cancellation {
                let cancellation = cancellation.clone();
                ctx.set(
                    "cancelled",
                    self.vm
                        .create_function(move |_, ()| Ok(cancellation.is_cancelled()))?,
                )?;
            }

            let (ok, mut values) = {
                let span = debug_span!("call");
                match self
                    .func
                    .call_async::<LuaMultiValue>(ctx)
                    .instrument(span)
                    .await
                {
                    Ok(values) => {
                        let mut values = values.into_vec();
                        let ok = values
                            .first()
                            .and_then(|b| b.as_boolean())
                            .unwrap_or_default();
                        if !values.is_empty() {
                            values.remove(0);
                        }
                        (ok, values)
                    }
                    Err(e) => match &e {
                        LuaError::ExternalError(ee) => {
                            if let Some(timeout) = ee.downcast_ref::<Timeout>() {
                                return Err(LmbError::Timeout(timeout.clone()));
                            } else if ee.downcast_ref::<Cancelled>().is_some() {
                                return Err(LmbError::Cancelled(Cancelled));
                            } else {
                                return Err(LmbError::Lua(e));
                            }
                        }
                        _ => return Err(LmbError::Lua(e)),
                    },
                }
            };

            if !ok {
                return Err(process_pcall_error(&values, &self.vm)?);
            }

            let value = match values.len() {
                0 => json!(null),
                1 => self.vm.from_value::<Value>(values.remove(0))?,
                _ => {
                    let mut arr = Vec::with_capacity(values.len());
                    for value in values {
                        arr.push(self.vm.from_value::<Value>(value)?);
                    }
                    Value::Array(arr)
                }
            };
            Ok(value)
        }
        .await;

        Invoked::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed))
            .result(result)
            .build()
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
        let source = include_str!("./fixtures/errors/error.lua");
        let runner = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap();
        let Invoked {
            result: Err(LmbError::Lua(LuaError::RuntimeError(message))),
            ..
        } = runner.invoke().call().await
        else {
            panic!("Expected a Lua runtime error");
        };
        assert_eq!("test:2: unknown error", message);
    }

    #[test_case(include_str!("./fixtures/core/hello.lua"), None, json!(true); "hello")]
    #[test_case(include_str!("./fixtures/core/add.lua"), Some(json!(1)), json!(2); "add")]
    #[tokio::test]
    async fn test_invoke(source: &'static str, state: Option<Value>, expected: Value) {
        let runner = Runner::builder(source, empty()).build().unwrap();
        let state = State::builder().maybe_state(state).build();
        let result = runner.invoke().state(state).call().await;
        assert_eq!(expected, result.result.unwrap());
    }

    #[tokio::test]
    async fn test_invoke_closure() {
        let source = include_str!("./fixtures/core/closure.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        for i in 1..=10 {
            let result = runner.invoke().call().await;
            assert_eq!(json!(i), result.result.unwrap());
        }
    }

    #[tokio::test]
    async fn test_invoke_timeout() {
        let source = include_str!("./fixtures/core/infinite.lua");
        let runner = Runner::builder(source, empty())
            .timeout(Duration::from_millis(10))
            .build()
            .unwrap();
        let res = runner.invoke().call().await;
        let err = res.result.unwrap_err();
        assert!(matches!(err, LmbError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_multi() {
        let source = include_str!("./fixtures/core/multi.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await;
        assert_eq!(json!([true, 1]), result.result.unwrap());
    }

    #[tokio::test]
    async fn test_from_shared_reader() {
        let source = include_str!("./fixtures/core/hello.lua");
        let reader = Arc::new(SharedReader::new(empty()));
        Runner::from_shared_reader(source, reader).call().unwrap();
    }

    #[tokio::test]
    async fn test_shared_reader_accessor() {
        use std::io::Cursor;

        let source = include_str!("./fixtures/core/hello.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();

        // Test shared_reader() accessor
        let shared = runner.shared_reader();
        assert!(std::sync::Arc::strong_count(shared) >= 1);

        // Test swap_reader() through the runner
        runner.swap_reader(Cursor::new(b"new data")).await;
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let source = include_str!("./fixtures/errors/syntax-error.lua");
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
        let source = include_str!("./fixtures/value-as-error.lua");
        let runner = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap();
        let Invoked {
            result: Err(LmbError::LuaValue(value)),
            ..
        } = runner.invoke().call().await
        else {
            panic!("Expected a Lua value error");
        };
        assert_eq!(json!({"a": 1}), value);
    }

    #[test]
    #[cfg_attr(
        debug_assertions,
        should_panic(expected = "pcall should always return an error")
    )]
    fn test_process_pcall_error_empty_values() {
        let lua = Lua::new();
        let result = super::process_pcall_error(&[], &lua);
        // In release mode, the debug_assert! is stripped, so we can test the fallback behavior
        let err = result.unwrap();
        assert!(matches!(err, LmbError::Lua(LuaError::RuntimeError(msg)) if msg == "pcall failed"));
    }

    #[test]
    fn test_process_pcall_error_with_lua_error() {
        let lua = Lua::new();
        let lua_err = LuaError::runtime("test error");
        let error_value = LuaValue::Error(Box::new(lua_err));
        let result = super::process_pcall_error(&[error_value], &lua);
        let err = result.unwrap();
        assert!(matches!(err, LmbError::Lua(LuaError::RuntimeError(msg)) if msg == "test error"));
    }

    #[test]
    fn test_process_pcall_error_with_string_value() {
        let lua = Lua::new();
        let string_value = lua.create_string("error message").unwrap();
        let result = super::process_pcall_error(&[LuaValue::String(string_value)], &lua);
        let err = result.unwrap();
        assert!(
            matches!(err, LmbError::Lua(LuaError::RuntimeError(msg)) if msg == "error message")
        );
    }

    #[test]
    fn test_process_pcall_error_with_non_string_value() {
        let lua = Lua::new();
        let number_value = LuaValue::Number(42.0);
        let result = super::process_pcall_error(&[number_value], &lua);
        let err = result.unwrap();
        assert!(matches!(err, LmbError::LuaValue(Value::Number(n)) if n.as_f64() == Some(42.0)));
    }

    #[test]
    fn cancelled_error_displays_message() {
        assert_eq!(Cancelled.to_string(), "Lua script execution was cancelled");
    }

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
        std::thread::sleep(Duration::from_millis(100));
        assert!(c.force_deadline_passed());
    }

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
        let invoked = handle.await.unwrap();
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
        let handle: tokio::task::JoinHandle<Invoked> =
            tokio::spawn(async move { runner.invoke().call().await });
        cancellation.cancel();
        let invoked = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("invoke should be force-interrupted, not hang")
            .unwrap();
        assert!(matches!(invoked.result, Err(LmbError::Cancelled(_))));
    }
}
