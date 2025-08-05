use std::sync::Arc;

use mlua::prelude::*;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _};
use tracing::debug_span;

use crate::{LmbResult, Runner};

pub(crate) fn bind<R>(runner: &mut Runner<R>) -> LmbResult<()>
where
    for<'lua> R: 'lua + AsyncRead + Unpin,
{
    let globals = runner.vm.globals();

    let io = runner.vm.create_table()?;
    let reader = runner.reader.clone();
    io.set(
        "read",
        runner.vm.create_async_function(move |vm, fmt: LuaValue| {
            let reader = reader.clone();
            async move {
                if let Some(f) = fmt.as_string().and_then(|s| s.to_str().ok()) {
                    match &*f {
                        "*a" | "*all" => {
                            let _ = debug_span!("read_all").entered();
                            let mut buf = vec![];
                            reader.lock().read_to_end(&mut buf).await?;
                            return Ok(LuaValue::String(vm.create_string(buf)?));
                        }
                        "*l" | "*line" => {
                            let _ = debug_span!("read_line").entered();
                            let mut line = String::new();
                            if reader.lock().read_line(&mut line).await? == 0 {
                                return Ok(LuaValue::String(vm.create_string("")?));
                            }
                            return Ok(LuaValue::String(vm.create_string(line.trim_end())?));
                        }
                        "*n" | "*number" => {
                            let _ = debug_span!("read_number").entered();
                            let mut buf = String::new();
                            if reader.lock().read_line(&mut buf).await? == 0 {
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
                                cause: Arc::new(LuaError::external(format!("invalid format {f}"))),
                            });
                        }
                    }
                }

                if let Some(n) = fmt.as_usize() {
                    let _ = debug_span!("read_bytes", %n).entered();
                    let mut buf = vec![0; n];
                    let read = reader.lock().read(&mut buf).await?;
                    if read == 0 {
                        return Ok(LuaNil);
                    }
                    buf.truncate(read);
                    // Unlike Rust strings, Lua strings may not be valid UTF-8.
                    // We leverage this trait to give Lua the power to handle binary.
                    return Ok(LuaValue::String(vm.create_string(&buf)?));
                }

                Err(LuaError::BadArgument {
                    to: Some("read".to_string()),
                    pos: 1,
                    name: None,
                    cause: Arc::new(LuaError::external(format!("invalid option {fmt:?}"))),
                })
            }
        })?,
    )?;

    globals.set("io", io)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::{Value, json};
    use test_case::test_case;

    use crate::Runner;

    #[test_case(""; "empty input")]
    #[test_case("one line"; "single line")]
    #[test_case("one line\n"; "single line with trailing newline")]
    #[test_case("first line\nsecond line"; "multiple lines")]
    #[test_case("first line\nsecond line\n"; "multiple lines with trailing newline")]
    #[tokio::test]

    async fn test_read_all(text: &'static str) {
        let source = include_str!("fixtures/read-all.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(json!(text), result.result.unwrap());
    }

    #[test_case(b"", json!(null); "empty input")]
    #[test_case(b"one line", json!("o"); "single line")]
    #[test_case(b"\n", json!("\n"); "newline only")]
    #[test_case(&[1, 2, 3], json!("\u{1}"); "bytes")]
    #[tokio::test]

    async fn test_read_count(bytes: &'static [u8], expected: Value) {
        let source = include_str!("fixtures/read-count.lua");
        let input = Cursor::new(bytes);
        let runner = Runner::builder(source, input).build().unwrap();
        assert_eq!(
            expected,
            runner.invoke().call().await.unwrap().result.unwrap()
        );
    }

    #[tokio::test]
    async fn test_read_invalid_format() {
        let source = include_str!("fixtures/invalid-format.lua");
        let text = "";
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        let err = result.result.err().unwrap();
        assert!(
            err.to_string()
                .contains("bad argument #1 to `read`: invalid format")
        );
    }

    #[test_case("", ""; "empty input")]
    #[test_case("one line", "one line"; "single line")]
    #[test_case("one line\n", "one line"; "single line with trailing newline")]
    #[test_case("first line\nsecond line", "first line"; "multiple lines")]
    #[test_case("first line\nsecond line\n", "first line"; "multiple lines with trailing newline")]
    #[tokio::test]

    async fn test_read_line(text: &'static str, expected: &'static str) {
        let source = include_str!("fixtures/read-line.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(json!(expected), result.result.unwrap());
    }

    #[test_case("1", json!(1); "valid number")]
    #[test_case("1\n", json!(1); "valid number with newline")]
    #[test_case("1\n2.34", json!(1); "multiple lines with valid number")]
    #[test_case("2.34", json!(2.34); "valid float")]
    #[test_case("not a number", json!(null); "invalid number")]
    #[tokio::test]

    async fn test_read_number(text: &'static str, expected: Value) {
        let source = include_str!("fixtures/read-number.lua");
        let input = Cursor::new(text);
        let runner = Runner::builder(source, input).build().unwrap();
        let result = runner.invoke().call().await.unwrap();
        assert_eq!(json!(expected), result.result.unwrap());
    }
}
