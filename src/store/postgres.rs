use std::fmt;

use parking_lot::Mutex;
use postgres::{Client, NoTls};
use serde_json::Value;
use tracing::debug_span;

use crate::{LmbError, LmbResult};

use super::StoreBackend;

static MIGRATIONS: &[&str] = &[include_str!("../migrations/0001-initial.pg.sql")];

static SQL_GET: &str = "SELECT value FROM store WHERE key = $1";
static SQL_PUT: &str = "INSERT INTO store (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()";
static SQL_DEL: &str = "DELETE FROM store WHERE key = $1";
static SQL_HAS: &str = "SELECT 1 FROM store WHERE key = $1";
static SQL_KEYS: &str = "SELECT key FROM store WHERE key LIKE $1";
static SQL_KEYS_ALL: &str = "SELECT key FROM store";

/// PostgreSQL-backed implementation of [`StoreBackend`].
pub struct PostgresBackend {
    client: Mutex<Client>,
    url: String,
}

impl fmt::Debug for PostgresBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresBackend").finish_non_exhaustive()
    }
}

impl PostgresBackend {
    /// Creates a new `PostgresBackend` by connecting to the given URL.
    pub fn connect(url: &str) -> LmbResult<Self> {
        let client = Client::connect(url, NoTls).map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(Self {
            client: Mutex::new(client),
            url: url.to_string(),
        })
    }
}

