use mlua::prelude::*;
use thiserror::Error;

/// Custom error type for handling various error scenarios.
#[derive(Debug, Error)]
pub enum Error {
    /// Error from the [`bat`] library
    #[error("bat error: {0}")]
    Bat(#[from] bat::error::Error),
    /// Error from the `SQLite` database
    #[error("sqlite error: {0}")]
    Database(#[from] rusqlite::Error),
    /// Error from database migration
    #[error("migration error: {0}")]
    DatabaseMigration(#[from] rusqlite_migration::Error),
    /// Error in formatting output
    #[error("format error: {0}")]
    Format(#[from] std::fmt::Error),
    /// Invalid key length for HMAC
    #[error("invalid length: {0}")]
    InvalidLength(#[from] crypto_common::InvalidLength),
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Error from the Lua engine
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),
    /// Lua syntax error
    #[error("lua syntax error: {0}")]
    LuaSyntax(#[from] Box<full_moon::Error>),
    /// Error decoding value from `MessagePack` format
    #[error("RMP decode error: {0}")]
    RMPDecode(#[from] rmp_serde::decode::Error),
    /// Error encoding value to `MessagePack` format
    #[error("RMP encode error: {0}")]
    RMPEncode(#[from] rmp_serde::encode::Error),
    /// Error from [`serde_json`] library
    #[error("serde JSON error: {0}")]
    SerdeJSONError(#[from] serde_json::Error),
}
