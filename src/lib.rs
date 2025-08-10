#![deny(missing_debug_implementations, missing_docs)]

//! A library for running Lua scripts.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bon::{Builder, bon};
use mlua::{AsChunk, prelude::*};
use parking_lot::Mutex;
use rusqlite::Connection;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt as _, BufReader};
use tracing::debug_span;

use crate::bindings::{Binding, store::StoreBinding};

mod bindings;

/// Error handling module
pub mod error;

/// Represents a timeout error when executing a Lua script
#[derive(Clone, Debug)]
pub struct Timeout {
    elapsed: Duration,
    timeout: Duration,
}

impl std::fmt::Display for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lua script execution timed out after {:?}, timeout was {:?}",
            self.elapsed, self.timeout
        )
    }
}

impl std::error::Error for Timeout {}

/// Lua VM error handling
#[derive(Debug, Error)]
pub enum LmbError {
    /// Error thrown by the Lua VM
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    /// Error reading from the input stream
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Error from reqwest crate
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    /// Error when the Lua script times out
    #[error("Timeout: {0}")]
    Timeout(#[from] Timeout),
}

type LmbInput<R> = Arc<Mutex<BufReader<R>>>;
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
pub struct Runner<R>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    func: LuaFunction,
    reader: LmbInput<R>,
    store: Option<LmbStore>,
    timeout: Option<Duration>,
    vm: Lua,
}

#[bon]
impl<R> Runner<R>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    /// Creates a new Lua runner with the given source code and input reader.
    #[builder]
    pub fn new<S>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: R,
        #[builder(into)] default_name: Option<String>,
        http_timeout: Option<Duration>,
        store: Option<Connection>,
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

        let reader = Arc::new(Mutex::new(BufReader::new(reader)));
        {
            let _ = debug_span!("register_modules").entered();
            vm.register_module("@lmb", Binding::builder(reader.clone()).build())?;
            vm.register_module("@lmb/coroutine", bindings::coroutine::CoroutineBinding {})?;
            vm.register_module("@lmb/crypto", bindings::crypto::CryptoBinding {})?;
            vm.register_module(
                "@lmb/http",
                bindings::http::HttpBinding::builder()
                    .maybe_timeout(http_timeout)
                    .build()?,
            )?;
            vm.register_module("@lmb/json", bindings::json::JsonBinding {})?;
            vm.register_module("@lmb/json-path", bindings::json_path::JsonPathBinding {})?;
            vm.register_module("@lmb/toml", bindings::toml::TomlBinding {})?;
            vm.register_module("@lmb/yaml", bindings::yaml::YamlBinding {})?;
        }

        let mut runner = Self {
            func,
            reader,
            store: store.map(|conn| Arc::new(Mutex::new(conn))),
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
    pub async fn invoke(&self, state: Option<Value>) -> LmbResult<Invoked> {
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
        if state.is_some() {
            ctx.set("state", self.vm.to_value(&state)?)?;
        }
        if self.store.is_some() {
            ctx.set("store", StoreBinding::builder(self.store.clone()).build()?)?;
        }

        let invoked = Invoked::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed));

        let values = {
            let _ = debug_span!("call").entered();
            match self.func.call_async::<LuaMultiValue>(ctx).await {
                Ok(values) => values,
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

        let value = if values.is_empty() {
            json!(null)
        } else if values.len() == 1 {
            let value = values.front().expect("Expected at least one value");
            self.vm.from_value::<Value>(value.clone())?
        } else {
            let mut arr = json!([]);
            let arr_mut = arr.as_array_mut().expect("Expected an array");
            for value in values {
                arr_mut.push(self.vm.from_value::<Value>(value.clone())?);
            }
            arr
        };
        Ok(invoked.result(Ok(value)).build())
    }
}

impl<R> Runner<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    /// Rewinds the input stream to the beginning.
    /// This function should be only called in tests or benchmarks to reset the input stream.
    pub async fn rewind_input(&self) -> LmbResult<()> {
        self.reader.lock().rewind().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use test_case::test_case;
    use tokio::io::empty;

    use super::*;

    #[test_case(include_str!("fixtures/hello.lua"), None, json!(true); "hello")]
    #[test_case(include_str!("fixtures/add.lua"), Some(json!(1)), json!(2); "add")]
    #[tokio::test]
    async fn test_invoke(source: &'static str, state: Option<Value>, expected: Value) {
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().maybe_state(state).call().await.unwrap();
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
    async fn test_error_handling() {
        let source = include_str!("fixtures/error.lua");
        let runner = Runner::builder(source, empty())
            .default_name("test")
            .build()
            .unwrap();
        let Some(Invoked {
            result: Err(LmbError::Lua(LuaError::RuntimeError(msg))),
            ..
        }) = runner.invoke().call().await.ok()
        else {
            panic!("Expected a Lua runtime error");
        };
        assert_eq!(Some("test:3: An error occurred"), msg.lines().next());
    }

    #[tokio::test]
    async fn test_multi() {
        let source = include_str!("fixtures/multi.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(json!([true, 1]), result.result.unwrap());
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let source = include_str!("fixtures/syntax-error.lua");
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
}
