CREATE TABLE IF NOT EXISTS store (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  value BLOB NOT NULL
);