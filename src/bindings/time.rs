//! Time utilities binding module.
//!
//! This module provides time utilities not covered by Luau's built-in `os` library.
//! Import via `require("@lmb/time")`.
//!
//! # Available Methods
//!
//! - `now_ms()` - Returns current Unix timestamp in milliseconds (integer).
//! - `parse(input, format)` - Parses a date string using POSIX strptime format specifiers
//!   and returns a Unix timestamp in seconds (integer).
//!
//! # Example
//!
//! ```lua
//! local time = require("@lmb/time")
//!
//! -- Millisecond precision timestamp
//! local ms = time.now_ms()
//!
//! -- Parse date strings
//! local ts = time.parse("2026-02-09", "%Y-%m-%d")
//! local ts2 = time.parse("2026-02-09 12:00:00", "%Y-%m-%d %H:%M:%S")
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use jiff::tz::TimeZone;
use mlua::prelude::*;

/// Time binding that exposes time utilities to Lua.
pub(crate) struct TimeBinding;

impl LuaUserData for TimeBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("now_ms", |_, ()| {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(LuaError::external)?
                .as_millis() as i64;
            Ok(ms)
        });

        methods.add_function("parse", |_, (input, format): (String, String)| {
            let tm = jiff::fmt::strtime::parse(&format, &input).map_err(LuaError::external)?;
            // Try to convert directly to timestamp (works when offset is present).
            // Fall back to interpreting as UTC datetime.
            let ts = tm
                .to_timestamp()
                .or_else(|_| {
                    tm.to_datetime()
                        .and_then(|dt| dt.to_zoned(TimeZone::UTC))
                        .map(|z| z.timestamp())
                })
                .map_err(LuaError::external)?;
            Ok(ts.as_second())
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_time() {
        let source = include_str!("../fixtures/bindings/time.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
