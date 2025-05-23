use bon::bon;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use rusqlite::Connection;
use rusqlite_migration::SchemaVersion;
use serde_json::Value;
use std::{collections::HashMap, mem::size_of, path::Path, sync::Arc, time::Duration};
use stmt::*;
use tracing::{debug, trace, trace_span};

use crate::{MIGRATIONS, Result};

mod stmt;

static DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Store that persists data across executions.
#[derive(Clone, Debug)]
pub struct Store {
    busy_timeout: Duration,
    conn: Arc<Mutex<Connection>>,
}

#[bon]
impl Store {
    /// Create a new store with path on the filesystem.
    ///
    /// ```rust
    /// # use assert_fs::NamedTempFile;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store_file = NamedTempFile::new("db.sqlite3")?;
    /// Store::builder().path(store_file.path()).build()?;
    /// # Ok(())
    /// # }
    /// ```
    #[builder]
    pub fn new(
        path: &Path,
        busy_timeout: Option<Duration>,
        run_migrations: Option<bool>,
    ) -> Result<Self> {
        debug!(?path, "open store");
        let conn = Connection::open(path)?;

        let busy_timeout = busy_timeout.unwrap_or(DEFAULT_BUSY_TIMEOUT);
        let busy_timeout_ms = u64::try_from(busy_timeout.as_millis())?;
        conn.pragma_update(None, "busy_timeout", busy_timeout_ms)?;
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        // give the mutex a 20% buffer relative to the SQLite busy timeout
        let busy_timeout = busy_timeout.mul_f64(0.8);
        let store = Self {
            busy_timeout,
            conn: Arc::new(Mutex::new(conn)),
        };
        if let Some(true) = run_migrations {
            store.migrate(None)?;
        }
        Ok(store)
    }

