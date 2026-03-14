/// Store backend trait definition.
pub mod backend;
/// `SQLite` store backend implementation.
pub mod sqlite;

pub use backend::StoreBackend;
pub use sqlite::SqliteBackend;

use bon::bon;
use serde_json::Value;

use crate::{LmbResult, LmbStore};

/// Represents a key-value store for Lua scripts.
#[derive(Debug)]
pub struct Store {
    inner: LmbStore,
}

impl Store {
    /// Returns a reference to the underlying store backend.
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
        self.inner.get(key.as_ref())
    }

    /// Puts a value into the store by key
    pub fn put<S: AsRef<str>>(&self, key: S, value: &Value) -> LmbResult<()> {
        self.inner.put(key.as_ref(), value)
    }

    /// Deletes a key from the store. Returns true if the key existed.
    pub fn del<S: AsRef<str>>(&self, key: S) -> LmbResult<bool> {
        self.inner.del(key.as_ref())
    }

    /// Returns true if the key exists in the store.
    pub fn has<S: AsRef<str>>(&self, key: S) -> LmbResult<bool> {
        self.inner.has(key.as_ref())
    }

    /// Returns all keys matching the given pattern, or all keys if no pattern is given.
    /// Pattern uses SQL LIKE syntax: `%` matches any sequence, `_` matches one character.
    pub fn keys(&self, pattern: Option<&str>) -> LmbResult<Vec<String>> {
        self.inner.keys(pattern)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{SqliteBackend, Store, StoreBackend};

    fn create_test_store() -> Store {
        let backend = SqliteBackend::new_in_memory().unwrap();
        backend.migrate().unwrap();
        Store::builder(std::sync::Arc::new(backend)).build()
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
