use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bon::{Builder, bon};
use mlua::prelude::*;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LmbError {
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("Expected a Lua function, but got {actual} instead")]
    FromLuaConversionError { actual: Box<str> },
}

type LmbResult<T> = Result<T, LmbError>;

#[derive(Debug)]
pub struct Runner {
    func: LuaFunction,
    source: Box<str>,
    timeout: Option<Duration>,
    vm: Lua,
}

#[derive(Builder, Debug)]
pub struct CallResult {
    pub elapsed: Duration,
    pub used_memory: usize,
    pub value: Value,
}

#[bon]
impl Runner {
    #[builder]
    pub fn new<S: AsRef<str>>(source: S, timeout: Option<Duration>) -> LmbResult<Self> {
        let source = source.as_ref();

        let vm = Lua::new();
        vm.sandbox(true)?;

        let func: LuaValue = vm.load(source).eval()?;
        let LuaValue::Function(func) = func else {
            return Err(LmbError::FromLuaConversionError {
                actual: func.type_name().into(),
            });
        };

        Ok(Self {
            func,
            vm, // otherwise the Lua VM would be destroyed
            source: source.into(),
            timeout,
        })
    }

    #[builder]
    pub fn call(&self, state: Option<Value>) -> LmbResult<CallResult> {
        let used_memory = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        self.vm.set_interrupt({
            let timeout = self.timeout.clone();
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
        let value = self.func.call::<LuaValue>(self.vm.to_value(&state))?;
        Ok(CallResult::builder()
            .elapsed(start.elapsed())
            .used_memory(used_memory.load(Ordering::Relaxed))
            .value(self.vm.from_value::<Value>(value)?)
            .build())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_call() {
        {
            let source = include_str!("fixtures/hello.lua");
            let runner = Runner::builder().source(&source).build().unwrap();
            let result = runner.call().maybe_state(None).call().unwrap();
            assert_eq!(json!(true), result.value);
        }
        {
            let source = include_str!("fixtures/add.lua");
            let runner = Runner::builder().source(&source).build().unwrap();
            let result = runner.call().state(json!(1)).call().unwrap();
            assert_eq!(json!(2), result.value);
        }
        {
            let source = include_str!("fixtures/closure.lua");
            let runner = Runner::builder().source(&source).build().unwrap();
            for i in 1..=10 {
                let result = runner.call().call().unwrap();
                assert_eq!(json!(i), result.value);
            }
        }
        {
            let source = include_str!("fixtures/infinite.lua");
            let runner = Runner::builder()
                .source(&source)
                .timeout(Duration::from_millis(10))
                .build()
                .unwrap();
            assert!(matches!(
                runner.call().call().unwrap_err(),
                LmbError::LuaError(LuaError::RuntimeError(..))
            ));
        }
    }
}