    /// Perform migration on the database. Migrations should be idempotent. If version is omitted,
    /// database will be migrated to the latest. If version is 0, all migrations will be reverted.
    ///
    /// ```rust
    /// # use assert_fs::NamedTempFile;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store_file = NamedTempFile::new("db.sqlite3")?;
    /// let store = Store::builder().path(store_file.path()).build()?;
    /// store.migrate(None)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn migrate(&self, version: Option<usize>) -> Result<()> {
        let Some(mut conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };
        if let Some(version) = version {
            let _s = trace_span!("migrate_to_version", version).entered();
            MIGRATIONS.to_version(&mut conn, version)?;
        } else {
            let _s = trace_span!("migrate_to_latest").entered();
            MIGRATIONS.to_latest(&mut conn)?;
        }
        Ok(())
    }

    /// Return current version of migrations.
    pub fn current_version(&self) -> Result<SchemaVersion> {
        let Some(conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };
        let version = MIGRATIONS.current_version(&conn)?;
        Ok(version)
    }

    /// Delete value by name.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// assert_eq!(json!(null), store.get("a")?);
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// store.delete("a");
    /// assert_eq!(json!(null), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete<S: AsRef<str>>(&self, name: S) -> Result<usize> {
        let Some(conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };
        let (sql, values) = stmt_delete_value_by_name(name);
        let affected = conn.execute(&sql, &*values.as_params())?;
        Ok(affected)
    }

    /// Get value from the store. A `nil` will be returned to Lua virtual machine
    /// when the value is absent.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// assert_eq!(json!(null), store.get("a")?);
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn get<S: AsRef<str>>(&self, name: S) -> Result<Value> {
        let Some(conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };

        let name = name.as_ref();

        let (sql, values) = stmt_get_value_by_name(name);
        let mut cached_stmt = conn.prepare_cached(&sql)?;
        let row = {
            let _s = trace_span!("store_get", name).entered();
            cached_stmt.query_row(&*values.as_params(), |row| {
                let type_hint: Box<str> = row.get_unwrap("type_hint");
                let value: Vec<u8> = row.get_unwrap("value");
                Ok((type_hint, value))
            })
        };
        match row {
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                trace!("value is absent");
                Ok(Value::Null)
            }
            Err(e) => Err(e.into()),
            Ok((type_hint, value)) => {
                trace!(type_hint, "value is present");
                Ok(rmp_serde::from_slice(&value)?)
            }
        }
    }

    /// List values.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &true.into())?;
    /// let values = store.list()?;
    /// assert_eq!(1, values.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn list(&self) -> Result<Vec<StoreValueMetadata>> {
        let Some(conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };

        let (sql, values) = stmt_get_all_values();
        let mut cached_stmt = conn.prepare_cached(&sql)?;
        let mut rows = cached_stmt.query(&*values.as_params())?;

        let mut res = vec![];
        while let Ok(Some(row)) = rows.next() {
            res.push(StoreValueMetadata {
                name: row.get_unwrap("name"),
                size: row.get_unwrap("size"),
                type_hint: row.get_unwrap("type_hint"),
                created_at: row.get_unwrap("created_at"),
                updated_at: row.get_unwrap("updated_at"),
            });
        }
        Ok(res)
    }

    /// Put (insert or update) the value into the store.
    ///
    /// The key distinction between this function and [`Store::update`] is
    /// that this function unconditionally puts with the provided value.
    ///
    /// ```rust
    /// # use serde_json::json;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &true.into());
    /// assert_eq!(json!(true), store.get("a")?);
    /// store.put("b", &1.into());
    /// assert_eq!(json!(1), store.get("b")?);
    /// store.put("c", &"hello".into());
    /// assert_eq!(json!("hello"), store.get("c")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn put<S: AsRef<str>>(&self, name: S, value: &Value) -> Result<usize> {
        let Some(conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };

        let name = name.as_ref();
        let size = Self::get_size(value);
        let type_hint = Self::type_hint(value);
        let value = rmp_serde::to_vec(&value)?;

        let (sql, values) = stmt_upsert_store(name, value, size, type_hint)?;
        let mut cached_stmt = conn.prepare_cached(&sql)?;
        let affected = {
            let _s = trace_span!("store_insert", name, type_hint).entered();
            cached_stmt.execute(&*values.as_params())?
        };

        Ok(affected)
    }

    /// Insert or update the value into the store.
    ///
    /// Unlike [`Store::put`], this function accepts a closure and only mutates the value in the store
    /// when the closure returns a new value. If the closure results in an error,
    /// the value in the store remains unchanged.
    ///
    /// This function also takes a default value.
    ///
    /// # Successfully update the value
    ///
    /// ```rust
    /// # use maplit::hashmap;
    /// # use serde_json::{json, Value};
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// let updated = store.update(&["b"], |old| {
    ///     old.entry("b".into()).and_modify(|v| {
    ///         if let Value::Number(n) = v {
    ///             let n = n.as_i64().expect("n is required");
    ///             *v = json!(n + 1);
    ///         }
    ///     });
    ///     Ok(())
    /// }, Some(hashmap!{ "b".into() => 1.into() }));
    /// assert_eq!(hashmap!{ "b".into() => 2.into() }, updated?);
    /// assert_eq!(json!(2), store.get("b")?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Do nothing when an error is returned
    ///
    /// ```rust
    /// # use maplit::hashmap;
    /// # use mlua::prelude::*;
    /// # use serde_json::{json, Value};
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let store = Store::default();
    /// store.put("a", &1.into());
    /// let res = store.update(&["a"], |old| {
    ///     if let Value::Number(n) = old.get("a").expect("n is required").clone() {
    ///         let n = n.as_i64().expect("n is required");
    ///         if n == 1 {
    ///             return Err(LuaError::runtime("n equals to 1"));
    ///         }
    ///         old.insert("a".into(), json!(n + 1));
    ///     }
    ///     Ok(())
    /// }, Some(hashmap!{ "a".into() => 1.into() }));
    /// assert!(res.is_err());
    /// assert_eq!(json!(1), store.get("a")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn update<S: AsRef<str>>(
        &self,
        names: &[S],
        f: impl FnOnce(Arc<DashMap<Box<str>, Value>>) -> mlua::Result<()>,
        default_values: Option<HashMap<Box<str>, Value>>,
    ) -> Result<HashMap<Box<str>, Value>> {
        let Some(mut conn) = self.conn.try_lock_for(self.busy_timeout) else {
            return Err(crate::Error::DatabaseBusy);
        };

        let tx = conn.transaction()?;

        let names = names
            .iter()
            .map(|name| name.as_ref().to_owned().into_boxed_str())
            .collect::<Vec<_>>();
        let _s = trace_span!("store_update", ?names).entered();

        let mut default_values = default_values.unwrap_or_default();

        let values = DashMap::new();
        for name in &names {
            let _s = trace_span!("query", name).entered();
            let (sql, values_) = stmt_get_value_by_name(name);
            let mut cached_stmt = tx.prepare_cached(&sql)?;
            let value = match cached_stmt.query_row(&*values_.as_params(), |row| row.get("value")) {
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    trace!("use default value");
                    let default_value = default_values
                        .entry(name.clone())
                        .or_insert_with(|| Value::Null)
                        .clone();
                    rmp_serde::to_vec(&default_value)?
                }
                Err(e) => return Err(e.into()),
                Ok(v) => {
                    trace!("value is present");
                    v
                }
            };
            let value: Value = rmp_serde::from_slice(&value)?;
            values.insert(name.clone(), value);
        }

        let values = Arc::new(values);
        {
            let _s = trace_span!("call_function").entered();
            f(values.clone())?;
        }

        for name in &names {
            let _s = trace_span!("upsert", name).entered();
            let value = values
                .entry(name.clone())
                .or_insert_with(|| Value::Null)
                .clone();
            let size = Self::get_size(&value);
            let type_hint = Self::type_hint(&value);

            let value = rmp_serde::to_vec(&value)?;
            let (sql, values) = stmt_upsert_store(name, value, size, type_hint)?;
            let mut cached_stmt = tx.prepare_cached(&sql)?;
            cached_stmt.execute(&*values.as_params())?;
        }

        tx.commit()?;
        trace!("value updated");

        let values = values
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect::<HashMap<_, _>>();
        Ok(values)
    }

    fn get_size(v: &Value) -> usize {
        match v {
            Value::Null => size_of::<()>(),
            Value::Bool(_) => size_of::<bool>(),
            Value::Number(n) => match (n.as_u64(), n.as_i64(), n.as_f64()) {
                (Some(_), _, _) => size_of::<u64>(),
                (_, Some(_), _) => size_of::<i64>(),
                (_, _, Some(_)) => size_of::<f64>(),
                (_, _, _) => unreachable!(),
            },
            Value::String(s) => s.capacity(),
            Value::Array(a) => a.iter().fold(0, |acc, e| acc + Self::get_size(e)),
            Value::Object(m) => m
                .iter()
                .fold(0, |acc, (k, v)| acc + k.capacity() + Self::get_size(v)),
        }
    }

    fn type_hint(v: &Value) -> &'static str {
        match v {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}

