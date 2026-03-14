use bon::Builder;
use mlua::prelude::*;
use serde_json::Value;
use tracing::debug_span;

use crate::LmbStore;

/// Lua `UserData` for transactional operations within `store.tx()`.
/// Holds a clone of the `LmbStore` Arc; the outer `tx()` method holds the
/// transaction open for the duration of the callback.
pub(crate) struct TxBinding {
    inner: LmbStore,
}

impl LuaUserData for TxBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |vm, this, key: String| {
            let _ = debug_span!("tx_get", %key).entered();
            let value = this.inner.get(&key).into_lua_err()?;
            if let Some(value) = &value {
                vm.to_value(value)
            } else {
                Ok(LuaNil)
            }
        });

        methods.add_method("set", |vm, this, (key, value): (String, LuaValue)| {
            let _ = debug_span!("tx_set", %key).entered();
            let value: Value = vm.from_value(value)?;
            this.inner.put(&key, &value).into_lua_err()?;
            Ok(LuaNil)
        });

        methods.add_method("del", |_vm, this, key: String| {
            let _ = debug_span!("tx_del", %key).entered();
            this.inner.del(&key).into_lua_err()
        });
    }
}

#[derive(Builder)]
pub(crate) struct StoreBinding {
    store: Option<LmbStore>,
}

impl LuaUserData for StoreBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "get",
            |vm, this, (key, opts): (String, Option<LuaTable>)| {
                let _ = debug_span!("store_get", %key).entered();
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };
                if let Some(value) = &store.get(&key).into_lua_err()? {
                    return vm.to_value(value);
                }
                // Return default if provided
                if let Some(opts) = opts
                    && let Ok(default) = opts.get::<LuaValue>("default")
                    && default != LuaNil
                {
                    return Ok(default);
                }
                Ok(LuaNil)
            },
        );

        methods.add_method("set", |vm, this, (key, value): (String, LuaValue)| {
            let _ = debug_span!("store_set", %key).entered();
            let Some(store) = &this.store else {
                return Ok(LuaNil);
            };
            let value: Value = vm.from_value(value)?;
            store.put(&key, &value).into_lua_err()?;
            Ok(LuaNil)
        });

        methods.add_method("del", |_vm, this, key: String| {
            let _ = debug_span!("store_del", %key).entered();
            let Some(store) = &this.store else {
                return Ok(false);
            };
            store.del(&key).into_lua_err()
        });

        methods.add_method("has", |_vm, this, key: String| {
            let _ = debug_span!("store_has", %key).entered();
            let Some(store) = &this.store else {
                return Ok(false);
            };
            store.has(&key).into_lua_err()
        });

        methods.add_method("keys", |vm, this, pattern: Option<String>| {
            let _ = debug_span!("store_keys").entered();
            let Some(store) = &this.store else {
                return vm.create_table().map(LuaValue::Table);
            };
            let keys = store.keys(pattern.as_deref()).into_lua_err()?;
            let table = vm.create_table()?;
            for (i, key) in keys.into_iter().enumerate() {
                table.set(i + 1, key)?;
            }
            Ok(LuaValue::Table(table))
        });

        methods.add_method("tx", |_vm, this, f: LuaFunction| {
            let span = debug_span!("store_tx").entered();
            let Some(store) = &this.store else {
                return Err(LuaError::runtime("store is not available"));
            };

            {
                let _ = debug_span!(parent: &span, "begin").entered();
                store.begin_tx().into_lua_err()?;
            }

            let tx_binding = TxBinding {
                inner: store.clone(),
            };

            let result = {
                let _ = debug_span!(parent: &span, "call").entered();
                f.call::<LuaValue>(tx_binding)
            };

            match result {
                Ok(val) => {
                    let _ = debug_span!(parent: &span, "commit").entered();
                    store.commit_tx().into_lua_err()?;
                    Ok(val)
                }
                Err(e) => {
                    let _ = debug_span!(parent: &span, "rollback").entered();
                    // Best-effort rollback; if this fails too, we still return the original error
                    let _ = store.rollback_tx();
                    Err(e)
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tokio::io::empty;

    use crate::{Runner, store::SqliteBackend};

    #[tokio::test]
    async fn test_store_binding() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_tx() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-tx.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_tx_rollback() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-tx-rollback.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_without_connection() {
        let source = include_str!("../fixtures/bindings/store/store-without-connection.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap().result.unwrap();
        assert_eq!(json!(true), result);
    }

    #[tokio::test]
    async fn test_store_unicode_keys() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-unicode-keys.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_keys() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-keys.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_del() {
        let backend = SqliteBackend::new_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-del.lua");
        let runner = Runner::builder(source, empty())
            .store(Arc::new(backend) as Arc<dyn crate::store::StoreBackend>)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_no_store() {
        use mlua::Lua;

        use super::StoreBinding;

        let vm = Lua::new();
        let ctx = vm.create_table().unwrap();
        ctx.set("store", StoreBinding::builder().build()).unwrap();
        let source = include_str!("../fixtures/bindings/store/store-no-store.lua");
        let func: mlua::Function = vm.load(source).eval().unwrap();
        func.call_async::<mlua::Value>(ctx).await.unwrap();
    }
}
