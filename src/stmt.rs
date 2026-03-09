pub(crate) static MIGRATIONS: &[&str] = &[include_str!("migrations/0001-initial.sql")];

pub(crate) static SQL_GET: &str = "SELECT value FROM store WHERE key = ?";
pub(crate) static SQL_PUT: &str = "INSERT OR REPLACE INTO store (key, value) VALUES (?, ?)";
pub(crate) static SQL_DEL: &str = "DELETE FROM store WHERE key = ?";
pub(crate) static SQL_HAS: &str = "SELECT 1 FROM store WHERE key = ?";
pub(crate) static SQL_KEYS: &str = "SELECT key FROM store WHERE key LIKE ?";
pub(crate) static SQL_KEYS_ALL: &str = "SELECT key FROM store";
