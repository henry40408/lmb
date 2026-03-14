use std::{cell::RefCell, sync::Arc};

use parking_lot::ReentrantMutex;
use rusqlite::{Connection, params};
use serde_json::Value;
use tracing::debug_span;

use crate::LmbResult;

use super::StoreBackend;

static MIGRATIONS: &[&str] = &[include_str!("../migrations/0001-initial.sql")];

static SQL_GET: &str = "SELECT value FROM store WHERE key = ?";
static SQL_PUT: &str = "INSERT OR REPLACE INTO store (key, value) VALUES (?, ?)";
static SQL_DEL: &str = "DELETE FROM store WHERE key = ?";
static SQL_HAS: &str = "SELECT 1 FROM store WHERE key = ?";
static SQL_KEYS: &str = "SELECT key FROM store WHERE key LIKE ?";
static SQL_KEYS_ALL: &str = "SELECT key FROM store";

/// SQLite-backed implementation of [`StoreBackend`].
#[derive(Debug)]
pub struct SqliteBackend {
    conn: Arc<ReentrantMutex<RefCell<Connection>>>,
}

impl SqliteBackend {
    /// Creates a new `SqliteBackend` wrapping the given connection.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(ReentrantMutex::new(RefCell::new(conn))),
        }
    }

    /// Creates a new `SqliteBackend` with an in-memory database.
    pub fn new_in_memory() -> LmbResult<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self::new(conn))
    }

    fn set_pragmas(&self) -> LmbResult<()> {
        let _ = debug_span!("set_pragmas").entered();
        let guard = self.conn.lock();
        let conn = guard.borrow();
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }
}

