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
            move |vm, fmt: LuaValue| {
                if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                    match &*f {
                        "*a" | "*all" => {
                            let mut buf = vec![];
                            let mut locked = reader.lock();
                            locked.read_to_end(&mut buf)?;
                            return Ok(LuaValue::String(vm.create_string(buf)?));
                        }
                        "*l" | "*line" => {
                            let mut locked = reader.lock();
                            let mut line = String::new();
                            if locked.read_line(&mut line)? == 0 {
                                return Ok(LuaNil);
                            }
                            return Ok(LuaValue::String(vm.create_string(line.trim_end())?));
                        }
                        "*n" | "*number" => {
                            let mut locked = reader.lock();
                            let mut buf = String::new();
                            if locked.read_line(&mut buf)? == 0 {
                                return Ok(LuaNil);
                            }
                            match buf.trim().parse::<f64>() {
                                Ok(num) => return Ok(LuaValue::Number(num)),
                                Err(_) => return Ok(LuaNil),
                            }
                        }
                        _ => {
                            return Err(LuaError::BadArgument {
                                to: Some("read".to_string()),
                                pos: 1,
                                name: None,
                                cause: Arc::new(LuaError::runtime("invalid format")),
                            });
                        }
                    }
                }
                Ok(LuaNil)
            }
        })?,
    )?;

    globals.set("io", io)?;

    Ok(())
}

#[derive(Debug)]
pub struct Runner<R>
where
    for<'lua> R: 'lua + Read + Seek,
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
    for<'lua> R: 'lua + Read + Seek,
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
        let mut runner = Self {
            func,
            reader: Arc::new(Mutex::new(BufReader::new(reader))),
            name,
            source: source.into(),
            timeout,
            vm, // otherwise the Lua VM would be destroyed
        };
        setup_io(&mut runner)?;
        Ok(runner)
    }

    #[builder]
    pub fn invoke(&self, state: Option<Value>) -> LmbResult<Invoked> {
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
    fn test_invoke() {
        {
            let source = include_str!("fixtures/hello.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            let result = runner.invoke().call().unwrap();
            assert_eq!(json!(true), result.result.unwrap());
        }
        {
            let source = include_str!("fixtures/add.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            let result = runner.invoke().state(json!(1)).call().unwrap();
            assert_eq!(json!(2), result.result.unwrap());
        }
        {
            let source = include_str!("fixtures/closure.lua");
            let runner = Runner::builder(&source, empty()).build().unwrap();
            for i in 1..=10 {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(i), result.result.unwrap());
            }
        }
        {
            let source = include_str!("fixtures/infinite.lua");
            let runner = Runner::builder(&source, empty())
                .timeout(Duration::from_millis(10))
                .build()
                .unwrap();
            let res = runner.invoke().call().unwrap();
            let err = res.result.unwrap_err();
            assert!(matches!(err, LmbError::Timeout { .. }));
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
            assert_eq!(json!(text), result.result.unwrap());
        }
        {
            let source = include_str!("fixtures/read-line.lua");
            let input = Cursor::new("first line\nsecond line\n");
            let runner = Runner::builder(&source, input).build().unwrap();
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!("first line"), result.result.unwrap());
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!("second line"), result.result.unwrap());
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(null), result.result.unwrap());
            }
        }
        {
            let source = include_str!("fixtures/read-number.lua");
            let input = Cursor::new("42\n3.14\nnot a number\n");
            let runner = Runner::builder(&source, input).build().unwrap();
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(42), result.result.unwrap());
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(3.14), result.result.unwrap());
            }
            {
                let result = runner.invoke().call().unwrap();
                assert_eq!(json!(null), result.result.unwrap());
            }
        }
        {
            let source = include_str!("fixtures/invalid-format.lua");
            let text = "first line\nsecond line\n";
            let input = Cursor::new(text);
            let runner = Runner::builder(&source, input).build().unwrap();
            let result = runner.invoke().call().unwrap();
            let err = result.result.err().unwrap();
            assert!(
                err.to_string()
                    .contains("bad argument #1 to `read`: runtime error: invalid format")
            );
        }
    }

    #[test]
    fn test_error_handling() {
        let source = include_str!("fixtures/error.lua");
        let runner = Runner::builder(&source, empty()).build().unwrap();
        let Some(Invoked {
            result: Err(LmbError::LuaError(LuaError::RuntimeError(msg))),
            ..
        }) = runner.invoke().call().ok()
        else {
            panic!("Expected a Lua runtime error");
        };
        assert!(msg.starts_with("[string \"(unnamed)\"]:3: An error occurred"));
    }
}
