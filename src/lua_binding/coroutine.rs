use mlua::prelude::*;

pub struct LuaModCoroutine {}

impl LuaUserData for LuaModCoroutine {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("join_all", |_, _, coroutines: LuaTable| async move {
            let mut tasks = vec![];
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine.into_lua_err()?;
                let task = coroutine.into_async::<LuaValue>(());
                tasks.push(task);
            }
            let joined = futures::future::join_all(tasks).await;
            let mut results = vec![];
            for result in joined {
                let result = result?;
                results.push(result);
            }
            Ok(results)
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[test]
    fn join_all() {
        let script = r#"
        local m = require('@lmb')
        local i = 0
        m.coroutine:join_all({
            coroutine.create(function() i = i + 1 end),
            coroutine.create(function() i = i + 2 end),
        })
        return i
        "#;
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(3), res.payload);
    }
}
