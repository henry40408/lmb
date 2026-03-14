//! Time utilities binding module.
//!
//! This module provides time utilities not covered by Luau's built-in `os` library.
//! Import via `require("@lmb/time")`.
//!
//! # Available Methods
//!
//! - `now_ms()` - Returns current Unix timestamp in milliseconds (integer).
//! - `parse(input [, format])` - Parses a date string and returns a Unix timestamp in seconds.
//!   When `format` is provided, uses POSIX strptime format specifiers.
//!   When omitted, auto-detects from supported formats (see below).
//!
//! # Auto-detected Formats
//!
//! When `format` is omitted, `parse` tries the following in order:
//!
//! 1. **RFC 3339 / RFC 9557 / ISO 8601** — `2026-02-09T12:00:00Z`, `2026-02-09 12:00:00+08:00`
//! 2. **RFC 2822 / 5322 / 1123** — `Sat, 09 Feb 2026 12:00:00 +0000`
//! 3. **asctime** — `Sat Feb  9 12:00:00 2026`
//!
//! # Example
//!
//! ```lua
//! local time = require("@lmb/time")
//!
//! -- Millisecond precision timestamp
//! local ms = time.now_ms()
//!
//! -- Auto-detect format
//! local ts = time.parse("2026-02-09T12:00:00Z")
//! local ts = time.parse("Sat, 09 Feb 2026 12:00:00 +0000")
//!
//! -- Explicit format
//! local ts = time.parse("2026-02-09", "%Y-%m-%d")
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use jiff::tz::TimeZone;
use mlua::prelude::*;

/// strptime formats for auto-detection, tried in order after jiff's built-in parser.
const AUTO_FORMATS: &[&str] = &[
    // RFC 2822 / 5322 / 1123
    "%a, %d %b %Y %H:%M:%S %z",
    // asctime
    "%a %b %d %H:%M:%S %Y",
];

/// Convert a parsed strptime result to a Unix timestamp in seconds,
/// falling back to UTC when no offset is present.
fn broken_down_to_timestamp(tm: &jiff::fmt::strtime::BrokenDownTime) -> Option<i64> {
    tm.to_timestamp()
        .or_else(|_| {
            tm.to_datetime()
                .and_then(|dt| dt.to_zoned(TimeZone::UTC))
                .map(|z| z.timestamp())
        })
        .ok()
        .map(|ts| ts.as_second())
}

/// Try to parse a date string by auto-detecting the format.
fn parse_auto(input: &str) -> Result<i64, String> {
    // 1. jiff built-in parser (RFC 3339 / RFC 9557 / ISO 8601)
    if let Ok(ts) = input.parse::<jiff::Timestamp>() {
        return Ok(ts.as_second());
    }
    // Also try civil::DateTime for inputs without offset (e.g. "2026-02-09 12:00:00")
    if let Ok(dt) = input.parse::<jiff::civil::DateTime>() {
        let z = dt.to_zoned(TimeZone::UTC).map_err(|e| e.to_string())?;
        return Ok(z.timestamp().as_second());
    }
    // Also try civil::Date for date-only inputs (e.g. "2026-02-09")
    if let Ok(d) = input.parse::<jiff::civil::Date>() {
        let z = d.to_zoned(TimeZone::UTC).map_err(|e| e.to_string())?;
        return Ok(z.timestamp().as_second());
    }

    // 2-3. Try strptime with known formats
    for fmt in AUTO_FORMATS {
        if let Ok(tm) = jiff::fmt::strtime::parse(fmt, input)
            && let Some(ts) = broken_down_to_timestamp(&tm)
        {
            return Ok(ts);
        }
    }

    Err(format!(
        "failed to parse '{input}': no known format matched"
    ))
}

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

        methods.add_function("parse", |_, (input, format): (String, Option<String>)| {
            let ts = match format {
                Some(fmt) => {
                    let tm = jiff::fmt::strtime::parse(&fmt, &input).map_err(LuaError::external)?;
                    broken_down_to_timestamp(&tm).ok_or_else(|| {
                        LuaError::external(format!(
                            "failed to convert parsed date '{input}' to timestamp"
                        ))
                    })?
                }
                None => parse_auto(&input).map_err(LuaError::external)?,
            };
            Ok(ts)
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
