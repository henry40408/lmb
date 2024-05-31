use mlua::prelude::*;
use std::io::{stderr, stdout, Read, Write as _};

use crate::{LmbInput, LmbResult, LmbState, LmbStateKey, LmbStore, LmbValue};

use crypto::*;
use http::*;
use json::*;
use read::*;

mod crypto;
mod http;
mod json;
mod read;

// ref: https://www.lua.org/pil/8.1.html
const K_LOADED: &str = "_LOADED";

/// Interface of Lmb between Lua and Rust.
pub struct LuaLmb<R>
where
    R: Read,
{
    input: LmbInput<R>,
    state: Option<LmbState>,
    store: Option<LmbStore>,
}

impl<R> LuaLmb<R>
where
    for<'lua> R: 'lua + Read + Send,
{
    /// Create a new instance of interface with input [`LmbInput`] and store [`LmbStore`].
    ///
    /// <div class="warning">Export for benchmarking, but end-user should not directly use it.</div>
    ///
    /// ```rust
    /// # use std::{io::{Cursor, BufReader}, sync::Arc};
    /// # use parking_lot::Mutex;
    /// use lmb::*;
    /// let input = Arc::new(Mutex::new(BufReader::new(Cursor::new("0"))));
    /// let store = LmbStore::default();
    /// let _ = LuaLmb::new(input, Some(store), None);
    /// ```
    pub fn new(input: LmbInput<R>, store: Option<LmbStore>, state: Option<LmbState>) -> Self {
        Self {
            input,
            state,
            store,
        }
    }

    /// Register the interface to a Lua virtual machine.
    ///
    /// ```rust
    /// # use std::{io::{Cursor, BufReader}, sync::Arc};
    /// # use mlua::prelude::*;
    /// # use parking_lot::Mutex;
    /// use lmb::*;
    /// let vm = Lua::new();
    /// let input = Arc::new(Mutex::new(BufReader::new(Cursor::new("0"))));
    /// let store = LmbStore::default();
    /// let _ = LuaLmb::register(&vm, input, Some(store), None);
    /// ```
    pub fn register(
        vm: &Lua,
        input: LmbInput<R>,
        store: Option<LmbStore>,
        state: Option<LmbState>,
    ) -> LmbResult<()> {
        let io_table = vm.create_table()?;

        let read_fn = vm.create_function({
            let input = input.clone();
            move |vm, f: Option<LuaValue<'_>>| lua_lmb_read(vm, &input, f)
        })?;
        io_table.set("read", read_fn)?;

        io_table.set("stderr", LmbStderr {})?;

        let write_fn = vm.create_function(|_, vs: LuaMultiValue<'_>| {
            let mut locked = stdout().lock();
            for v in vs.into_vec() {
                write!(locked, "{}", v.to_string()?)?;
            }
            Ok(())
        })?;
        io_table.set("write", write_fn)?;

        let globals = vm.globals();
        globals.set("io", io_table)?;

        let loaded = vm.named_registry_value::<LuaTable<'_>>(K_LOADED)?;
        loaded.set("@lmb", Self::new(input, store, state))?;
        loaded.set("@lmb/crypto", LuaLmbCrypto {})?;
        loaded.set("@lmb/http", LuaLmbHTTP {})?;
        loaded.set("@lmb/json", LuaLmbJSON {})?;
        vm.set_named_registry_value(K_LOADED, loaded)?;

        Ok(())
    }
}

struct LmbStderr {}

impl LuaUserData for LmbStderr {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("write", |_, _, vs: LuaMultiValue<'_>| {
            let mut locked = stderr().lock();
            for v in vs.into_vec() {
                write!(locked, "{}", v.to_string()?)?;
            }
            Ok(())
        });
    }
}

fn lua_lmb_get<R>(_: &Lua, lmb: &LuaLmb<R>, key: String) -> LuaResult<LmbValue>
where
    R: Read,
{
    let Some(store) = &lmb.store else {
        return Ok(LmbValue::None);
    };
    if let Ok(v) = store.get(key.as_str()) {
        return Ok(v);
    }
    Ok(LmbValue::None)
}

fn lua_lmb_set<R>(_: &Lua, lmb: &LuaLmb<R>, (key, value): (String, LmbValue)) -> LuaResult<LmbValue>
where
    R: Read,
{
    let Some(store) = &lmb.store else {
        return Ok(LmbValue::None);
    };
    store.put(key, &value).into_lua_err()?;
    Ok(value)
}

fn lua_lmb_update<'lua, R>(
    vm: &'lua Lua,
    lmb: &LuaLmb<R>,
    (key, f, default_v): (String, LuaFunction<'lua>, Option<LmbValue>),
) -> LuaResult<LmbValue>
where
    R: Read,
{
    let update_fn = |old: &mut LmbValue| -> LuaResult<()> {
        let old_v = vm.to_value(old)?;
        let new = f.call::<_, LmbValue>(old_v)?;
        *old = new;
        Ok(())
    };

    let Some(store) = &lmb.store else {
        return Ok(LmbValue::None);
    };

    store.update(key, update_fn, default_v).into_lua_err()
}

