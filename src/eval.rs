use bon::{Builder, bon, builder};
use chrono::Utc;
use mlua::{Compiler, prelude::*};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    fmt::Write,
    io::{BufReader, Read, Seek},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, error, trace_span, warn};

use crate::{
    DEFAULT_TIMEOUT, Error, Input, LuaSource, Result, ScheduleOptions, State, Store, bind_vm,
};

/// Solution obtained by the function.
#[derive(Builder, Debug)]
pub struct Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Evaluation.
    #[builder(start_fn)]
    pub evaluation: Arc<Evaluation<R>>,
    /// Duration.
    pub duration: Duration,
    /// Max memory usage in bytes.
    pub max_memory_usage: usize,
    /// Payload returned by the script.
    pub payload: Value,
}

#[bon]
impl<R> Solution<R>
where
    for<'lua> R: 'lua + Read,
{
    /// Render the solution.
    #[builder]
    pub fn write<W>(
        &self,
        #[builder(start_fn)] mut f: W,
        #[builder(default)] json: bool,
    ) -> Result<()>
    where
        W: Write,
    {
        if json {
            let res = serde_json::to_string(&self.payload)?;
            Ok(write!(f, "{}", res)?)
        } else {
            match &self.payload {
                Value::String(s) => Ok(write!(f, "{}", s)?),
                _ => Ok(write!(f, "{}", self.payload)?),
            }
        }
    }
}

/// Container holding the function and input for evaluation.
#[derive(Debug)]
pub struct Evaluation<R>
where
    for<'lua> R: 'lua + Read,
{
    compiled: Box<[u8]>,
    input: Input<R>,
    source: LuaSource,
    store: Option<Store>,
    timeout: Option<Duration>,
    vm: Lua,
    allowed_env_vars: Option<Vec<Box<str>>>,
}

