use std::sync::{Arc, LazyLock};

use bon::bon;
use dashmap::DashMap;
use mlua::prelude::*;
use rusqlite::params;
use serde_json::Value;

use crate::{LmbResult, LmbStore};

static MIGRATIONS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| vec![include_str!("migrations/0001-initial.sql")]);

static SQL_PUT: &str = "INSERT OR REPLACE INTO store (key, value) VALUES (?, ?)";
static SQL_GET: &str = "SELECT value FROM store WHERE key = ?";

pub(crate) struct StoreSnapshotBinding {
    inner: Arc<DashMap<Box<str>, Value>>,
}

#[bon]
impl StoreSnapshotBinding {
    #[builder]
    pub(crate) fn new(#[builder(start_fn)] inner: Arc<DashMap<Box<str>, Value>>) -> Self {
        Self { inner }
    }
}

impl LuaUserData for StoreSnapshotBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            if let Some(tuple) = this.inner.get(&key.into_boxed_str()) {
                return vm.to_value(tuple.value()).into_lua_err();
            }
            Ok(LuaNil)
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let value = vm.from_value::<Value>(value).into_lua_err()?;
                this.inner.insert(key.into_boxed_str(), value);
                Ok(LuaNil)
            },
        );
    }
}

pub(crate) struct StoreBinding {
    store: Option<LmbStore>,
}

#[bon]
impl StoreBinding {
    #[builder]
    pub(crate) fn new(#[builder(start_fn)] store: Option<LmbStore>) -> LmbResult<Self> {
        if let Some(store) = &store {
            let conn = store.lock();
            for migration in MIGRATIONS.iter() {
                conn.execute_batch(migration).into_lua_err()?;
            }
        }
        Ok(Self { store })
    }
}

impl LuaUserData for StoreBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |vm, this, (key, value): (String, LuaValue)| {
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };
                let conn = store.lock();
                let value = vm.from_value::<Value>(value).into_lua_err()?;
                let serialized = rmp_serde::to_vec(&value).into_lua_err()?;
                conn.execute(SQL_PUT, params![key, serialized])
                    .into_lua_err()?;
                Ok(LuaNil)
            },
        );
        methods.add_meta_method(LuaMetaMethod::Index, |vm, this, key: String| {
            let Some(store) = &this.store else {
                return Ok(LuaNil);
            };
            let conn = store.lock();
            let mut stmt = conn.prepare(SQL_GET).into_lua_err()?;
            let mut rows = stmt.query(params![key]).into_lua_err()?;
            if let Some(row) = rows.next().into_lua_err()? {
                let value: Vec<u8> = row.get(0).into_lua_err()?;
                let value: Value = rmp_serde::from_slice(&value).into_lua_err()?;
                return vm.to_value(&value).into_lua_err();
            }
            Ok(LuaNil)
        });
        methods.add_method(
            "update",
            |vm, this, (keys_defaults, f): (LuaTable, LuaFunction)| {
                let Some(store) = &this.store else {
                    return Ok(LuaNil);
                };

                fn parse_key_default(k: LuaValue, v: LuaValue) -> LuaResult<(Box<str>, LuaValue)> {
                    match (k.is_integer(), k.as_string(), v.as_string()) {
                        // key is string, value is default value e.g. { a = 1 }
                        (false, Some(k), _) => {
                            Ok((k.to_str().into_lua_err()?.to_string().into_boxed_str(), v))
                        }
                        // key is integer (unused), value is key e.g. { "a" }
                        (true, _, Some(k)) => Ok((
                            k.to_str().into_lua_err()?.to_string().into_boxed_str(),
                            LuaNil,
                        )),
                        _ => Err(LuaError::external("Key is either a number or a string")),
                    }
                }

                let mut conn = store.lock();
                let tx = conn.transaction().into_lua_err()?;

                let snapshot = DashMap::new();
                {
                    let mut stmt = tx.prepare(SQL_GET).into_lua_err()?;
                    for pair in keys_defaults.pairs::<LuaValue, LuaValue>() {
                        let (k, v) = pair.into_lua_err()?;
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
                f.call::<LuaValue>(snapshot_binding).into_lua_err()?;

                for pair in keys_defaults.pairs::<LuaValue, LuaValue>() {
                    let (k, v) = pair?;
                    let (key, _) = parse_key_default(k, v)?;
                    if let Some(pair) = snapshot.get(&key) {
                        let serialized = rmp_serde::to_vec(pair.value()).into_lua_err()?;
                        tx.execute(SQL_PUT, params![&key, serialized])
                            .into_lua_err()?;
                    }
                }
                tx.commit().into_lua_err()?;

                Ok(LuaNil)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_store_binding() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("fixtures/store.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }

    #[tokio::test]
    async fn test_store_update() {
        let conn = Connection::open_in_memory().unwrap();
        let source = include_str!("fixtures/store-update.lua");
        let runner = Runner::builder(source, empty())
            .store(conn)
            .build()
            .unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
