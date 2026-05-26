//! Regular expression binding module.
//!
//! Luau's built-in `string.match`/`string.find`/`string.gmatch` only support Lua
//! patterns, which lack features such as alternation, lookahead, and non-greedy
//! quantifiers. This module exposes the Rust [`regex`] crate, which is fast and
//! safe (guaranteed linear time, no catastrophic backtracking).
//! Import via `require("@lmb/regex")`.
//!
//! # Available Methods
//!
//! - `match(text, pattern)` - Returns the first match, or `nil` when there is none.
//! - `captures(text, pattern)` - Returns the capture groups of the first match as a
//!   table (excluding the whole match), or `nil` when there is no match.
//! - `find_all(text, pattern)` - Returns every match as a table.
//! - `replace(text, pattern, replacement)` - Replaces the first occurrence.
//! - `replace_all(text, pattern, replacement)` - Replaces every occurrence.
//! - `split(text, pattern)` - Splits `text` on the pattern and returns a table of parts.
//! - `is_match(text, pattern)` - Returns a boolean indicating whether the pattern matches.
//!
//! Invalid patterns raise a Lua error.
//!
//! # Example
//!
//! ```lua
//! local regex = require("@lmb/regex")
//!
//! local m = regex.match("hello world", "\\w+")        -- "hello"
//! local caps = regex.captures("2026-02-09", "(\\d{4})-(\\d{2})-(\\d{2})")
//! -- caps = {"2026", "02", "09"}
//! local all = regex.find_all("a1b2c3", "\\d+")         -- {"1", "2", "3"}
//! local s = regex.replace("foo bar", "\\s+", "-")      -- "foo-bar"
//! local s = regex.replace_all("a1b2c3", "\\d", "X")    -- "aXbXcX"
//! local parts = regex.split("a,b,,c", ",+")            -- {"a", "b", "c"}
//! local ok = regex.is_match("hello123", "\\d+")        -- true
//! ```

use mlua::prelude::*;
use regex::Regex;

pub(crate) struct RegexBinding;

impl LuaUserData for RegexBinding {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_function("match", |_, (text, pattern): (String, String)| {
            let re = Regex::new(&pattern).into_lua_err()?;
            Ok(re.find(&text).map(|m| m.as_str().to_string()))
        });

        methods.add_function("captures", |vm, (text, pattern): (String, String)| {
            let re = Regex::new(&pattern).into_lua_err()?;
            match re.captures(&text) {
                Some(caps) => {
                    let groups: Vec<Option<String>> = caps
                        .iter()
                        .skip(1)
                        .map(|g| g.map(|m| m.as_str().to_string()))
                        .collect();
                    vm.to_value(&groups)
                }
                None => Ok(LuaNil),
            }
        });

        methods.add_function("find_all", |vm, (text, pattern): (String, String)| {
            let re = Regex::new(&pattern).into_lua_err()?;
            let matches: Vec<String> = re
                .find_iter(&text)
                .map(|m| m.as_str().to_string())
                .collect();
            vm.to_value(&matches)
        });

        methods.add_function(
            "replace",
            |_, (text, pattern, replacement): (String, String, String)| {
                let re = Regex::new(&pattern).into_lua_err()?;
                Ok(re.replace(&text, replacement.as_str()).into_owned())
            },
        );

        methods.add_function(
            "replace_all",
            |_, (text, pattern, replacement): (String, String, String)| {
                let re = Regex::new(&pattern).into_lua_err()?;
                Ok(re.replace_all(&text, replacement.as_str()).into_owned())
            },
        );

        methods.add_function("split", |vm, (text, pattern): (String, String)| {
            let re = Regex::new(&pattern).into_lua_err()?;
            let parts: Vec<String> = re.split(&text).map(str::to_string).collect();
            vm.to_value(&parts)
        });

        methods.add_function("is_match", |_, (text, pattern): (String, String)| {
            let re = Regex::new(&pattern).into_lua_err()?;
            Ok(re.is_match(&text))
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::empty;

    use crate::Runner;

    #[tokio::test]
    async fn test_regex() {
        let source = include_str!("../fixtures/bindings/regex.lua");
        let runner = Runner::builder(source, empty()).build().unwrap();
        runner.invoke().call().await.unwrap().result.unwrap();
    }
}
