use futures::{StreamExt as _, stream::FuturesUnordered};
use mlua::prelude::*;

pub(crate) struct CoroutineBinding;

impl LuaUserData for CoroutineBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_function("join_all", |_, coroutines: LuaTable| async move {
            let mut tasks = vec![];
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine.into_lua_err()?;
                let task = coroutine.into_async::<LuaValue>(()).into_lua_err()?;
                tasks.push(task);
            }
            let joined = futures::future::join_all(tasks).await;
            let mut results = vec![];
            for result in joined {
                let result = result.into_lua_err()?;
                results.push(result);
            }
            Ok(results)
        });

        methods.add_async_function("race", |_, coroutines: LuaTable| async move {
            let mut tasks = FuturesUnordered::new();
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine.into_lua_err()?;
                let task = coroutine.into_async::<LuaValue>(()).into_lua_err()?;
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

    #[tokio::test]
    async fn test_join_all() {
        let source = include_str!("fixtures/join-all.lua");
        let runner = crate::Runner::builder(source, tokio::io::empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_race() {
        let source = include_str!("fixtures/race.lua");
        let runner = crate::Runner::builder(source, tokio::io::empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
