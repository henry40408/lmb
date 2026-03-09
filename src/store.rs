use bon::bon;
use rusqlite::params;
use serde_json::Value;

use crate::{
    LmbResult, LmbStore,
    stmt::{SQL_DEL, SQL_GET, SQL_HAS, SQL_KEYS, SQL_KEYS_ALL, SQL_PUT},
};

/// Represents a key-value store for Lua scripts.
#[derive(Debug)]
pub struct Store {
    inner: LmbStore,
}

impl Store {
    /// Returns a reference to the underlying store connection.
    pub fn inner(&self) -> &LmbStore {
        &self.inner
    }
}

#[bon]
impl Store {
    /// Creates a new key-value store.
    #[builder]
    pub fn new(#[builder(start_fn)] inner: LmbStore) -> Self {
        Self { inner }
    }

    /// Retrieves a value from the store by key
    pub fn get<S: AsRef<str>>(&self, key: S) -> LmbResult<Option<Value>> {
        let guard = self.inner.lock();
        let conn = guard.borrow();
        let mut stmt = conn.prepare_cached(SQL_GET)?;
        let mut rows = stmt.query(params![key.as_ref()])?;
        if let Some(row) = rows.next()? {
            let value: Vec<u8> = row.get(0)?;
            let value: Value = rmp_serde::from_slice(&value)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Puts a value into the store by key
    pub fn put<S: AsRef<str>>(&self, key: S, value: &Value) -> LmbResult<()> {
        let guard = self.inner.lock();
        let conn = guard.borrow();
        let serialized = rmp_serde::to_vec(value)?;
        conn.prepare_cached(SQL_PUT)?
            .execute(params![key.as_ref(), serialized])?;
        Ok(())
    }

    /// Deletes a key from the store. Returns true if the key existed.
    pub fn del<S: AsRef<str>>(&self, key: S) -> LmbResult<bool> {
        let guard = self.inner.lock();
        let conn = guard.borrow();
        let affected = conn
            .prepare_cached(SQL_DEL)?
            .execute(params![key.as_ref()])?;
        Ok(affected > 0)
    }

    /// Returns true if the key exists in the store.
    pub fn has<S: AsRef<str>>(&self, key: S) -> LmbResult<bool> {
        let guard = self.inner.lock();
        let conn = guard.borrow();
        let mut stmt = conn.prepare_cached(SQL_HAS)?;
        let mut rows = stmt.query(params![key.as_ref()])?;
        Ok(rows.next()?.is_some())
    }

    /// Returns all keys matching the given pattern, or all keys if no pattern is given.
    /// Pattern uses SQL LIKE syntax: `%` matches any sequence, `_` matches one character.
    pub fn keys(&self, pattern: Option<&str>) -> LmbResult<Vec<String>> {
        let guard = self.inner.lock();
        let conn = guard.borrow();
        let mut keys = Vec::new();
        if let Some(pat) = pattern {
            let mut stmt = conn.prepare_cached(SQL_KEYS)?;
            let mut rows = stmt.query(params![pat])?;
            while let Some(row) = rows.next()? {
                keys.push(row.get(0)?);
            }
        } else {
            let mut stmt = conn.prepare_cached(SQL_KEYS_ALL)?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                keys.push(row.get(0)?);
            }
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, sync::Arc};

    use parking_lot::ReentrantMutex;
    use rusqlite::Connection;
    use serde_json::json;

    use super::Store;
    use crate::stmt::MIGRATIONS;

    fn create_test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        for migration in MIGRATIONS.iter() {
            conn.execute_batch(migration).unwrap();
        }
        Store::builder(Arc::new(ReentrantMutex::new(RefCell::new(conn)))).build()
    }

    #[test]
    fn test_get_nonexistent_key() {
        let store = create_test_store();
        let result = store.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get() {
        let store = create_test_store();
        let value = json!({"hello": "world"});
        store.put("key1", &value).unwrap();
        let result = store.get("key1").unwrap();
        assert_eq!(Some(value), result);
    }

    #[test]
    fn test_overwrite_value() {
        let store = create_test_store();
        let value1 = json!(1);
        let value2 = json!(2);
        store.put("counter", &value1).unwrap();
        assert_eq!(Some(value1), store.get("counter").unwrap());
        store.put("counter", &value2).unwrap();
        assert_eq!(Some(value2), store.get("counter").unwrap());
    }

    #[test]
    fn test_complex_json_value() {
        let store = create_test_store();
        let value = json!({
            "string": "hello",
            "number": 42,
            "float": 1.23,
            "boolean": true,
            "null": null,
            "array": [1, 2, 3],
            "nested": {
                "a": {"b": {"c": "deep"}}
            }
        });
        store.put("complex", &value).unwrap();
        assert_eq!(Some(value), store.get("complex").unwrap());
    }

    #[test]
    fn test_null_value() {
        let store = create_test_store();
        let value = json!(null);
        store.put("nullable", &value).unwrap();
        assert_eq!(Some(value), store.get("nullable").unwrap());
    }

    #[test]
    fn test_unicode_key_and_value() {
        let store = create_test_store();
        let key = "你好世界";
        let value = json!({
            "greeting": "こんにちは",
            "emoji": "🎉🚀",
            "arabic": "مرحبا"
        });
        store.put(key, &value).unwrap();
        assert_eq!(Some(value), store.get(key).unwrap());
    }

    #[test]
    fn test_del() {
        let store = create_test_store();
        store.put("key1", &json!("val")).unwrap();
        assert!(store.del("key1").unwrap());
        assert!(!store.del("key1").unwrap());
        assert!(store.get("key1").unwrap().is_none());
    }

    #[test]
    fn test_has() {
        let store = create_test_store();
        assert!(!store.has("key1").unwrap());
        store.put("key1", &json!("val")).unwrap();
        assert!(store.has("key1").unwrap());
    }

    #[test]
    fn test_keys() {
        let store = create_test_store();
        store.put("user:1", &json!("a")).unwrap();
        store.put("user:2", &json!("b")).unwrap();
        store.put("item:1", &json!("c")).unwrap();

        let mut all = store.keys(None).unwrap();
        all.sort();
        assert_eq!(vec!["item:1", "user:1", "user:2"], all);

        let mut users = store.keys(Some("user:%")).unwrap();
        users.sort();
        assert_eq!(vec!["user:1", "user:2"], users);
    }
}
