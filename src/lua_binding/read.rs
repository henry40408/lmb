use mlua::prelude::*;
use tokio::io::{AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _};

use crate::Input;

// This function intentionally uses Lua values instead of JSON values to pass bytes as partial,
// invalid strings, allowing Lua to handle the bytes.
// For a demonstration, see "count-bytes.lua".
pub(crate) async fn lua_lmb_read<R>(
    vm: Lua,
    input: Input<R>,
    f: Option<LuaValue>,
) -> LuaResult<LuaValue>
where
    R: AsyncRead + Unpin,
{
    let Some(f) = f else {
        // This pattern is the default for read, so io.read() has the same effect as io.read("*line").
        // https://www.lua.org/pil/21.1.html
        let mut buf = String::new();
        let count = input.lock().await.read_line(&mut buf).await?;
        if count == 0 {
            return Ok(LuaNil);
        }
        // in Lua, *l doesn't include newline character
        return buf.trim().into_lua(&vm);
    };

    if let Some(f) = f.as_str() {
        // Assume that the input is a valid UTF-8 string,
        // so we can easily convert it to a string in Lua.
        // Otherwise, it will be a list of bytes.
        let mut buf = String::new();
        match f.as_ref() {
            "*a" | "*all" => {
                let count = input.lock().await.read_to_string(&mut buf).await?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                return buf.into_lua(&vm);
            }
            "*l" | "*line" => {
                let count = input.lock().await.read_line(&mut buf).await?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                // in Lua, *l doesn't include newline character
                return buf.trim().into_lua(&vm);
            }
            "*n" | "*number" => {
                let count = input.lock().await.read_to_string(&mut buf).await?;
                if count == 0 {
                    return Ok(LuaNil);
                }
                // in Lua *n returns nil when number is invalid
                let num = buf.trim().parse::<f64>().ok();
                return num.into_lua(&vm);
            }
            _ => {}
        }
    }

    if let Some(i) = f.as_usize() {
        let mut buf = vec![0; i];
        let count = input.lock().await.read(&mut buf).await?;
        if count == 0 {
            return Ok(LuaNil);
        }
        buf.truncate(count);
        // Unlike Rust strings, Lua strings may not be valid UTF-8.
        // We leverage this trait to give Lua the power to handle binary.
        return Ok(LuaValue::String(vm.create_string(&buf)?));
    }

    let f = f.to_string()?;
    Err(LuaError::runtime(format!("unexpected format {f}")))
}

pub(crate) async fn lua_lmb_read_unicode<R>(
    vm: Lua,
    input: Input<R>,
    f: LuaValue,
) -> LuaResult<LuaValue>
where
    R: AsyncRead + Unpin,
{
    if let Some(f) = f.as_str() {
        match f.as_ref() {
            "*a" | "*all" => {
                let mut s = vec![];
                input
                    .lock()
                    .await
                    .read_to_end(&mut s)
                    .await
                    .into_lua_err()?;
                return Ok(LuaValue::String(vm.create_string(s)?));
            }
            "*l" | "*line" => {
                let mut s = String::new();
                input.lock().await.read_line(&mut s).await.into_lua_err()?;
                return Ok(LuaValue::String(vm.create_string(s.trim())?));
            }
            _ => {}
        }
    }

    if let Some(n) = f.as_usize() {
        let mut remaining = n;
        let mut buf = vec![];
        let mut single = [0u8; 1];
        while remaining > 0 {
            let count = input.lock().await.read(&mut single).await?;
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

    let f = f.to_string()?;
    Err(LuaError::runtime(format!("unexpected format {f}")))
}
