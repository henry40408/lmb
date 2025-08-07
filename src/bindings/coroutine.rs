use futures::{StreamExt as _, stream::FuturesUnordered};
use mlua::prelude::*;

pub(crate) enum Settled {
    Fulfilled(LuaValue),
    Rejected(LuaError),
}

impl LuaUserData for Settled {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| match this {
            Settled::Fulfilled(_) => Ok("fulfilled"),
            Settled::Rejected(_) => Ok("rejected"),
        });
        fields.add_field_method_get("value", |_, this| {
            Ok(match this {
                Settled::Fulfilled(value) => value.clone(),
                Self::Rejected(_) => LuaNil,
            })
        });
        fields.add_field_method_get("reason", |vm, this| {
            Ok(match this {
                Self::Fulfilled(_) => LuaNil,
                Settled::Rejected(err) => match err {
                    LuaError::RuntimeError(msg) => LuaValue::String(vm.create_string(msg)?),
                    _ => LuaValue::String(vm.create_string(err.to_string())?),
                },
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

            let joined = futures::future::join_all(tasks).await;
            let mut results = vec![];
            for result in joined {
                match result {
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
            let value = result?;
            Ok(value)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test]
    async fn test_all_settled() {
        let source = include_str!("fixtures/all-settled.lua");
        let runner = crate::Runner::builder(source, tokio::io::empty())
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

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