impl StoreBackend for SqliteBackend {
    fn get(&self, key: &str) -> LmbResult<Option<Value>> {
        let guard = self.conn.lock();
        let conn = guard.borrow();
        let mut stmt = conn.prepare_cached(SQL_GET)?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let value: Vec<u8> = row.get(0)?;
            let value: Value = rmp_serde::from_slice(&value)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn put(&self, key: &str, value: &Value) -> LmbResult<()> {
        let guard = self.conn.lock();
        let conn = guard.borrow();
        let serialized = rmp_serde::to_vec(value)?;
        conn.prepare_cached(SQL_PUT)?
            .execute(params![key, serialized])?;
        Ok(())
    }

    fn del(&self, key: &str) -> LmbResult<bool> {
        let guard = self.conn.lock();
        let conn = guard.borrow();
        let affected = conn.prepare_cached(SQL_DEL)?.execute(params![key])?;
        Ok(affected > 0)
    }

    fn has(&self, key: &str) -> LmbResult<bool> {
        let guard = self.conn.lock();
        let conn = guard.borrow();
        let mut stmt = conn.prepare_cached(SQL_HAS)?;
        let mut rows = stmt.query(params![key])?;
        Ok(rows.next()?.is_some())
    }

    fn keys(&self, pattern: Option<&str>) -> LmbResult<Vec<String>> {
        let guard = self.conn.lock();
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

    fn begin_tx(&self) -> LmbResult<()> {
        let guard = self.conn.lock();
        guard.borrow().execute_batch("BEGIN IMMEDIATE")?;
        Ok(())
    }

    fn commit_tx(&self) -> LmbResult<()> {
        let guard = self.conn.lock();
        guard.borrow().execute_batch("COMMIT")?;
        Ok(())
    }

    fn rollback_tx(&self) -> LmbResult<()> {
        let guard = self.conn.lock();
        guard.borrow().execute_batch("ROLLBACK")?;
        Ok(())
    }

    fn migrate(&self) -> LmbResult<()> {
        self.set_pragmas()?;
        let guard = self.conn.lock();
        let conn = guard.borrow();
        let current_version: i32 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let migrations_to_run = MIGRATIONS.len() as i32;

        if current_version < migrations_to_run {
            let span =
                debug_span!("run_migrations", current_version, total = migrations_to_run).entered();
            for (idx, migration) in MIGRATIONS.iter().enumerate() {
                let version = idx as i32 + 1;
                if version > current_version {
                    let _ =
                        debug_span!(parent: &span, "run_migration", version, migration).entered();
                    conn.execute_batch(migration)?;
                }
            }
            conn.pragma_update(None, "user_version", migrations_to_run)?;
        }
        Ok(())
    }

    fn fork(&self) -> LmbResult<std::sync::Arc<dyn StoreBackend>> {
        Ok(std::sync::Arc::new(Self {
            conn: self.conn.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn create_test_backend() -> SqliteBackend {
        let backend = SqliteBackend::new_in_memory().unwrap();
        backend.migrate().unwrap();
        backend
    }

    #[test]
    fn test_get_nonexistent_key() {
        let backend = create_test_backend();
        let result = backend.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get() {
        let backend = create_test_backend();
        let value = json!({"hello": "world"});
        backend.put("key1", &value).unwrap();
        let result = backend.get("key1").unwrap();
        assert_eq!(Some(value), result);
    }

    #[test]
    fn test_overwrite_value() {
        let backend = create_test_backend();
        let value1 = json!(1);
        let value2 = json!(2);
        backend.put("counter", &value1).unwrap();
        assert_eq!(Some(value1), backend.get("counter").unwrap());
        backend.put("counter", &value2).unwrap();
        assert_eq!(Some(value2), backend.get("counter").unwrap());
    }

    #[test]
    fn test_del() {
        let backend = create_test_backend();
        backend.put("key1", &json!("val")).unwrap();
        assert!(backend.del("key1").unwrap());
        assert!(!backend.del("key1").unwrap());
        assert!(backend.get("key1").unwrap().is_none());
    }

    #[test]
    fn test_has() {
        let backend = create_test_backend();
        assert!(!backend.has("key1").unwrap());
        backend.put("key1", &json!("val")).unwrap();
        assert!(backend.has("key1").unwrap());
    }

    #[test]
    fn test_keys() {
        let backend = create_test_backend();
        backend.put("user:1", &json!("a")).unwrap();
        backend.put("user:2", &json!("b")).unwrap();
        backend.put("item:1", &json!("c")).unwrap();

        let mut all = backend.keys(None).unwrap();
        all.sort();
        assert_eq!(vec!["item:1", "user:1", "user:2"], all);

        let mut users = backend.keys(Some("user:%")).unwrap();
        users.sort();
        assert_eq!(vec!["user:1", "user:2"], users);
    }

    #[test]
    fn test_transaction_commit() {
        let backend = create_test_backend();
        backend.begin_tx().unwrap();
        backend.put("tx_key", &json!("tx_val")).unwrap();
        backend.commit_tx().unwrap();
        assert_eq!(Some(json!("tx_val")), backend.get("tx_key").unwrap());
    }

    #[test]
    fn test_transaction_rollback() {
        let backend = create_test_backend();
        backend.put("existing", &json!("before")).unwrap();
        backend.begin_tx().unwrap();
        backend.put("existing", &json!("during")).unwrap();
        backend.rollback_tx().unwrap();
        assert_eq!(Some(json!("before")), backend.get("existing").unwrap());
    }

    #[test]
    fn test_complex_json_value() {
        let backend = create_test_backend();
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
        backend.put("complex", &value).unwrap();
        assert_eq!(Some(value), backend.get("complex").unwrap());
    }

    #[test]
    fn test_unicode_key_and_value() {
        let backend = create_test_backend();
        let key = "你好世界";
        let value = json!({
            "greeting": "こんにちは",
            "emoji": "🎉🚀",
            "arabic": "مرحبا"
        });
        backend.put(key, &value).unwrap();
        assert_eq!(Some(value), backend.get(key).unwrap());
    }

    #[test]
    fn test_migrate_idempotent() {
        let backend = create_test_backend();
        // Running migrate again should be a no-op
        backend.migrate().unwrap();
        backend.put("key", &json!("val")).unwrap();
        assert_eq!(Some(json!("val")), backend.get("key").unwrap());
    }
}
