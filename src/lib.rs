use std::{
    io::{BufRead, BufReader, Read, Seek},
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

#[derive(Debug, Error)]
pub enum LmbError {
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("Expected a Lua function, but got {actual} instead")]
    FromLuaConversionError { actual: Box<str> },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

type LmbResult<T> = Result<T, LmbError>;

#[derive(Builder, Debug)]
pub struct CallResult {
    pub elapsed: Duration,
    pub used_memory: usize,
    pub value: Value,
}

#[derive(Debug)]
pub struct Runner<R>
where
    for<'lua> R: 'lua + Read + Seek,
{
    func: LuaFunction,
    reader: Arc<Mutex<BufReader<R>>>,
    source: Box<str>,
    timeout: Option<Duration>,
    vm: Lua,
}

fn setup_io<R>(runner: &mut Runner<R>) -> LmbResult<()>
where
    for<'lua> R: 'lua + Read + Seek,
{
    let globals = runner.vm.globals();

    let io = runner.vm.create_table()?;
    io.set(
        "read",
        runner.vm.create_function_mut({
            let reader = runner.reader.clone();
            move |vm, fmt: String| match &*fmt {
                "*a" | "*all" => {
                    let mut buf = vec![];
                    let mut locked = reader.lock();
                    locked.read_to_end(&mut buf)?;
                    Ok(LuaValue::String(vm.create_string(buf)?))
                }
                "*l" | "*line" => {
                    let mut locked = reader.lock();
                    let mut line = String::new();
                    if locked.read_line(&mut line)? == 0 {
                        return Ok(LuaNil);
                    }
                    Ok(LuaValue::String(vm.create_string(line.trim_end())?))
                }
                "*n" | "*number" => {
                    let mut locked = reader.lock();
                    let mut buf = String::new();
                    if locked.read_line(&mut buf)? == 0 {
                        return Ok(LuaNil);
                    }
                    match buf.trim().parse::<f64>() {
                        Ok(num) => Ok(LuaValue::Number(num)),
                        Err(_) => Ok(LuaNil),
                    }
                }
                _ => Ok(LuaNil),
            }
        })?,
    )?;

    globals.set("io", io)?;

    Ok(())
}

#[bon]
impl<R> Runner<R>
where
    for<'lua> R: 'lua + Read + Seek,
{
    #[builder]
    pub fn new<S>(
        #[builder(start_fn)] source: S,
        #[builder(start_fn)] reader: R,
        timeout: Option<Duration>,
    ) -> LmbResult<Self>
    where
        S: AsRef<str>,
    {
        let source = source.as_ref();

        let vm = Lua::new();
        vm.sandbox(true)?;

        let func: LuaValue = vm.load(source).eval()?;
        let LuaValue::Function(func) = func else {
            return Err(LmbError::FromLuaConversionError {
                actual: func.type_name().into(),
            });
        };
        let mut runner = Self {
            func,
            reader: Arc::new(Mutex::new(BufReader::new(reader))),
            vm, // otherwise the Lua VM would be destroyed
            source: source.into(),
            timeout,
        };
        setup_io(&mut runner)?;
        Ok(runner)
    }

    #[builder]
    pub fn invoke(&self, state: Option<Value>) -> LmbResult<CallResult> {
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

    pub fn rewind_input(&self) -> LmbResult<()> {
        self.reader.lock().rewind()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, empty};

    use serde_json::json;

    use super::*;

    #[test]
    fn test_call() {
        {
            let source = include_str!("fixtures/hello.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            let result = runner.invoke().call().unwrap();
            assert_eq!(json!(true), result.value);
        }
        {
            let source = include_str!("fixtures/add.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            let result = runner.invoke().state(json!(1)).call().unwrap();
            assert_eq!(json!(2), result.value);
        }
        {
            let source = include_str!("fixtures/closure.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            for i in 1..=10 {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(i), result.value);
            }
        }
        {
            let source = include_str!("fixtures/infinite.lua");
            let runner = Runner::builder(&source, empty())
                .timeout(Duration::from_millis(10))
                .build()
                .unwrap();
            assert!(matches!(
                runner.invoke().call().unwrap_err(),
                LmbError::LuaError(LuaError::RuntimeError(..))
            ));
        }
    }

    #[test]
    fn test_read() {
        {
            let source = include_str!("fixtures/read-all.lua");
            let text = "first line\nsecond line\n";
            let input = Cursor::new(text);
            let runner = Runner::builder(&source, input).build().unwrap();
            let result = runner.invoke().call().unwrap();
            assert_eq!(json!(text), result.value);
        }
        {
            let source = include_str!("fixtures/read-line.lua");
            let input = Cursor::new("first line\nsecond line\n");
            let runner = Runner::builder(&source, input).build().unwrap();
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!("first line"), result.value);
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!("second line"), result.value);
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(null), result.value);
            }
        }
        {
            let source = include_str!("fixtures/read-number.lua");
            let input = Cursor::new("42\n3.14\nnot a number\n");
            let runner = Runner::builder(&source, input).build().unwrap();
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(42), result.value);
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(3.14), result.value);
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(null), result.value);
            }
        }
    }
}
