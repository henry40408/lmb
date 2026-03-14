/// Store backend trait definition.
pub mod backend;
/// `SQLite` store backend implementation.
pub mod sqlite;

pub use backend::StoreBackend;
pub use sqlite::SqliteBackend;
