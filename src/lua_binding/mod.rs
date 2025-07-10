use bon::{Builder, builder};
use dashmap::DashMap;
use mlua::prelude::*;
use serde_json::Value;
use std::{
    io::{Write as _, stderr, stdout},
    sync::Arc,
    time::Duration,
};
use tokio::io::AsyncRead;

use crate::{Evaluation, Input, LuaSource, Result, State, StateKey, Store};

use coroutine::*;
use crypto::*;
use http::*;
use json::*;
use read::*;

mod coroutine;
mod crypto;
mod http;
mod json;
mod read;

// ref: https://www.lua.org/pil/8.1.html
const K_LOADED: &str = "_LOADED";

/// Interface between Lua and Rust.
#[derive(Builder, Debug)]
pub struct LuaBinding<R>
where
    R: AsyncRead + Send + Unpin,
{
    input: Input<R>,
    next: Option<Box<LuaSource>>,
    state: Option<Arc<State>>,
    store: Option<Store>,
    allowed_env_vars: Option<Vec<Box<str>>>,
}

impl<R> LuaUserData for LuaBinding<R>
where
    for<'lua> R: 'lua + AsyncRead + Send + Unpin,
{
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field("_VERSION", env!("APP_VERSION"));
        fields.add_field("coroutine", LuaModCoroutine {});
        fields.add_field("crypto", LuaModCrypto {});
        fields.add_field("http", LuaModHTTP {});
        fields.add_field("json", LuaModJSON {});
        fields.add_field_method_get("request", |vm, this| {
            let Some(v) = this.state.as_ref().and_then(|m| m.get(&StateKey::Request)) else {
                return Ok(LuaNil);
            };
            vm.to_value(&*v)
        });
        fields.add_field_method_get("response", |vm, this| {
            let Some(v) = this.state.as_ref().and_then(|m| m.get(&StateKey::Response)) else {
                return Ok(LuaNil);
            };
            vm.to_value(&*v)
        });
        fields.add_field_method_set("response", |vm, this, value: LuaValue| {
            if let Some(v) = this.state.as_ref() {
                v.insert(StateKey::Response, vm.from_value(value)?);
            }
            Ok(())
        });
        fields.add_field_method_get("store", |_, this| {
            Ok(LuaStoreBinding {
                store: this.store.clone(),
            })
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get_env", |vm, this, key: String| {
            let Some(allowed_vars) = &this.allowed_env_vars else {
                return Ok(LuaNil);
            };
            let key = key.into_boxed_str();
            if !allowed_vars.contains(&key) {
                return Ok(LuaNil);
            }
            match std::env::var(&*key).ok() {
                Some(v) => vm.to_value(&v),
                None => Ok(LuaNil),
            }
        });
        methods.add_async_method("sleep_ms", |_, _, ms: u64| async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            Ok(ms)
        });
        methods.add_async_method("next", |_, this, ()| async move {
            let Some(next) = &this.next else {
                return Ok(LuaNil);
            };
            let next = next.clone();
            let e = Evaluation::new_with_input(*next, this.input.clone())
                .maybe_store(this.store.clone())
                .call()
                .into_lua_err()?;
            let res = e
                .evaluate_async()
                .maybe_state(this.state.clone())
                .call()
                .await
                .into_lua_err()?;
            res.to_lua().call().into_lua_err()
        });
        methods.add_async_method("read_unicode", |vm, this, f| {
            lua_lmb_read_unicode(vm, this.input.clone(), f)
        });
    }
}

/// Bind Lua interface to Lua VM.
#[builder]
pub fn bind_vm<R>(
    #[builder(start_fn)] vm: &Lua,
    #[builder(start_fn)] input: Input<R>,
    next: Option<Box<LuaSource>>,
    store: Option<Store>,
    state: Option<Arc<State>>,
    allowed_env_vars: Option<Vec<Box<str>>>,
) -> Result<()>
where
    for<'lua> R: 'lua + AsyncRead + Send + Unpin,
{
    let io_table = vm.create_table()?;

    let read_fn = vm.create_async_function({
        let input = input.clone();
        move |vm, f: Option<LuaValue>| lua_lmb_read(vm, input.clone(), f)
    })?;
    io_table.set("read", read_fn)?;

    io_table.set("stderr", LuaStderr {})?;

    let write_fn = vm.create_function(|_, vs: LuaMultiValue| {
        let mut locked = stdout().lock();
        for v in vs.into_vec() {
            write!(locked, "{}", v.to_string()?.into_boxed_str())?;
        }
        Ok(())
    })?;
    io_table.set("write", write_fn)?;

    let globals = vm.globals();
    globals.set("io", io_table)?;

    let loaded = vm.named_registry_value::<LuaTable>(K_LOADED)?;
    let binding = LuaBinding::builder()
        .input(input)
        .maybe_next(next)
        .maybe_store(store)
        .maybe_state(state)
        .maybe_allowed_env_vars(allowed_env_vars)
        .build();
    loaded.set("@lmb", binding)?;
    vm.set_named_registry_value(K_LOADED, loaded)?;

    Ok(())
}

struct LuaStderr {}