/// Value metadata. The value itself is intentionally not included.
#[derive(Debug)]
pub struct StoreValueMetadata {
    /// Name.
    pub name: Box<str>,
    /// Size in bytes.
    pub size: usize,
    /// Type hint e.g. String.
    pub type_hint: Box<str>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Update timestamp.
    pub updated_at: DateTime<Utc>,
}

impl Default for Store {
    /// Open and initialize a `SQLite` database in memory.
    fn default() -> Self {
        debug!("open store in memory");
        let conn = Connection::open_in_memory().expect("failed to open SQLite database in memory");
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
        };
        store
            .migrate(None)
            .expect("failed to migrate SQLite database in memory");
        store
    }
}

#[cfg(test)]
mod tests {
    use assert_fs::NamedTempFile;
    use serde_json::{Value, json};
    use std::{thread, time::Duration};
    use test_case::test_case;
    use tokio::io::empty;

    use crate::{Evaluation, Store};

    #[test]
    fn concurrency() {
        let script = r#"
        return require('@lmb').store:update({ 'a' }, function(s)
          s.a = s.a + 1
        end, { a = 0 })
        "#;

        let store = Store::default();

        let mut threads = vec![];
        for _ in 0..=1000 {
            let store = store.clone();
            threads.push(thread::spawn(move || {
                let e = Evaluation::builder(script, empty())
                    .store(store)
                    .build()
                    .unwrap();
                e.evaluate().call().unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(json!(1001), store.get("a").unwrap());
    }

    #[test_case("a", json!([true, 1, 1.23, "hello"]), 1+8+8+5)]
    #[test_case("o", json!({ "bool": true, "num": 1.23, "str": "hello" }), (4+1)+(3+8)+(3+5))]
    fn collective_types(key: &'static str, value: Value, size: usize) {
        let store = Store::default();
        store.put(key, &value).unwrap();
        assert_eq!(value, store.get(key).unwrap());

        let values = store.list().unwrap();
        let value = values.first().unwrap();
        assert_eq!(size, value.size);
    }

    #[test]
    fn get_put() {
        let script = r#"
        local m = require('@lmb')
        local a = m.store.a
        assert(not m.store.b)
        m.store.a = 4.56
        return a
        "#;

        let store = Store::default();
        store.put("a", &1.23.into()).unwrap();

        let e = Evaluation::builder(script, empty())
            .store(store.clone())
            .build()
            .unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!(1.23), res.payload);
        assert_eq!(json!(4.56), store.get("a").unwrap());
        assert_eq!(json!(null), store.get("b").unwrap());
    }

    #[test]
    fn migrate() {
        let store = Store::default();
        store.migrate(None).unwrap(); // duplicated
        store.current_version().unwrap();
        store.migrate(Some(0)).unwrap();
    }

    #[test]
    fn nested_update() {
        let store_file = NamedTempFile::new("db.sqlite3").unwrap();
        let store = Store::builder()
            .path(store_file.path())
            .run_migrations(true)
            .busy_timeout(Duration::from_micros(3))
            .build()
            .unwrap();
        store.put("a", &0.into()).unwrap();
        store.put("b", &0.into()).unwrap();

        let source = r#"
          local m = require('@lmb')
          return m.store:update({'a'}, function(s)
            s.a = s.a + 1
            m.store:update({'b'}, function(t)
              t.b = t.b + 1
            end)
          end)
        "#;
        let e = Evaluation::builder(source, empty())
            .store(store)
            .build()
            .unwrap();
        let res = e.evaluate().call();
        let err = res.unwrap_err();
        assert!(err.to_string().contains("lua error: database is busy"));
    }

    #[test]
    fn new_store() {
        let store_file = NamedTempFile::new("db.sqlite3").unwrap();
        let store = Store::builder().path(store_file.path()).build().unwrap();
        store.migrate(None).unwrap();
    }

    #[test_case("nil", json!(null), 0)]
    #[test_case("bt", json!(true), 1)]
    #[test_case("bf", json!(false), 1)]
    #[test_case("ni", json!(1), 8)]
    #[test_case("nf", json!(1.23), 8)]
    #[test_case("s", json!("hello"), 5)]
    fn primitive_types(key: &'static str, value: Value, size: usize) {
        let store = Store::default();
        store.put(key, &value).unwrap();
        assert_eq!(value, store.get(key).unwrap());

        let values = store.list().unwrap();
        let value = values.first().unwrap();
        assert_eq!(size, value.size);
    }

    #[test]
    fn reuse() {
        let script = r#"
        local m = require('@lmb')
        local a = m.store.a
        m.store.a = a + 1
        return a
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = Evaluation::builder(script, empty())
            .store(store.clone())
            .build()
            .unwrap();

        {
            let res = e.evaluate().call().unwrap();
            assert_eq!(json!(1), res.payload);
            assert_eq!(json!(2), store.get("a").unwrap());
        }

        {
            let res = e.evaluate().call().unwrap();
            assert_eq!(json!(2), res.payload);
            assert_eq!(json!(3), store.get("a").unwrap());
        }
    }

    #[test]
    fn update_without_default_value() {
        let script = r#"
        return require('@lmb').store:update({ 'a' }, function(s)
          s.a = s.a + 1
        end)
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = Evaluation::builder(script, empty())
            .store(store.clone())
            .build()
            .unwrap();

        let res = e.evaluate().call().unwrap();
        assert_eq!(json!({ "a": 2 }), res.payload);
        assert_eq!(json!(2), store.get("a").unwrap());
    }

    #[test]
    fn rollback_when_error() {
        let script = r#"
        return require('@lmb').store:update({ 'a' }, function(values)
          local a = table.unpack(values)
          assert(a ~= 1, 'expect a not to equal 1')
          return table.pack(a + 1)
        end, { 0 })
        "#;

        let store = Store::default();
        store.put("a", &1.into()).unwrap();

        let e = Evaluation::builder(script, empty())
            .store(store.clone())
            .build()
            .unwrap();

        let res = e.evaluate().call();
        assert!(res.is_err());

        assert_eq!(json!(1), store.get("a").unwrap());
    }
}
