/// `PostgreSQL` store backend implementation.
#[cfg(feature = "postgres")]
pub mod postgres;
/// `SQLite` store backend implementation.
pub mod sqlite;

#[cfg(feature = "postgres")]
pub use self::postgres::PostgresBackend;
pub use sqlite::SqliteBackend;

use serde_json::Value;

use crate::LmbResult;

/// Trait for store backends that provide key-value storage.
pub trait StoreBackend: Send + Sync + std::fmt::Debug {
    /// Retrieves a value from the store by key.
    fn get(&self, key: &str) -> LmbResult<Option<Value>>;
    /// Puts a value into the store by key.
    fn put(&self, key: &str, value: &Value) -> LmbResult<()>;
    /// Deletes a key from the store. Returns true if the key existed.
    fn del(&self, key: &str) -> LmbResult<bool>;
    /// Returns true if the key exists in the store.
    fn has(&self, key: &str) -> LmbResult<bool>;
    /// Returns all keys matching the given pattern, or all keys if no pattern is given.
    /// Pattern uses SQL LIKE syntax: `%` matches any sequence, `_` matches one character.
    fn keys(&self, pattern: Option<&str>) -> LmbResult<Vec<String>>;
    /// Begins a transaction.
    fn begin_tx(&self) -> LmbResult<()>;
    /// Commits the current transaction.
    fn commit_tx(&self) -> LmbResult<()>;
    /// Rolls back the current transaction.
    fn rollback_tx(&self) -> LmbResult<()>;
    /// Runs any pending migrations.
    fn migrate(&self) -> LmbResult<()>;
    /// Creates an independent backend instance suitable for concurrent use.
    ///
    /// For connection-per-thread backends (e.g. PostgreSQL), this opens a new
    /// connection. For backends with built-in concurrency control (e.g. SQLite),
    /// this may share the underlying connection.
    fn fork(&self) -> LmbResult<std::sync::Arc<dyn StoreBackend>>;
}
