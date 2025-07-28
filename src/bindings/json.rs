use mlua::prelude::*;
use serde_json::Value;

pub(crate) struct JsonBinding;

impl LuaUserData for JsonBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("decode", |vm, json_str: String| {
            let parsed = serde_json::from_str::<Value>(&json_str).into_lua_err()?;
            vm.to_value(&parsed)
        });
        methods.add_function("encode", |vm, value: LuaValue| {
            let json_str = serde_json::to_string(&value).into_lua_err()?;
            vm.to_value(&json_str)
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_json_binding() {
        let source = include_str!("fixtures/json.lua");
        let runner = Runner::builder(&source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
