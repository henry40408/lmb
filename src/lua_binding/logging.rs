use mlua::prelude::*;
use serde_json::Value;
use tracing::{Level, event};

/// Logging module
pub struct LuaModLogging {}

fn do_log(level: tracing::Level, value: Value) {
    let value = match value {
        Value::String(s) => s,
        Value::Array(arr) => {
            let mut s = String::new();
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    s.push_str("\t");
                }
                let t = match v {
                    Value::String(s) => s.clone(),
                    _ => v.to_string(),
                };
                s.push_str(&t);
            }
            s
        }
        _ => format!("{}", value),
    };
    match level {
        Level::ERROR => event!(target: "(lua)", Level::ERROR, "{}", value),
        Level::WARN => event!(target: "(lua)", Level::WARN, "{}", value),
        Level::INFO => event!(target: "(lua)", Level::INFO, "{}", value),
        Level::DEBUG => event!(target: "(lua)", Level::DEBUG, "{}", value),
        Level::TRACE => event!(target: "(lua)", Level::TRACE, "{}", value),
    }
}

fn lua_multi_value_to_json_value(vm: &Lua, values: &LuaMultiValue) -> Result<Value, LuaError> {
    if values.len() == 1 {
        Ok(vm.from_value(values[0].clone())?)
    } else {
        Ok(Value::Array(
            values
                .iter()
                .map(|v| vm.from_value(v.clone()))
                .collect::<Result<Vec<Value>, _>>()?,
        ))
    }
}

impl LuaUserData for LuaModLogging {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("log", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::INFO, value);
            Ok(())
        });
        methods.add_method("error", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::ERROR, value);
            Ok(())
        });
        methods.add_method("warn", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::WARN, value);
            Ok(())
        });
        methods.add_method("info", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::INFO, value);
            Ok(())
        });
        methods.add_method("debug", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::DEBUG, value);
            Ok(())
        });
        methods.add_method("trace", |vm, _, values: LuaMultiValue| {
            let value = lua_multi_value_to_json_value(vm, &values)?;
            do_log(Level::TRACE, value);
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[test]
    fn test_logging() {
        let script = include_str!("fixtures/logging.lua");
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);
    }
}
