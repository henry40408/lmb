use bon::bon;
use parking_lot::MutexGuard;
use rusqlite::{Connection, params};
use serde_json::Value;

use crate::{
    LmbResult, LmbStore,
    stmt::{SQL_GET, SQL_PUT},
};

/// Represents a key-value store for Lua scripts.
#[derive(Debug)]
pub struct Store {
    inner: LmbStore,
}

impl Store {
    /// Acquires a lock on the underlying database connection.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.inner.lock()
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
        let conn = self.inner.lock();
        let mut stmt = conn.prepare(SQL_GET)?;
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
    pub fn put<'a, S: AsRef<str>>(&'a self, key: S, value: &'a Value) -> LmbResult<()> {
        let conn = self.inner.lock();
        let key = key.as_ref();
        let serialized = rmp_serde::to_vec(&value)?;
        conn.execute(SQL_PUT, params![key, serialized])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use rusqlite::Connection;
    use serde_json::json;

    use super::Store;
    use crate::stmt::MIGRATIONS;

    fn create_test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        for migration in MIGRATIONS.iter() {
            conn.execute_batch(migration).unwrap();
        }
        Store::builder(Arc::new(Mutex::new(conn))).build()
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
}
