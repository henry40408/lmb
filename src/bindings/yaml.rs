use mlua::prelude::*;
use tracing::debug_span;

pub(crate) struct YamlBinding;

impl LuaUserData for YamlBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("decode", |vm, yaml_str: String| {
            let _ = debug_span!("decode YAML").entered();
            let parsed = serde_yaml::from_str::<serde_yaml::Value>(&yaml_str).into_lua_err()?;
            vm.to_value(&parsed)
        });
        methods.add_function("encode", |vm, value: LuaValue| {
            let _ = debug_span!("encode YAML").entered();
            let yaml_str = serde_yaml::to_string(&value).into_lua_err()?;
            vm.to_value(&yaml_str)
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_yaml_encode_decode() {
        let source = include_str!("fixtures/yaml.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
