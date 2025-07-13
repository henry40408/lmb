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
    use std::{io::Cursor, time::Duration};

    use serde_json::json;
    use tokio::io::empty;

    use crate::Evaluation;

    #[tokio::test]
    async fn join_all() {
        let ms = 100;
        let input = Cursor::new(format!("{ms}"));
        let script = include_str!("fixtures/join-all.lua");
        let e = Evaluation::builder(script, input)
            .timeout(Duration::from_millis(ms * 5 / 4))
            .build()
            .unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(null), res.payload);
    }

    #[tokio::test]
    async fn race() {
        let script = include_str!("fixtures/race.lua");
        let e = Evaluation::builder(script, empty()).build().unwrap();
        let res = e.evaluate_async().call().await.unwrap();
        assert_eq!(json!(100), res.payload);
    }
}
