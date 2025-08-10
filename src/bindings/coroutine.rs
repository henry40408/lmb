use futures::{StreamExt as _, stream::FuturesUnordered};
use mlua::prelude::*;

enum Settled {
    Fulfilled(LuaValue),
    Rejected(LuaError),
}

impl LuaUserData for Settled {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| {
            Ok(match this {
                Settled::Fulfilled(_) => "fulfilled",
                Settled::Rejected(_) => "rejected",
            })
        });
        fields.add_field_method_get("value", |_, this| {
            Ok(match this {
                Settled::Fulfilled(value) => value.clone(),
                Settled::Rejected(_) => LuaNil,
            })
        });
        fields.add_field_method_get("reason", |_, this| {
            Ok(match this {
                Settled::Fulfilled(_) => LuaNil,
                Settled::Rejected(err) => LuaValue::Error(Box::new(err.clone())),
            })
        });
    }
}

pub(crate) struct CoroutineBinding;

impl LuaUserData for CoroutineBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_function("all_settled", |_, coroutines: LuaTable| async move {
            let mut tasks = vec![];
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine?;
                let task = coroutine.into_async::<LuaValue>(())?;
                tasks.push(task);
            }
            let mut results = vec![];
            for result in tasks {
                match result.await {
                    Ok(value) => results.push(Settled::Fulfilled(value)),
                    Err(err) => results.push(Settled::Rejected(err)),
                }
            }
            Ok(results)
        });

        methods.add_async_function("join_all", |_, coroutines: LuaTable| async move {
            let mut tasks = vec![];
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine?;
                let task = coroutine.into_async::<LuaValue>(())?;
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

        methods.add_async_function("race", |_, coroutines: LuaTable| async move {
            let mut tasks = FuturesUnordered::new();
            for coroutine in coroutines.sequence_values::<LuaThread>() {
                let coroutine = coroutine?;
                let task = coroutine.into_async::<LuaValue>(())?;
                tasks.push(task);
            }
            let Some(result) = tasks.next().await else {
                return Ok(LuaNil);
            };
            result
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::empty;

    #[tokio::test]
    async fn test_all_settled() {
        let source = include_str!("fixtures/all-settled.lua");
        let runner = crate::Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_join_all() {
        let source = include_str!("fixtures/join-all.lua");
        let runner = crate::Runner::builder(source, empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_race() {
        let source = include_str!("fixtures/race.lua");
        let runner = crate::Runner::builder(source, empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
