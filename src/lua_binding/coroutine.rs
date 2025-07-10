use futures::{StreamExt, stream::FuturesUnordered};
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
        methods.add_async_method("race", |_, _, coroutines: LuaTable| async move {
            let mut tasks = FuturesUnordered::new();
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine.into_lua_err()?;
                let task = coroutine.into_async::<LuaValue>(());
                tasks.push(task);
            }
            let Some(result) = tasks.next().await else {
                return Ok(LuaNil);
            };
            let value = result.into_lua_err()?;
            Ok(value)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[tokio::test]
    async fn join_all() {
        let ms = 100;
        let script = format!(
            r#"
            local m = require('@lmb')
            m.coroutine:join_all({{
                coroutine.create(function()
                    m:sleep_ms({ms})
                end),
                coroutine.create(function()
                    m:sleep_ms({ms})
                end),
            }})
            "#
        );
        let e = Evaluation::builder(script, empty())
            .timeout(Duration::from_millis(ms * 5 / 4))
            .build()
            .unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(null), res.payload);
    }

    #[tokio::test]
    async fn race() {
        let script = r#"
        local m = require('@lmb')
        return m.coroutine:race({
            coroutine.create(function()
                m:sleep_ms(100)
                return 100
            end),
            coroutine.create(function()
                m:sleep_ms(200)
                return 200
            end),
        })
        "#;
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(100), res.payload);
    }
}
