use std::sync::Arc;

use bon::bon;
use mlua::prelude::*;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncSeek};

use crate::LmbInput;

pub mod io;
pub mod json;
pub mod store;

pub(crate) struct Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    reader: LmbInput<R>,
}

#[bon]
impl<R> Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    #[builder]
    pub fn new(#[builder(start_fn)] reader: LmbInput<R>) -> Self {
        Self { reader }
    }
}

impl<R> LuaUserData for Binding<R>
where
    for<'lua> R: 'lua + AsyncRead + AsyncSeek + Unpin,
{
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("read_unicode", async |vm, this, fmt: LuaValue| {
            let reader = &this.reader;

            if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                match &*f {
                    "*a" | "*all" => {
                        let mut buf = String::new();
                        reader.lock().read_to_string(&mut buf).await?;
                        return vm.to_value(&buf);
                    }
                    "*l" | "*line" => {
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
                            cause: Arc::new(LuaError::runtime("invalid format")),
                        });
                    }
                }
            }

            if let Some(n) = fmt.as_usize() {
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
                cause: Arc::new(LuaError::runtime("invalid option")),
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use test_case::test_case;

    use crate::Runner;

    #[test_case("Hello, world!"; "English")]
    #[test_case("你好，世界"; "Chinese")]
    #[test_case("こんにちは世界"; "Japanese")]
    #[test_case("안녕하세요 세계"; "Korean")]
    #[test_case("Привет, мир"; "Russian")]
    #[test_case("مرحبا بالعالم"; "Arabic")]
    #[test_case("😀😃😄😁😆😅😂🤣😊😇🙂🙃😉😌😍🥰😘😗😙😚"; "Emoji")]
    #[tokio::test]

    async fn test_read_unicode(text: &'static str) {
        let source = include_str!("fixtures/read-unicode.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(&source, input).build().unwrap();

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