#[bon]
impl<R> Evaluation<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Build evaluation with a reader.
    #[builder]
    pub fn new(
        #[builder(start_fn, into)] source: LuaSource,
        #[builder(start_fn)] input: R,
        store: Option<Store>,
        timeout: Option<Duration>,
        allowed_env_vars: Option<Vec<Box<str>>>,
    ) -> Result<Arc<Evaluation<R>>> {
        let input = Arc::new(Mutex::new(BufReader::new(input)));
        Self::new_with_input(source, input)
            .maybe_store(store)
            .maybe_timeout(timeout)
            .maybe_allowed_env_vars(allowed_env_vars)
            .call()
    }

    /// Build evaluation with a wrapped reader.
    #[builder]
    pub fn new_with_input(
        #[builder(start_fn, into)] source: LuaSource,
        #[builder(start_fn)] input: Input<R>,
        store: Option<Store>,
        timeout: Option<Duration>,
        allowed_env_vars: Option<Vec<Box<str>>>,
    ) -> Result<Arc<Evaluation<R>>> {
        let compiled = {
            let _s = trace_span!("compile").entered();
            let compiler = Compiler::new();
            compiler.compile(&*source.script)?.into_boxed_slice()
        };
        let vm = Lua::new();
        vm.sandbox(true)?;
        bind_vm(&vm, input.clone())
            .maybe_next(source.next.clone())
            .maybe_store(store.clone())
            .maybe_allowed_env_vars(allowed_env_vars.clone())
            .call()?;
        Ok(Arc::new(Evaluation {
            compiled,
            input,
            source,
            store,
            timeout,
            vm,
            allowed_env_vars,
        }))
    }

    /// Evaluate the function with a state.
    ///
    /// ```rust
    /// # use std::{io::empty, sync::Arc};
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> Result<()> {
    /// let e = Evaluation::builder("return 1+1", empty()).build().unwrap();
    /// let state = Arc::new(State::new());
    /// state.insert(StateKey::from("bool"), true.into());
    /// let res = e.evaluate().state(state).call()?;
    /// assert_eq!(json!(2), res.payload);
    /// # Ok(())
    /// # }
    /// ```
    #[builder]
    pub fn evaluate(self: &Arc<Self>, state: Option<Arc<State>>) -> Result<Solution<R>> {
        if state.is_some() {
            bind_vm(&self.vm, self.input.clone())
                .maybe_next(self.source.next.clone())
                .maybe_store(self.store.clone())
                .maybe_state(state)
                .maybe_allowed_env_vars(self.allowed_env_vars.clone())
                .call()?;
        }

        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);

        let max_memory = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();
        self.vm.set_interrupt({
            let max_memory = max_memory.clone();
            move |vm| {
                max_memory.fetch_max(vm.used_memory(), Ordering::Relaxed);
                if start.elapsed() > timeout {
                    vm.remove_interrupt();
                    return Err(LuaError::runtime("timeout"));
                }
                Ok(LuaVmState::Continue)
            }
        });

        let script_name = &self.source.name;
        let chunk = self.vm.load(&*self.compiled);
        let chunk = match script_name {
            Some(name) => chunk.set_name(name.to_string()),
            None => chunk,
        };

        let result = {
            let _s = trace_span!("evaluate").entered();
            let awaited = futures::executor::block_on(chunk.eval_async())?;
            self.vm.from_value(awaited)?
        };

        let duration = start.elapsed();
        let max_memory = max_memory.load(Ordering::Acquire);
        debug!(?duration, ?script_name, ?max_memory, "script evaluated");
        let solution = Solution::builder(self.clone())
            .duration(duration)
            .max_memory_usage(max_memory)
            .payload(result)
            .build();
        Ok(solution)
    }

    /// Schedule the script.
    pub fn schedule(self: &Arc<Self>, options: &ScheduleOptions) {
        let bail = options.bail;
        debug!(bail, "script scheduled");
        let mut error_count = 0usize;
        loop {
            let now = Utc::now();
            if let Some(next) = options.schedule.upcoming(Utc).take(1).next() {
                debug!(%next, "next run");
                let elapsed = next - now;
                thread::sleep(elapsed.to_std().expect("failed to fetch next schedule"));
                if let Err(err) = self.clone().evaluate().call() {
                    warn!(?err, "failed to evaluate");
                    if bail > 0 {
                        debug!(bail, error_count, "check bail threshold");
                        error_count += 1;
                        if error_count == bail {
                            error!("bail because threshold reached");
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Render the errors. Delegate to [`crate::LuaSource::write_errors`].
    pub fn write_errors<W>(&self, f: W, errors: Vec<&Error>) -> Result<()>
    where
        W: Write,
    {
        Ok(self.source.write_errors(f, errors).call()?)
    }
}

impl<R> Evaluation<R>
where
    for<'lua> R: 'lua + Read + Send + Seek,
{
    /// Rewind the input.
    pub fn rewind_input(self: &Arc<Self>) -> Result<()> {
        Ok(self.input.lock().rewind()?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use std::{
        fs,
        io::{Cursor, empty},
        sync::Arc,
        time::{Duration, Instant},
    };
    use test_case::test_case;

    use crate::{Evaluation, LuaSource, State, StateKey, Store};

    #[tokio::test]
    async fn call_next() {
        let input = "1";
        let next_source: LuaSource = r#"
        return io.read('*n')
        "#
        .into();
        let mut source: LuaSource = r#"
        local m = require('@lmb')
        return m:next() + 1
        "#
        .into();
        source.next = Some(Box::new(next_source));
        let e = Evaluation::builder(source, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(2), res.payload);
    }

    #[test_case("./lua-examples/error.lua")]
    fn error_in_script(path: &str) {
        let script = fs::read_to_string(path).unwrap();
        let e = Evaluation::builder(script, empty()).build().unwrap();
        assert!(e.evaluate().call().is_err());
    }

    #[test_case("algebra.lua", "2", 4.into())]
    #[test_case("count-bytes.lua", "A", json!({ "65": 1 }))]
    #[test_case("hello.lua", "", json!(null))]
    #[test_case("input.lua", "lua", json!(null))]
    #[test_case("read-unicode.lua", "你好，世界", "你好".into())]
    #[test_case("return-table.lua", "123", json!({ "bool": true, "num": 1.23, "str": "hello" }))]
    #[test_case("store.lua", "", json!({ "a": 1 }))]
    fn evaluate_examples(filename: &str, input: &'static str, expected: Value) {
        let script = fs::read_to_string(format!("./lua-examples/{filename}")).unwrap();
        let store = Store::default();
        let e = Evaluation::builder(script, input.as_bytes())
            .store(store)
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn evaluate_infinite_loop() {
        let timer = Instant::now();
        let timeout = Duration::from_millis(100);
        let script = r#"while true do end"#;
        let e = Evaluation::builder(script, empty())
            .timeout(timeout)
            .build()
            .unwrap();
        let res = e.evaluate().call();
        assert!(res.is_err());

        let elapsed = timer.elapsed().as_millis();
        assert!(elapsed < 500, "actual elapsed {elapsed:?}"); // 500% error
    }

    #[test_case("return 1+1", json!(2))]
    #[test_case("return 'a'..1", json!("a1"))]
    #[test_case("return require('@lmb')._VERSION", json!(env!("APP_VERSION")))]
    fn evaluate_scripts(script: &str, expected: Value) {
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test]
    fn reevaluate() {
        let input = "foo\nbar";
        let script = "return io.read('*l')";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("foo"), res.payload);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("bar"), res.payload);
    }

    #[test]
    fn rewind_input() {
        let input = Cursor::new("0");
        let script = "return io.read('*a')";
        let e = Evaluation::builder(script, input).build().unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("0"), res.payload);

        e.rewind_input().unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("0"), res.payload);
    }

    #[test]
    fn with_state() {
        let script = r#"return require("@lmb").request"#;
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let state = Arc::new(State::new());
        state.insert(StateKey::Request, 1.into());
        {
            let res = e.evaluate().state(state.clone()).call().unwrap();
            assert_eq!(json!(1), res.payload);
        }
        state.insert(StateKey::Request, 2.into());
        {
            let res = e.evaluate().state(state.clone()).call().unwrap();
            assert_eq!(json!(2), res.payload);
        }
    }

    #[test]
    fn write_solution() {
        let script = "return 1+1";
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let solution = e.evaluate().call().unwrap();
        let mut buf = String::new();
        solution.write(&mut buf).call().unwrap();
        assert_eq!("2", buf);
    }
}