impl StoreBackend for PostgresBackend {
    fn get(&self, key: &str) -> LmbResult<Option<Value>> {
        let mut client = self.client.lock();
        let rows = client
            .query(SQL_GET, &[&key])
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        if let Some(row) = rows.first() {
            let value: Vec<u8> = row.get(0);
            let value: Value = rmp_serde::from_slice(&value)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn put(&self, key: &str, value: &Value) -> LmbResult<()> {
        let serialized = rmp_serde::to_vec(value)?;
        let mut client = self.client.lock();
        client
            .execute(SQL_PUT, &[&key, &serialized])
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(())
    }

    fn del(&self, key: &str) -> LmbResult<bool> {
        let mut client = self.client.lock();
        let affected = client
            .execute(SQL_DEL, &[&key])
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(affected > 0)
    }

    fn has(&self, key: &str) -> LmbResult<bool> {
        let mut client = self.client.lock();
        let rows = client
            .query(SQL_HAS, &[&key])
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(!rows.is_empty())
    }

    fn keys(&self, pattern: Option<&str>) -> LmbResult<Vec<String>> {
        let mut client = self.client.lock();
        let rows = if let Some(pat) = pattern {
            client
                .query(SQL_KEYS, &[&pat])
                .map_err(|e| LmbError::Store(Box::new(e)))?
        } else {
            client
                .query(SQL_KEYS_ALL, &[])
                .map_err(|e| LmbError::Store(Box::new(e)))?
        };
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    fn begin_tx(&self) -> LmbResult<()> {
        let mut client = self.client.lock();
        client
            .batch_execute("BEGIN")
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(())
    }

    fn commit_tx(&self) -> LmbResult<()> {
        let mut client = self.client.lock();
        client
            .batch_execute("COMMIT")
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(())
    }

    fn rollback_tx(&self) -> LmbResult<()> {
        let mut client = self.client.lock();
        client
            .batch_execute("ROLLBACK")
            .map_err(|e| LmbError::Store(Box::new(e)))?;
        Ok(())
    }

    fn migrate(&self) -> LmbResult<()> {
        let mut client = self.client.lock();

        // Use advisory lock to prevent concurrent migration runs
        client
            .batch_execute("SELECT pg_advisory_lock(hashtext('lmb_migration'))")
            .map_err(|e| LmbError::Store(Box::new(e)))?;

        // Create migrations tracking table
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS lmb_migrations (version INTEGER PRIMARY KEY)",
            )
            .map_err(|e| LmbError::Store(Box::new(e)))?;

        let current_version: i32 = client
            .query_one("SELECT COALESCE(MAX(version), 0) FROM lmb_migrations", &[])
            .map_err(|e| LmbError::Store(Box::new(e)))?
            .get(0);
        let migrations_to_run = MIGRATIONS.len() as i32;

        if current_version < migrations_to_run {
            let span =
                debug_span!("run_migrations", current_version, total = migrations_to_run).entered();

            // Wrap all migrations in a transaction so a partial failure rolls back cleanly
            client
                .batch_execute("BEGIN")
                .map_err(|e| LmbError::Store(Box::new(e)))?;

            let result = (|| -> LmbResult<()> {
                for (idx, migration) in MIGRATIONS.iter().enumerate() {
                    let version = idx as i32 + 1;
                    if version > current_version {
                        let _ = debug_span!(parent: &span, "run_migration", version, migration)
                            .entered();
                        client
                            .batch_execute(migration)
                            .map_err(|e| LmbError::Store(Box::new(e)))?;
                        client
                            .execute(
                                "INSERT INTO lmb_migrations (version) VALUES ($1) ON CONFLICT DO NOTHING",
                                &[&version],
                            )
                            .map_err(|e| LmbError::Store(Box::new(e)))?;
                    }
                }
                Ok(())
            })();

            if result.is_err() {
                let _ = client.batch_execute("ROLLBACK");
                // Release advisory lock before returning error
                let _ = client.batch_execute("SELECT pg_advisory_unlock(hashtext('lmb_migration'))");
                return result;
            }

            client
                .batch_execute("COMMIT")
                .map_err(|e| LmbError::Store(Box::new(e)))?;
        }

        // Release advisory lock
        client
            .batch_execute("SELECT pg_advisory_unlock(hashtext('lmb_migration'))")
            .map_err(|e| LmbError::Store(Box::new(e)))?;

        Ok(())
    }

    fn fork(&self) -> LmbResult<std::sync::Arc<dyn StoreBackend>> {
        Ok(std::sync::Arc::new(Self::connect(&self.url)?))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use serde_json::json;

    use super::*;

    // Serialize all PG tests since they share a single database
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn create_test_backend() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://lmb:lmb@localhost:5432/lmb".to_string());
        let backend = PostgresBackend::connect(&url).expect("Failed to connect to PostgreSQL");
        backend.migrate().expect("Failed to run migrations");
        backend
            .client
            .lock()
            .batch_execute("DELETE FROM store")
            .expect("Failed to clean up store table");
        drop(backend);
        guard
    }

    fn connect() -> PostgresBackend {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://lmb:lmb@localhost:5432/lmb".to_string());
        PostgresBackend::connect(&url).expect("Failed to connect to PostgreSQL")
    }

    #[test]
    fn test_get_nonexistent_key() {
        let _guard = create_test_backend();
        let backend = connect();
        let result = backend.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_put_and_get() {
        let _guard = create_test_backend();
        let backend = connect();
        let value = json!({"hello": "world"});
        backend.put("key1", &value).unwrap();
        let result = backend.get("key1").unwrap();
        assert_eq!(Some(value), result);
    }

    #[test]
    fn test_overwrite_value() {
        let _guard = create_test_backend();
        let backend = connect();
        let value1 = json!(1);
        let value2 = json!(2);
        backend.put("counter", &value1).unwrap();
        assert_eq!(Some(value1), backend.get("counter").unwrap());
        backend.put("counter", &value2).unwrap();
        assert_eq!(Some(value2), backend.get("counter").unwrap());
    }

    #[test]
    fn test_del() {
        let _guard = create_test_backend();
        let backend = connect();
        backend.put("key1", &json!("val")).unwrap();
        assert!(backend.del("key1").unwrap());
        assert!(!backend.del("key1").unwrap());
        assert!(backend.get("key1").unwrap().is_none());
    }

    #[test]
    fn test_has() {
        let _guard = create_test_backend();
        let backend = connect();
        assert!(!backend.has("key1").unwrap());
        backend.put("key1", &json!("val")).unwrap();
        assert!(backend.has("key1").unwrap());
    }

    #[test]
    fn test_keys() {
        let _guard = create_test_backend();
        let backend = connect();
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
        let _guard = create_test_backend();
        let backend = connect();
        backend.begin_tx().unwrap();
        backend.put("tx_key", &json!("tx_val")).unwrap();
        backend.commit_tx().unwrap();
        assert_eq!(Some(json!("tx_val")), backend.get("tx_key").unwrap());
    }

    #[test]
    fn test_transaction_rollback() {
        let _guard = create_test_backend();
        let backend = connect();
        backend.put("existing", &json!("before")).unwrap();
        backend.begin_tx().unwrap();
        backend.put("existing", &json!("during")).unwrap();
        backend.rollback_tx().unwrap();
        assert_eq!(Some(json!("before")), backend.get("existing").unwrap());
    }

    #[test]
    fn test_complex_json_value() {
        let _guard = create_test_backend();
        let backend = connect();
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
        let _guard = create_test_backend();
        let backend = connect();
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
        let _guard = create_test_backend();
        let backend = connect();
        backend.migrate().unwrap();
        backend.put("key", &json!("val")).unwrap();
        assert_eq!(Some(json!("val")), backend.get("key").unwrap());
    }
}
