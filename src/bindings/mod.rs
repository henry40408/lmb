use std::{collections::HashMap, env, str, sync::Arc};

use bon::bon;
use mlua::prelude::*;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _};
use tracing::{Instrument, debug_span};

use crate::{LmbInput, Permissions};

macro_rules! define_codec_binding {
    ($name:ident, $span_prefix:literal, $decode:expr, $encode:expr) => {
        pub(crate) struct $name;

        impl mlua::prelude::LuaUserData for $name {
            fn add_methods<M: mlua::prelude::LuaUserDataMethods<Self>>(methods: &mut M) {
                use mlua::prelude::*;

                methods.add_function("decode", |vm, s: String| {
                    let _ = tracing::debug_span!(concat!("decode_", $span_prefix)).entered();
                    let parsed = ($decode)(&s).into_lua_err()?;
                    vm.to_value(&parsed)
                });
                methods.add_function("encode", |vm, value: LuaValue| {
                    let _ = tracing::debug_span!(concat!("encode_", $span_prefix)).entered();
                    let encoded = ($encode)(&value).into_lua_err()?;
                    vm.to_value(&encoded)
                });
            }
        }
    };
}

pub(crate) use define_codec_binding;

pub(crate) mod coroutine;
pub(crate) mod crypto;
pub(crate) mod globals;
pub(crate) mod http;
pub(crate) mod io;
pub(crate) mod json;
pub(crate) mod json_path;
pub(crate) mod logging;
pub(crate) mod store;
pub(crate) mod toml;
pub(crate) mod yaml;

pub(crate) struct Binding {
    permissions: Option<Permissions>,
    reader: LmbInput,
}

#[bon]
impl Binding {
    #[builder]
    pub fn new(#[builder(start_fn)] reader: LmbInput, permissions: Option<Permissions>) -> Self {
        Self {
            permissions,
            reader,
        }
    }
}

impl LuaUserData for Binding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("getenv", |vm, this, key: String| {
            if let Some(p) = &this.permissions
                && p.is_env_allowed(&key)
                && let Ok(val) = env::var(&key)
            {
                return vm.to_value(&val);
            }
            Ok(LuaNil)
        });

        methods.add_method("getenvs", |vm, this, ()| {
            let mut vars = HashMap::new();
            for (k, v) in env::vars() {
                if let Some(p) = &this.permissions
                    && p.is_env_allowed(&k)
                {
                    vars.insert(k, v);
                }
            }
            vm.to_value(&vars)
        });

        methods.add_async_method("read_unicode", |vm, this, fmt: LuaValue| {
            let span = debug_span!("read_unicode", fmt = ?fmt);
            async move {
                let reader = &this.reader;

                if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                    match &*f {
                        "*a" | "*all" => {
                            let mut buf = String::new();
                            reader.lock().await.read_to_string(&mut buf).await?;
                            return vm.to_value(&buf);
                        }
                        "*l" | "*line" => {
                            let mut line = String::new();
                            if reader.lock().await.read_line(&mut line).await? == 0 {
                                return Ok(LuaNil);
                            }
                            return vm.to_value(&line.trim_end());
                        }
                        _ => {
                            return Err(LuaError::BadArgument {
                                to: Some("read".to_string()),
                                pos: 1,
                                name: None,
                                cause: Arc::new(LuaError::external(format!("invalid format {f}"))),
                            });
                        }
                    }
                }

                if let Some(n) = fmt.as_usize() {
                    let mut remaining = n;
                    let mut buf = vec![];
                    let mut single = [0u8; 1];
                    // Hold the lock for the entire read operation to avoid repeated locking
                    let mut reader_guard = reader.lock().await;
                    while remaining > 0 {
                        let count = reader_guard.read(&mut single).await?;
                        if count == 0 {
                            break;
                        }
                        buf.extend_from_slice(&single);
                        if str::from_utf8(&buf).is_ok() {
                            remaining -= 1;
                        }
                    }
                    drop(reader_guard);
                    if buf.is_empty() {
                        return Ok(LuaNil);
                    }
                    return Ok(str::from_utf8(&buf).ok().map_or_else(
                        || LuaNil,
                        |s| {
                            vm.create_string(s)
                                .map_or_else(|_| LuaNil, LuaValue::String)
                        },
                    ));
                }

                Err(LuaError::BadArgument {
                    to: Some("read".to_string()),
                    pos: 1,
                    name: None,
                    cause: Arc::new(LuaError::external(format!("invalid option {fmt:?}"))),
                })
            }
            .instrument(span)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;
    use test_case::test_case;

    use crate::Runner;

    #[tokio::test]
    async fn test_read_unicode_all() {
        let text = [
            "Hello, world!",
            "你好，世界",
            "こんにちは世界!",
            "안녕하세요 세계!",
            "Привет, мир!",
            "مرحبا بالعالم",
            "😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚",
        ];
        let source = include_str!("../fixtures/bindings/io/read-unicode-all.lua");
        let input = Cursor::new(text.join("\n"));
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(result.result.unwrap().as_str().unwrap(), text.join("\n"));
    }

    #[tokio::test]
    async fn test_read_unicode_line() {
        let text = [
            "Hello, world!",
            "你好，世界",
            "こんにちは世界!",
            "안녕하세요 세계!",
            "Привет, мир!",
            "مرحبا بالعالم",
            "😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚",
        ];
        let source = include_str!("../fixtures/bindings/io/read-unicode-line.lua");
        let input = Cursor::new(text.join("\n"));
        let runner = Runner::builder(source, input).build().unwrap();
        assert_eq!(
            json!("Hello, world!"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("你好，世界"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("こんにちは世界!"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("안녕하세요 세계!"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("Привет, мир!"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("مرحبا بالعالم"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!("😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚"),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
        assert_eq!(
            json!(null),
            runner.invoke().call().await.unwrap().result.unwrap()
        );
    }

    #[test_case("Hello, world!"; "English")]
    #[test_case("你好，世界"; "Chinese")]
    #[test_case("こんにちは世界"; "Japanese")]
    #[test_case("안녕하세요 세계"; "Korean")]
    #[test_case("Привет, мир"; "Russian")]
    #[test_case("مرحبا بالعالم"; "Arabic")]
    #[test_case("😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚"; "Emoji")]
    #[tokio::test]

    async fn test_read_unicode_count(text: &'static str) {
        let source = include_str!("../fixtures/bindings/io/read-unicode.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();

        let mut actual = vec![];
        while let Some(s) = runner
            .invoke()
            .call()
            .await
            .unwrap()
            .result
            .unwrap()
            .as_str()
            .map(|s| s.to_string().into_boxed_str())
        {
            actual.push(s);
        }
        assert_eq!(actual.join(""), text);
    }
}
