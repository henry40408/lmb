use bon::bon;
use rusqlite::params;
use serde_json::Value;

use crate::{LmbResult, LmbStore};

static SQL_PUT: &str = "INSERT OR REPLACE INTO store (key, value) VALUES (?, ?)";
static SQL_GET: &str = "SELECT value FROM store WHERE key = ?";

/// Represents a key-value store for Lua scripts.
#[derive(Debug)]
pub struct Store {
    inner: LmbStore,
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