impl<R> LuaUserData for LuaLmb<R>
where
    for<'lua> R: 'lua + Read,
{
    fn add_fields<'lua, F: LuaUserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("APP_VERSION"));
        fields.add_field_method_get("request", |vm, this| {
            let Some(m) = &this.state else {
                return Ok(LuaNil);
            };
            let Some(v) = m.get(&LmbStateKey::Request) else {
                return Ok(LuaNil);
            };
            vm.to_value(&*v)
        });
    }

    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get", lua_lmb_get);
        methods.add_method("read_unicode", |vm, this, f| {
            lua_lmb_read_unicode(vm, &this.input, f)
        });
        methods.add_method("set", lua_lmb_set);
        methods.add_method("update", lua_lmb_update);
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;
    use test_case::test_case;

    use crate::{EvaluationBuilder, LmbValue};

    #[test]
    fn read_binary() {
        let input: &[u8] = &[1, 2, 3];
        let script = r#"
        local s = io.read('*a')
        local t = {}
        for b in (s or ""):gmatch('.') do
          table.insert(t, string.byte(b))
        end
        return t
        "#;
        let e = EvaluationBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(
            LmbValue::from(vec![1.into(), 2.into(), 3.into()]),
            res.payload
        );
    }

    #[test_case("assert(not io.read())")]
    #[test_case("assert(not io.read('*a'))")]
    #[test_case("assert(not io.read('*l'))")]
    #[test_case("assert(not io.read('*n'))")]
    #[test_case("assert(not io.read(1))")]
    fn read_empty(script: &'static str) {
        let e = EvaluationBuilder::new(script, empty()).build();
        let _ = e.evaluate().unwrap();
    }

    #[test_case("1", 1.into())]
    #[test_case("1.2", 1.2.into())]
    #[test_case("1.23e-10", 0.000000000123.into())]
    #[test_case("", LmbValue::None)]
    #[test_case("x", LmbValue::None)]
    #[test_case("1\n", 1.into())]
    fn read_number(input: &'static str, expected: LmbValue) {
        let script = "return io.read('*n')";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test_case("return io.read()", "foo".into())]
    #[test_case("return io.read('*a')", "foo\nbar".into())]
    #[test_case("return io.read('*l')", "foo".into())]
    #[test_case("return io.read(1)", "f".into())]
    #[test_case("return io.read(4)", "foo\n".into())]
    fn read_string(script: &str, expected: LmbValue) {
        let input = "foo\nbar";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test_case(1, "你")]
    #[test_case(2, "你好")]
    #[test_case(3, "你好")]
    fn read_unicode_cjk_characters(n: usize, expected: &str) {
        let script = format!("return require('@lmb'):read_unicode({n})");
        let input = "你好";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(expected), res.payload);
    }

    #[test]
    fn read_unicode_cjk_characters_sequentially() {
        let input = "你好";
        let script = "return require('@lmb'):read_unicode(1)";

        let e = EvaluationBuilder::new(script, input.as_bytes()).build();

        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from("你"), res.payload);

        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from("好"), res.payload);

        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::None, res.payload);
    }

    #[test_case("你好\n世界", "*a", "你好\n世界")]
    #[test_case("你好\n世界", "*l", "你好")]
    #[test_case("你好", "*a", "你好")]
    fn read_unicode_format(input: &'static str, f: &str, expected: &str) {
        let script = format!(r#"return require('@lmb'):read_unicode('{f}')"#);
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::String(expected.to_string()), res.payload);
    }

    #[test]
    fn read_unicode_invalid_sequence() {
        // ref: https://www.php.net/manual/en/reference.pcre.pattern.modifiers.php#54805
        let input: &[u8] = &[0xf0, 0x28, 0x8c, 0xbc];
        let script = "return require('@lmb'):read_unicode(1)";
        let e = EvaluationBuilder::new(script, input).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::None, res.payload);
    }

    #[test]
    fn read_unicode_mixed_characters() {
        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let script = "return require('@lmb'):read_unicode(12)";
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(input), res.payload);
    }

    #[test_case(1, "a")]
    #[test_case(2, "ab")]
    #[test_case(3, "ab")]
    fn read_unicode_non_cjk_characters(n: usize, expected: &str) {
        let input = "ab";
        let script = format!("return require('@lmb'):read_unicode({n})");
        let e = EvaluationBuilder::new(script, input.as_bytes()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::from(expected), res.payload);
    }

    #[test]
    fn write() {
        let script = "io.write('l', 'a', 'm'); return nil";
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::None, res.payload);

        let script = "io.stderr:write('err', 'or'); return nil";
        let e = EvaluationBuilder::new(script, empty()).build();
        let res = e.evaluate().unwrap();
        assert_eq!(LmbValue::None, res.payload);
    }
}
