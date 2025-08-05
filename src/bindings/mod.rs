use std::sync::Arc;

use bon::bon;
use mlua::prelude::*;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _};
use tracing::debug_span;

use crate::LmbInput;

pub(crate) mod coroutine;
pub(crate) mod crypto;
pub(crate) mod globals;
pub(crate) mod http;
pub(crate) mod io;
pub(crate) mod json;
pub(crate) mod store;

pub(crate) struct Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    reader: LmbInput<R>,
}

#[bon]
impl<R> Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    #[builder]
    pub fn new(#[builder(start_fn)] reader: LmbInput<R>) -> Self {
        Self { reader }
    }
}

impl<R> LuaUserData for Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("read_unicode", async |vm, this, fmt: LuaValue| {
            let reader = &this.reader;

            if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                match &*f {
                    "*a" | "*all" => {
                        let _ = debug_span!("read unicode all").entered();
                        let mut buf = String::new();
                        reader.lock().read_to_string(&mut buf).await?;
                        return vm.to_value(&buf);
                    }
                    "*l" | "*line" => {
                        let _ = debug_span!("read unicode line").entered();
                        let mut line = String::new();
                        if reader.lock().read_line(&mut line).await? == 0 {
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
                let _ = debug_span!("read unicode", %n).entered();
                let mut remaining = n;
                let mut buf = vec![];
                let mut single = [0u8; 1];
                while remaining > 0 {
                    let count = reader.lock().read(&mut single).await?;
                    if count == 0 {
                        break;
                    }
                    buf.extend_from_slice(&single);
                    if std::str::from_utf8(&buf).is_ok() {
                        remaining -= 1;
                    }
                }
                if buf.is_empty() {
                    return Ok(LuaNil);
                }
                return Ok(std::str::from_utf8(&buf).ok().map_or_else(
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
        let text = "Hello, 世界!\nこんにちは世界!\n안녕하세요 세계!\n😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚";
        let source = include_str!("fixtures/read-unicode-all.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(result.result.unwrap().as_str().unwrap(), text);
    }

    #[tokio::test]
    async fn test_read_unicode_line() {
        let text = "Hello, 世界!\nこんにちは世界!\n안녕하세요 세계!\n😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚";
        let source = include_str!("fixtures/read-unicode-line.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        assert_eq!(
            json!("Hello, 世界!"),
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
    #[test_case("😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚"; "Emoji")]
    #[tokio::test]

    async fn test_read_unicode_count(text: &'static str) {
        let source = include_str!("fixtures/read-unicode.lua");
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
