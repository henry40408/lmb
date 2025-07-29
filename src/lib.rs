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
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt as _, BufReader};

use crate::bindings::{Binding, store::StoreBinding};

mod bindings;

/// Error handling module
pub mod error;

/// Lua VM error handling
#[derive(Debug, Error)]
pub enum LmbError {
    /// Error thrown by the Lua VM
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    /// Error converting a Lua value to a Rust type
    #[error("Expected a Lua function, but got {actual} instead")]
    FromLuaConversion {
        /// The actual type of the Lua value
        actual: Box<str>,
    },
    /// Error reading from the input stream
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Error from reqwest crate
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    /// Error when the Lua script times out
    #[error("Timeout after {elapsed:?}, timeout was {timeout:?}")]
    Timeout {
        /// The duration the script ran before timing out
        elapsed: Duration,
        /// The timeout duration set for the script
        timeout: Duration,
    },
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
        store: Option<Connection>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsChunk,
    {
        let vm = Lua::new();
        vm.sandbox(true)?;

        let func: LuaValue = vm.load(source).eval()?;
        let LuaValue::Function(func) = func else {
            return Err(LmbError::FromLuaConversion {
                actual: func.type_name().into(),
            });
        };

        let reader = Arc::new(Mutex::new(BufReader::new(reader)));
        vm.register_module("@lmb", Binding::builder(reader.clone()).build())?;
        vm.register_module("@lmb/http", bindings::http::HttpBinding::builder().build()?)?;
        vm.register_module("@lmb/crypto", bindings::crypto::CryptoBinding {})?;
        vm.register_module("@lmb/json", bindings::json::JsonBinding {})?;

        let mut runner = Self {
            func,
            reader,
            store: store.map(|conn| Arc::new(Mutex::new(conn))),
            timeout,
            vm, // otherwise the Lua VM would be destroyed
        };

        bindings::io::bind(&mut runner)?;

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
                        return Err(LuaError::runtime("timeout"));
                    }
                }
                Ok(LuaVmState::Continue)
            }
        });

        let ctx = self.vm.create_table()?;
        ctx.set("state", self.vm.to_value(&state)?)?;
        ctx.set("store", StoreBinding::builder(self.store.clone()).build()?)?;

        let invoked = Invoked::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed));
        let value = match self.func.call_async::<LuaValue>(ctx).await {
            Ok(value) => value,
            Err(LuaError::RuntimeError(msg)) if msg == "timeout" => {
                let e = LmbError::Timeout {
                    elapsed: start.elapsed(),
                    timeout: self.timeout.unwrap_or_default(),
                };
                return Ok(invoked.result(Err(e)).build());
            }
            Err(e) => {
                return Ok(invoked.result(Err(LmbError::Lua(e))).build());
            }
        };

        let value = self.vm.from_value::<Value>(value)?;
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
        let runner = Runner::builder(source, empty()).build().unwrap();
        let Some(Invoked {
            result: Err(LmbError::Lua(LuaError::RuntimeError(msg))),
            ..
        }) = runner.invoke().call().await.ok()
        else {
            panic!("Expected a Lua runtime error");
        };
        assert!(msg.contains(":3: An error occurred"));
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let source = include_str!("fixtures/syntax-error.lua");
        let err = Runner::builder(source, empty()).build().unwrap_err();
        let LmbError::Lua(LuaError::SyntaxError { message, .. }) = err else {
            panic!("Expected a Lua syntax error");
        };
        assert!(
            message.contains(":2: Incomplete statement: expected assignment or a function call")
        );
    }
}
