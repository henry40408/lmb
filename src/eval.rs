use bon::{bon, builder, Builder};
use chrono::Utc;
use mlua::prelude::*;
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    fmt::Write,
    io::{BufReader, Read},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, error, trace_span, warn};

use crate::{
    bind_vm, Error, Input, LuaSource, Result, ScheduleOptions, State, Store, DEFAULT_TIMEOUT,
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
    /// Source.
    source: LuaSource,
    /// Input.
    input: Input<R>,
    /// Store.
    store: Option<Store>,
    /// Timeout.
    timeout: Option<Duration>,
    /// Lua virtual machine.
    vm: Lua,
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
    ) -> Result<Arc<Evaluation<R>>> {
        let input = Arc::new(Mutex::new(BufReader::new(input)));
        Self::new_with_input(source, input)
            .maybe_store(store)
            .maybe_timeout(timeout)
            .call()
    }

    /// Build evaluation with a wrapped reader.
    #[builder]
    pub fn new_with_input(
        #[builder(start_fn, into)] source: LuaSource,
        #[builder(start_fn)] input: Input<R>,
        store: Option<Store>,
        timeout: Option<Duration>,
    ) -> Result<Arc<Evaluation<R>>> {
        let vm = Lua::new();
        vm.sandbox(true)?;
        bind_vm(&vm, input.clone())
            .maybe_next(source.next.clone())
            .maybe_store(store.clone())
            .call()?;
        Ok(Arc::new(Evaluation {
            input,
            source,
            store,
            timeout,
            vm,
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
                .call()?;
        }

        let timeout = self.timeout.unwrap_or(DEFAULT_TIMEOUT);
        let max_memory = Arc::new(AtomicUsize::new(0));

        let start = Instant::now();
        self.vm.set_interrupt({
            let max_memory = Arc::clone(&max_memory);
            move |vm| {
                let used_memory = vm.used_memory();
                max_memory.fetch_max(used_memory, Ordering::Relaxed);
                if start.elapsed() > timeout {
                    vm.remove_interrupt();
                    return Err(mlua::Error::runtime("timeout"));
                }
                Ok(LuaVmState::Continue)
            }
        });

        let script_name = &self.source.name;
        let chunk = self.vm.load(&**self.source.compile()?);
        let chunk = match script_name {
            Some(name) => chunk.set_name(name.to_string()),
            None => chunk,
        };

        let _s = trace_span!("evaluate").entered();
        let result = self.vm.from_value(chunk.eval()?)?;

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

    /// Replace the input
    pub fn set_input(self: &Arc<Self>, input: R) {
        *self.input.lock() = BufReader::new(input);
    }

    /// Render the errors. Delegate to [`crate::LuaSource::write_errors`].
    pub fn write_errors<W>(&self, f: W, errors: Vec<&Error>) -> Result<()>
    where
        W: Write,
    {
        Ok(self.source.write_errors(f, errors).call()?)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use std::{
        fs,
        io::empty,
        sync::Arc,
        time::{Duration, Instant},
    };
    use test_case::test_case;

    use crate::{Evaluation, LuaSource, State, StateKey, Store};

    #[test]
    fn call_next() {
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
    #[test_case("store.lua", "", json!([1]))]
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
    fn replace_input() {
        let script = "return io.read('*a')";
        let e = Evaluation::builder(script, &b"0"[..]).build().unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("0"), res.payload);

        e.set_input(&b"1"[..]);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("1"), res.payload);
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
