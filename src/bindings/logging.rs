//! Logging binding module.
//!
//! This module provides standard log-level functions for Lua scripts.
//! Import via `require("@lmb/logging")`.
//!
//! # Available Methods
//!
//! - `error(...)` - Log at ERROR level.
//! - `warn(...)` - Log at WARN level.
//! - `info(...)` - Log at INFO level.
//! - `debug(...)` - Log at DEBUG level.
//! - `trace(...)` - Log at TRACE level.
//!
//! Each method accepts variadic arguments. Multiple arguments are converted
//! to strings and joined with tab characters (matching Lua `print()` convention).
//! Tables are serialized to JSON.
//!
//! # Example
//!
//! ```lua
//! local log = require("@lmb/logging")
//!
//! log.info("server started", "port", 8080)
//! log.debug("request", { method = "GET", path = "/" })
//! log.error("something went wrong")
//! ```

use mlua::prelude::*;

fn lua_value_to_string(vm: &Lua, value: LuaValue) -> LuaResult<String> {
    match value {
        LuaValue::Nil => Ok("nil".to_string()),
        LuaValue::Boolean(b) => Ok(b.to_string()),
        LuaValue::Integer(n) => Ok(n.to_string()),
        LuaValue::Number(n) => Ok(n.to_string()),
        LuaValue::String(s) => Ok(s.to_string_lossy()),
        LuaValue::Table(_) => {
            let json: serde_json::Value = vm.from_value(LuaValue::Table(
                value.as_table().expect("checked above").clone(),
            ))?;
            Ok(json.to_string())
        }
        LuaValue::Error(e) => Ok(e.to_string()),
        other => Ok(format!("<{}>", other.type_name())),
    }
}

fn lua_values_to_message(vm: &Lua, values: LuaMultiValue) -> LuaResult<String> {
    let parts: Vec<String> = values
        .into_iter()
        .map(|v| lua_value_to_string(vm, v))
        .collect::<LuaResult<_>>()?;
    Ok(parts.join("\t"))
}

macro_rules! add_log_method {
    ($methods:expr, $level:ident) => {
        $methods.add_function(stringify!($level), |vm, values: LuaMultiValue| {
            let message = lua_values_to_message(&vm, values)?;
            tracing::$level!(target: "lmb::lua", "{message}");
            Ok(())
        });
    };
}

/// Logging binding that exposes standard log levels to Lua.
pub(crate) struct LoggingBinding;

impl LuaUserData for LoggingBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        add_log_method!(methods, error);
        add_log_method!(methods, warn);
        add_log_method!(methods, info);
        add_log_method!(methods, debug);
        add_log_method!(methods, trace);
    }
}

#[cfg(test)]
mod tests {
    use mlua::prelude::*;
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_logging() {
        let source = include_str!("../fixtures/bindings/logging.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[test]
    fn test_lua_value_to_string_error() {
        let lua = Lua::new();
        let err = LuaError::runtime("test error");
        let value = LuaValue::Error(Box::new(err));
        let result = super::lua_value_to_string(&lua, value).unwrap();
        assert!(
            result.contains("test error"),
            "Expected error message, got: {result}"
        );
    }
}
