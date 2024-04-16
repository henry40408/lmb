use crate::*;
use include_dir::{include_dir, Dir};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::{path::Path, sync::Arc};
use tracing::debug;

static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

const FIND_SQL: &str = r#"SELECT value FROM store WHERE name = ?1"#;
const UPDATE_SQL: &str = r#"INSERT INTO store (name, value) VALUES (?1, ?2)
    ON CONFLICT(name) DO UPDATE SET value = ?2"#;

#[derive(Clone, Debug)]
pub struct LamStore {
    pub conn: Arc<Mutex<Connection>>,
}

impl LamStore {
    pub fn new(path: &Path) -> LamResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn migrate(&self) -> LamResult<()> {
        let conn = self.conn.lock();
        for e in MIGRATIONS_DIR.entries() {
            let path = e.path();
            debug!(?path, "open migration file");
            let sql = e
                .as_file()
                .expect("invalid file")
                .contents_utf8()
                .expect("invalid contents");
            debug!(?sql, "run migration SQL");
            conn.execute(sql, ())?;
        }
        Ok(())
    }

    pub fn insert<S: AsRef<str>>(&self, name: S, value: &LamValue) -> LamResult<()> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let value = rmp_serde::to_vec(&value)?;
        conn.execute(UPDATE_SQL, (name, value))?;

        Ok(())
    }

    pub fn get<S: AsRef<str>>(&self, name: S) -> LamResult<LamValue> {
        let conn = self.conn.lock();

        let name = name.as_ref();
        let v: Vec<u8> = match conn.query_row(FIND_SQL, (name,), |row| row.get(0)) {
            Err(_) => return Ok(LamValue::None),
            Ok(v) => v,
        };

        Ok(rmp_serde::from_slice::<LamValue>(&v)?)
    }

    pub fn update<S: AsRef<str>>(
        &self,
        name: S,
        f: impl FnOnce(&mut LamValue),
        default_v: &LamValue,
    ) -> LamResult<LamValue> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let name = name.as_ref();

        let v: Vec<u8> = match tx.query_row(FIND_SQL, (name,), |row| row.get(0)) {
            Err(_) => rmp_serde::to_vec(default_v)?,
            Ok(v) => v,
        };

        let mut deserialized = rmp_serde::from_slice(&v)?;
        f(&mut deserialized);
        let serialized = rmp_serde::to_vec(&deserialized)?;

        tx.execute(UPDATE_SQL, (name, serialized))?;
        tx.commit()?;

        Ok(deserialized)
    }
}

impl Default for LamStore {
    fn default() -> Self {
        Self {
            conn: Arc::new(Mutex::new(
                Connection::open_in_memory().expect("failed to open sqlite in memory"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::{collections::HashMap, thread};

    fn new_store() -> LamStore {
        let store = LamStore::default();
        store.migrate().unwrap();
        store
    }

    #[test]
    fn complicated_types() {
        let store = new_store();

        let l = LamValue::List(vec![
            LamValue::Boolean(true),
            LamValue::Number(1f64),
            LamValue::String("hello".to_string()),
        ]);
        store.insert("list", &l).unwrap();
        assert_eq!("table: 0x0", store.get("list").unwrap().to_string());

        let mut h = HashMap::new();
        h.insert("b".into(), LamValue::Boolean(true));
        h.insert("n".into(), LamValue::Number(1f64));
        h.insert("s".into(), LamValue::String("hello".to_string()));
        store.insert("table", &LamValue::Table(h)).unwrap();
        assert_eq!("table: 0x0", store.get("table").unwrap().to_string());
    }

    #[test]
    fn concurrency() {
        let input: &[u8] = &[];

        let store = new_store();

        let mut threads = vec![];
        for _ in 0..=1000 {
            let store = store.clone();
            threads.push(thread::spawn(move || {
                let e = EvalBuilder::new(
                    input,
                    r#"
                    return require('@lam'):update('a', function(v)
                      return v+1
                    end, 0)
                    "#,
                )
                .set_store(store)
                .build();
                e.evaluate().unwrap();
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        assert_eq!(LamValue::Number(1001f64), store.get("a").unwrap());
    }

    #[test]
    fn lua() {
        let input: &[u8] = &[];

        let store = new_store();
        store.insert("a", &LamValue::Number(1.23)).unwrap();

        let e = EvalBuilder::new(
            input,
            r#"
            local m = require('@lam')
            local a = m:get('a')
            m:set('a', 4.56)
            return a
            "#,
        )
        .set_store(store)
        .build();

        let res = e.evaluate().unwrap();
        assert_eq!("1.23", res.result.to_string());
        assert_eq!(LamValue::Number(4.56), e.store.get("a").unwrap());
    }

    #[test]
    fn migrate() {
        let store = new_store();
        store.migrate().unwrap(); // duplicated
    }

    #[test]
    fn primitive_types() {
        let store = new_store();

        assert_eq!(store.get("x").unwrap(), LamValue::None);

        let data = [
            ("nil", LamValue::None),
            ("bt", LamValue::Boolean(true)),
            ("bf", LamValue::Boolean(false)),
            ("ni", LamValue::Number(1f64)),
            ("nf", LamValue::Number(1.23f64)),
            ("s", LamValue::String("hello".to_string())),
        ];
        for (name, value) in data {
            store.insert(name, &value).unwrap();
            assert_eq!(value, store.get(name).unwrap());
        }
    }

    #[test]
    fn reuse() {
        let input: &[u8] = &[];

        let store = new_store();
        store.insert("a", &LamValue::Number(1f64)).unwrap();

        let e = EvalBuilder::new(
            input,
            r#"
            local m = require('@lam')
            local a = m:get('a')
            m:set('a', a+1)
            return a
            "#,
        )
        .set_store(store)
        .build();

        {
            let res = e.evaluate().unwrap();
            assert_eq!("1", res.result.to_string());
            assert_eq!(LamValue::Number(2f64), e.store.get("a").unwrap());
        }

        {
            let res = e.evaluate().unwrap();
            assert_eq!("2", res.result.to_string());
            assert_eq!(LamValue::Number(3f64), e.store.get("a").unwrap());
        }
    }

    #[test_log::test]
    fn rollback_when_error() {
        let input: &[u8] = &[];

        let store = new_store();
        store.insert("a", &LamValue::Number(1f64)).unwrap();

        let e = EvalBuilder::new(
            input,
            r#"return require('@lam'):update('a', function(v)
              if v == 1 then
                error('something went wrong')
              else
                return v+1
              end
            end, 0)"#,
        )
        .set_store(store)
        .build();

        let res = e.evaluate().unwrap();
        assert_eq!("1", res.result.to_string());
        assert_eq!(LamValue::Number(1f64), e.store.get("a").unwrap());
    }
}
