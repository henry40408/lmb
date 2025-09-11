pub(crate) static MIGRATIONS: &[&str] = &[include_str!("migrations/0001-initial.sql")];

pub(crate) static SQL_GET: &str = "SELECT value FROM store WHERE key = ?";
pub(crate) static SQL_PUT: &str = "INSERT OR REPLACE INTO store (key, value) VALUES (?, ?)";