impl LuaUserData for LuaStderr {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("write", |_, _, vs: LuaMultiValue| {
            let mut locked = stderr().lock();
            let vs = vs.into_vec();
            for (idx, v) in vs.iter().enumerate() {
                write!(locked, "{}", v.to_string()?.into_boxed_str())?;
                if idx != vs.len() - 1 {
                    write!(locked, "\t")?;
                }
            }
            Ok(())
        });
    }
}

struct LuaStoreBinding {
    store: Option<Store>,
}

impl LuaUserData for LuaStoreBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "update",
            |vm, this, (keys, f, default_values): (Vec<String>, LuaFunction, Option<LuaValue>)| {
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };
                let update_fn = |inner: Arc<DashMap<Box<str>, Value>>| -> LuaResult<()> {
                    let snapshot = LuaStoreSnapshot { inner };
                    f.call::<LuaValue>(snapshot)?;
                    Ok(())
                };
                let default_values = match default_values {
                    Some(v) => Some(vm.from_value(v)?),
                    None => None,
                };
                let value = store
                    .update(&keys, update_fn, default_values)
                    .into_lua_err()?;
                vm.to_value(&value)
            },
        );
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            let Some(store) = &this.store else {
                return Ok(LuaNil);
            };
            let value = store.get(key.as_str()).into_lua_err()?;
            match value {
                Value::Null => Ok(LuaNil),
                _ => vm.to_value(&value),
            }
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };
                let serialized = serde_json::to_value(&value).into_lua_err()?;
                store.put(key, &serialized).into_lua_err()?;
                vm.to_value(&value)
            },
        );
    }
}

struct LuaStoreSnapshot {
    inner: Arc<DashMap<Box<str>, Value>>,
}

impl LuaUserData for LuaStoreSnapshot {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: Box<str>| {
            let value = this.inner.entry(key).or_insert_with(|| Value::Null).clone();
            vm.to_value(&value)
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (Box<str>, LuaValue)| {
                let v: Value = vm.from_value(value.clone()).into_lua_err()?;
                this.inner.insert(key, v);
                Ok(value)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use test_case::test_case;
    use tokio::io::empty;

    use crate::Evaluation;

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
        let e = Evaluation::builder(script, input).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!([1, 2, 3]), res.payload);
    }

    #[test_case("assert(not io.read())")]
    #[test_case("assert(not io.read('*a'))")]
    #[test_case("assert(not io.read('*l'))")]
    #[test_case("assert(not io.read('*n'))")]
    #[test_case("assert(not io.read(1))")]
    fn read_empty(script: &'static str) {
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let _ = e.evaluate().call().unwrap();
    }

    #[test_case("1", 1.into())]
    #[test_case("1.2", 1.2.into())]
    #[test_case("1.23e-10", 0.000000000123.into())]
    #[test_case("", json!(null))]
    #[test_case("x", json!(null))]
    #[test_case("1\n", 1.into())]
    fn read_number(input: &'static str, expected: Value) {
        let script = "return io.read('*n')";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test_case("return io.read()", "foo".into())]
    #[test_case("return io.read('*a')", "foo\nbar".into())]
    #[test_case("return io.read('*l')", "foo".into())]
    #[test_case("return io.read(1)", "f".into())]
    #[test_case("return io.read(4)", "foo\n".into())]
    fn read_string(script: &str, expected: Value) {
        let input = "foo\nbar";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(expected, res.payload);
    }

    #[test_case(1, "你")]
    #[test_case(2, "你好")]
    #[test_case(3, "你好")]
    fn read_unicode_cjk_characters(n: usize, expected: &str) {
        let script = format!("return require('@lmb'):read_unicode({n})");
        let input = "你好";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(expected), res.payload);
    }

    #[test]
    fn read_unicode_cjk_characters_sequentially() {
        let input = "你好";
        let script = "return require('@lmb'):read_unicode(1)";

        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("你"), res.payload);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!("好"), res.payload);

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);
    }

    #[test_case("你好\n世界", "*a", "你好\n世界")]
    #[test_case("你好\n世界", "*l", "你好")]
    #[test_case("你好", "*a", "你好")]
    fn read_unicode_format(input: &'static str, f: &str, expected: &str) {
        let script = format!(r#"return require('@lmb'):read_unicode('{f}')"#);
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(expected), res.payload);
    }

    #[test]
    fn read_unicode_invalid_sequence() {
        // ref: https://www.php.net/manual/en/reference.pcre.pattern.modifiers.php#54805
        let input: &[u8] = &[0xf0, 0x28, 0x8c, 0xbc];
        let script = "return require('@lmb'):read_unicode(1)";
        let e = Evaluation::builder(script, input).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);
    }

    #[test]
    fn read_unicode_mixed_characters() {
        // mix CJK and non-CJK characters
        let input = r#"{"key":"你好"}"#;
        let script = "return require('@lmb'):read_unicode(12)";
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(input), res.payload);
    }

    #[test_case(1, "a")]
    #[test_case(2, "ab")]
    #[test_case(3, "ab")]
    fn read_unicode_non_cjk_characters(n: usize, expected: &str) {
        let input = "ab";
        let script = format!("return require('@lmb'):read_unicode({n})");
        let e = Evaluation::builder(script, input.as_bytes())
            .build()
            .unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(expected), res.payload);
    }

    #[test]
    fn write() {
        let script = "io.write('l', 'a', 'm'); return nil";
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);

        let script = "io.stderr:write('err', 'or'); return nil";
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);
    }
}
