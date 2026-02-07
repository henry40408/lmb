//! Coroutine utilities binding module.
//!
//! This module provides JavaScript Promise-like utilities for working with Lua coroutines.
//! Import via `require("@lmb/coroutine")`.
//!
//! # Available Methods
//!
//! - `all_settled(coroutines)` - Wait for all coroutines to complete, returning results with status.
//! - `join_all(coroutines)` - Wait for all coroutines to complete successfully, fails if any fails.
//! - `race(coroutines)` - Return the result of the first coroutine to complete.
//!
//! # Example
//!
//! ```lua
//! local co = require("@lmb/coroutine")
//!
//! -- Create coroutines
//! local threads = {
//!     coroutine.create(function() sleep_ms(10); return 1 end),
//!     coroutine.create(function() sleep_ms(20); return 2 end),
//! }
//!
//! -- Wait for all to complete
//! local results = co.join_all(threads)
//!
//! -- Race to get first result
//! local first = co.race(threads)
//!
//! -- Get all results with status
//! local settled = co.all_settled(threads)
//! for _, r in ipairs(settled) do
//!     if r.status == "fulfilled" then
//!         print(r.value)
//!     else
//!         print(r.reason)
//!     end
//! end
//! ```

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

    use crate::Runner;

    #[tokio::test]
    async fn test_all_settled() {
        let source = include_str!("../fixtures/bindings/coroutine/all-settled.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_join_all() {
        let source = include_str!("../fixtures/bindings/coroutine/join-all.lua");
        let runner = Runner::builder(source, empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_race() {
        let source = include_str!("../fixtures/bindings/coroutine/race.lua");
        let runner = Runner::builder(source, empty())
            .timeout(Duration::from_millis(110))
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
