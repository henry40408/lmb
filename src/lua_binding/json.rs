use mlua::prelude::*;
use serde_json::Value;

/// JSON module
pub struct LuaModJSON {}

impl LuaUserData for LuaModJSON {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("decode", |vm, _, value: String| {
            vm.to_value(&serde_json::from_str::<Value>(&value).into_lua_err()?)
        });
        methods.add_method("encode", |_, _, value: LuaValue| {
            serde_json::to_string(&value).into_lua_err()
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[test]
    fn json() {
        let script = include_str!("fixtures/json.lua");
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(null), res.payload);
    }
}
