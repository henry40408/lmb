use mlua::prelude::*;
use serde_json::Value;
use serde_json_path::JsonPath;

pub(crate) struct JsonPathBinding;

impl LuaUserData for JsonPathBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("query", |vm, (path, data): (String, LuaValue)| {
            let data: Value = vm.from_value(data)?;
            let path = JsonPath::parse(&path).into_lua_err()?;
            let result = path.query(&data).all();
            vm.to_value(&result)
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_json_path() {
        let source = include_str!("../fixtures/bindings/codecs/json-path.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
