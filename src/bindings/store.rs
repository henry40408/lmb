use std::{collections::HashMap, sync::Arc};

use bon::{Builder, bon};
use mlua::prelude::*;
use parking_lot::Mutex as PlMutex;
use rusqlite::params;
use serde_json::Value;
use tracing::debug_span;

use crate::{
    stmt::{SQL_GET, SQL_PUT},
    store::Store,
};

pub(crate) struct StoreSnapshotBinding {
    inner: Arc<PlMutex<HashMap<String, Value>>>,
}

#[bon]
impl StoreSnapshotBinding {
    #[builder]
    pub(crate) fn new(#[builder(start_fn)] inner: Arc<PlMutex<HashMap<String, Value>>>) -> Self {
        Self { inner }
    }
}

impl LuaUserData for StoreSnapshotBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            let _ = debug_span!("store_snapshot_index", %key).entered();
            if let Some(value) = this.inner.lock().get(&key) {
                return vm.to_value(value);
            }
            Ok(LuaNil)
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let _ = debug_span!("store_snapshot_new_index", %key).entered();
                let value = vm.from_value::<Value>(value)?;
                this.inner.lock().insert(key, value);
                Ok(LuaNil)
            },
        );
    }
}

#[derive(Builder)]
pub(crate) struct StoreBinding {
    store: Option<Store>,
}

impl LuaUserData for StoreBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let _ = debug_span!("store_new_index", %key).entered();
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };
                let value = vm.from_value(value)?;
                store.put(key, &value).into_lua_err()?;
                Ok(LuaNil)
            },
        );

        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            let _ = debug_span!("store_index", %key).entered();
            let Some(store) = &this.store else {
                return Ok(LuaNil);
            };
            if let Some(value) = &store.get(key).into_lua_err()? {
                return vm.to_value(value);
            }
            Ok(LuaNil)
        });

        methods.add_method(
            "update",
            |vm, this, (keys_defaults, f): (LuaTable, LuaFunction)| {
                let span = debug_span!("store_update").entered();
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };

                fn parse_key_default(k: LuaValue, v: LuaValue) -> LuaResult<(String, LuaValue)> {
                    match (k.is_integer(), k.as_string(), v.as_string()) {
                        // key is string, value is default value e.g. { a = 1 }
                        (false, Some(k), _) => Ok((k.to_str().into_lua_err()?.to_string(), v)),
                        // key is integer (unused), value is key e.g. { "a" }
                        (true, _, Some(k)) => Ok((k.to_str().into_lua_err()?.to_string(), LuaNil)),
                        _ => {
                            let k_type = k.type_name();
                            Err(LuaError::external(format!(
                                "Key is either a number or a string, got {k_type}"
                            )))
                        }
                    }
                }

                let mut conn = store.lock();
                let tx = conn.transaction().into_lua_err()?;

                let mut snapshot = HashMap::new();
                {
                    let _ = debug_span!(parent: &span, "create_snapshot").entered();
                    let mut stmt = tx.prepare_cached(SQL_GET).into_lua_err()?;
                    for pair in keys_defaults.pairs::<LuaValue, LuaValue>() {
                        let (k, v) = pair?;
                        let (key, default) = parse_key_default(k, v)?;
                        let mut rows = stmt.query(params![&key]).into_lua_err()?;
                        if let Some(row) = rows.next().into_lua_err()? {
                            let val: Vec<u8> = row.get(0).into_lua_err()?;
                            let val: Value = rmp_serde::from_slice(&val).into_lua_err()?;
                            snapshot.insert(key, val);
                        } else {
                            snapshot.insert(key, vm.from_value(default)?);
                        }
                    }
                }

                let snapshot = Arc::new(PlMutex::new(snapshot));
                let snapshot_binding = StoreSnapshotBinding::builder(snapshot.clone()).build();

                let returned = {
                    let _ = debug_span!(parent: &span, "call").entered();
                    f.call::<LuaValue>(snapshot_binding)?
                };

                {
                    let _ = debug_span!(parent: &span, "write_snapshot").entered();
                    let snapshot = snapshot.lock();
                    for pair in keys_defaults.pairs::<LuaValue, LuaValue>() {
                        let (k, v) = pair?;
                        let (key, _) = parse_key_default(k, v)?;
                        if let Some(value) = snapshot.get(&key) {
                            let serialized = rmp_serde::to_vec(value).into_lua_err()?;
                            tx.prepare_cached(SQL_PUT)
                                .into_lua_err()?
                                .execute(params![&key, serialized])
                                .into_lua_err()?;
                        }
                    }
                }
                tx.commit().into_lua_err()?;

                Ok(returned)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use serde_json::json;
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_store_binding() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_update() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-update.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_update_with_missing_keys() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-update-missing-keys.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_update_preserves_unmodified() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-update-preserves.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_without_connection() {
        let source = include_str!("../fixtures/bindings/store/store-without-connection.lua");
        // Build runner without store connection
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap().result.unwrap();
        assert_eq!(json!(true), result);
    }

    #[tokio::test]
    async fn test_store_unicode_keys() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("../fixtures/bindings/store/store-unicode-keys.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
