use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bon::{Builder, bon};
use mlua::prelude::*;
use parking_lot::Mutex;
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncSeek, AsyncSeekExt, BufReader};

use crate::lua_binding::Binding;

mod lua_binding;
mod lua_io;

#[derive(Debug, Error)]
pub enum LmbError {
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("Expected a Lua function, but got {actual} instead")]
    FromLuaConversionError { actual: Box<str> },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Timeout after {elapsed:?}, timeout was {timeout:?}")]
    Timeout {
        elapsed: Duration,
        timeout: Duration,
    },
}

type LmbResult<T> = Result<T, LmbError>;

#[derive(Builder, Debug)]
pub struct Invoked {
    pub elapsed: Duration,
    pub result: LmbResult<Value>,
    pub used_memory: usize,
}

#[derive(Debug)]
pub struct Runner<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    func: LuaFunction,
    name: Box<str>,
    reader: Arc<Mutex<BufReader<R>>>,
    source: Box<str>,
    timeout: Option<Duration>,
    vm: Lua,
}

#[bon]
impl<R> Runner<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    #[builder]
    pub fn new<S>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: R,
        #[builder(into)] name: Option<Box<str>>,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsRef<str>,
    {
        let source = source.as_ref();

        let vm = Lua::new();
        vm.sandbox(true)?;

        let name = name.unwrap_or_else(|| "(unnamed)".into());
        let func: LuaValue = vm.load(source).set_name(&*name).eval()?;
        let LuaValue::Function(func) = func else {
            return Err(LmbError::FromLuaConversionError {
                actual: func.type_name().into(),
            });
        };

        let reader = Arc::new(Mutex::new(BufReader::new(reader)));
        vm.register_module("@lmb", Binding::builder(reader.clone()).build())?;

        let mut runner = Self {
            func,
            reader,
            name,
            source: source.into(),
            timeout,
            vm, // otherwise the Lua VM would be destroyed
        };

        lua_io::bind(&mut runner)?;

        Ok(runner)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

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
        let invoked = Invoked::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed));
        let value = match self.func.call::<LuaValue>(self.vm.to_value(&state)) {
            Ok(value) => value,
            Err(LuaError::RuntimeError(msg)) if msg == "timeout" => {
                let e = LmbError::Timeout {
                    elapsed: start.elapsed(),
                    timeout: self.timeout.unwrap_or_default(),
                };
                return Ok(invoked.result(Err(e)).build());
            }
            Err(e) => {
                return Ok(invoked.result(Err(LmbError::LuaError(e))).build());
            }
        };

        let value = self.vm.from_value::<Value>(value)?;
        Ok(invoked.result(Ok(value)).build())
    }

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
        let runner = Runner::builder(&source, empty()).build().unwrap();
        for i in 1..=10 {
            let result = runner.invoke().call().await.unwrap();
            assert_eq!(json!(i), result.result.unwrap());
        }
    }

    #[tokio::test]
    async fn test_invoke_timeout() {
        let source = include_str!("fixtures/infinite.lua");
        let runner = Runner::builder(&source, empty())
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
        let runner = Runner::builder(&source, empty()).build().unwrap();
        let Some(Invoked {
            result: Err(LmbError::LuaError(LuaError::RuntimeError(msg))),
            ..
        }) = runner.invoke().call().await.ok()
        else {
            panic!("Expected a Lua runtime error");
        };
        let name = runner.name();
        let expected = format!("[string \"{name}\"]:3: An error occurred");
        assert!(msg.starts_with(&expected));
    }
}
