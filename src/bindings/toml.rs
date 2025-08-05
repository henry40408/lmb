use mlua::prelude::*;
use tracing::debug_span;

pub(crate) struct TomlBinding;

impl LuaUserData for TomlBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("decode", |vm, toml_str: String| {
            let _ = debug_span!("decode_toml").entered();
            let parsed = toml::from_str::<toml::Value>(&toml_str).into_lua_err()?;
            vm.to_value(&parsed)
        });
        methods.add_function("encode", |vm, value: LuaValue| {
            let _ = debug_span!("encode_toml").entered();
            let toml_str = toml::to_string(&value).into_lua_err()?;
            vm.to_value(&toml_str)
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_toml_encode_decode() {
        let source = include_str!("fixtures/toml.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
