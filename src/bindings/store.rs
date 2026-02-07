use std::sync::Arc;

use bon::{Builder, bon};
use dashmap::DashMap;
use mlua::prelude::*;
use rusqlite::params;
use serde_json::Value;
use tracing::debug_span;

use crate::{
    LmbStore,
    stmt::{SQL_GET, SQL_PUT},
    store::Store,
};

pub(crate) struct StoreSnapshotBinding {
    inner: Arc<DashMap<String, Value>>,
}

#[bon]
impl StoreSnapshotBinding {
    #[builder]
    pub(crate) fn new(#[builder(start_fn)] inner: Arc<DashMap<String, Value>>) -> Self {
        Self { inner }
    }
}

impl LuaUserData for StoreSnapshotBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            let _ = debug_span!("store_snapshot_index", %key).entered();
            if let Some(tuple) = this.inner.get(&key) {
                return vm.to_value(tuple.value());
            }
            Ok(LuaNil)
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let _ = debug_span!("store_snapshot_new_index", %key).entered();
                let value = vm.from_value::<Value>(value)?;
                this.inner.insert(key, value);
                Ok(LuaNil)
            },
        );
    }
}

#[derive(Builder)]
pub(crate) struct StoreBinding {
    store: Option<LmbStore>,
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
                let store = Store::builder(store.clone()).build();
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
            let store = Store::builder(store.clone()).build();
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

                let snapshot = DashMap::new();
                {
                    let _ = debug_span!(parent: &span, "create_snapshot").entered();
                    let mut stmt = tx.prepare(SQL_GET).into_lua_err()?;
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

                let snapshot = Arc::new(snapshot);
                let snapshot_binding = StoreSnapshotBinding::builder(snapshot.clone()).build();

                let returned = {
                    let _ = debug_span!(parent: &span, "call").entered();
                    f.call::<LuaValue>(snapshot_binding)?
                };

                {
                    let _ = debug_span!(parent: &span, "write_snapshot").entered();
                    for pair in keys_defaults.pairs::<LuaValue, LuaValue>() {
                        let (k, v) = pair?;
                        let (key, _) = parse_key_default(k, v)?;
                        if let Some(pair) = snapshot.get(&key) {
                            let serialized = rmp_serde::to_vec(pair.value()).into_lua_err()?;
                            tx.execute(SQL_PUT, params![&key, serialized])
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
        let source = r#"
            function f(ctx)
                -- Update with keys that don't exist, using defaults
                local result = ctx.store:update({ x = 100, y = 200 }, function(values)
                    values.x = values.x + 1
                    values.y = values.y + 2
                    return values.x + values.y
                end)
                assert(result == 303, "Expected 303, got " .. tostring(result))
                assert(ctx.store.x == 101, "Expected x=101, got " .. tostring(ctx.store.x))
                assert(ctx.store.y == 202, "Expected y=202, got " .. tostring(ctx.store.y))
            end
            return f
        "#;
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_update_preserves_unmodified() {
        let conn = Connection::open_in_memory().unwrap();
        let source = r#"
            function f(ctx)
                -- Set initial values
                ctx.store.preserved = "original"
                ctx.store.modified = 0

                -- Only modify 'modified', leave 'preserved' alone in update
                ctx.store:update({ modified = 0 }, function(values)
                    values.modified = values.modified + 10
                end)

                -- 'preserved' should still be the original value
                assert(ctx.store.preserved == "original", "preserved should be untouched")
                assert(ctx.store.modified == 10, "modified should be 10")
            end
            return f
        "#;
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_without_connection() {
        let source = r#"
            function f(ctx)
                -- Store operations should return nil without a connection
                local val = ctx.store.some_key
                return val
            end
            return f
        "#;
        // Build runner without store connection
        let runner = Runner::builder(source, empty()).build().unwrap();
        let result = runner.invoke().call().await.unwrap().result.unwrap();
        assert_eq!(json!(null), result);
    }

    #[tokio::test]
    async fn test_store_unicode_keys() {
        let conn = Connection::open_in_memory().unwrap();
        let source = r#"
            function f(ctx)
                -- Test Unicode keys
                ctx.store["你好"] = "世界"
                ctx.store["🔑"] = { emoji = "🎉" }
                ctx.store["キー"] = 42

                assert(ctx.store["你好"] == "世界", "Chinese key/value failed")
                assert(ctx.store["🔑"].emoji == "🎉", "Emoji key failed")
                assert(ctx.store["キー"] == 42, "Japanese key failed")
            end
            return f
        "#;
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
