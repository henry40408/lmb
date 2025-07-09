#![deny(missing_debug_implementations, missing_docs)]

//! A Lua function runner.

use bon::Builder;
use dashmap::DashMap;
use include_dir::{Dir, include_dir};
use rusqlite_migration::Migrations;
use std::{
    result::Result as StdResult,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::{io::BufReader, sync::Mutex};

pub use error::*;
pub use eval::*;
pub use example::*;
pub use guide::*;
pub use lua_binding::*;
pub use schedule::*;
pub use source::*;
pub use store::*;

mod error;
mod eval;
mod example;
mod guide;
mod lua_binding;
mod schedule;
mod source;
mod store;

/// Default timeout for evaluation in seconds.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Directory containing migration files.
static MIGRATIONS_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// Migrations for the `SQLite` database.
static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::from_directory(&MIGRATIONS_DIR)
        .expect("failed to load migrations from the directory")
});

/// Function input, wrapped in an Arc and Mutex for thread safety.
pub type Input<R> = Arc<Mutex<BufReader<R>>>;

/// Generic result type for the function runner.
pub type Result<T> = StdResult<T, Error>;

/// Enum representing different state keys.
#[derive(Debug, Eq, Hash, PartialEq)]
pub enum StateKey {
    /// HTTP request object
    Request,
    /// HTTP response object
    Response,
    /// Plain string key
    String(Box<str>),
}

impl<S> From<S> for StateKey
where
    S: AsRef<str>,
{
    /// Converts a type that can be referenced as a string into a [`StateKey`].
    fn from(value: S) -> Self {
        Self::String(value.as_ref().into())
    }
}

/// State of each evaluation, using a [`dashmap::DashMap`].
pub type State = DashMap<StateKey, serde_json::Value>;

/// Options for printing scripts.
#[derive(Builder, Debug)]
pub struct PrintOptions {
    /// Disable colors [`https://no-colors.org`].
    no_color: bool,
    /// Theme.
    theme: Option<Box<str>>,
}

#[cfg(test)]
mod tests {
    use crate::{Evaluation, MIGRATIONS, StateKey, Store};
    use http::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
    use serde_json::json;
    use tokio::io::empty;

    #[tokio::test]
    async fn test_evaluation() {
        let markdown = include_str!("../guides/lua.md");
        let blocks = {
            let mut blocks = Vec::new();
            let parser = Parser::new_ext(markdown, Options::all());
            let mut is_code = false;
            let mut text = String::new();

            for event in parser {
                match &event {
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                        if &**lang == "lua" {
                            is_code = true;
                        }
                    }
                    Event::Text(t) => {
                        if is_code {
                            text.push_str(t);
                        }
                    }
                    Event::End(TagEnd::CodeBlock) => {
                        if is_code {
                            blocks.push(text.clone());
                            text.clear();
                            is_code = false;
                        }
                    }
                    _ => {}
                }
            }
            blocks
        };

        let mut server = mockito::Server::new_async().await;

        let headers_mock = server
            .mock("GET", "/headers")
            .with_status(200)
            .match_request(|r| r.has_header(ACCEPT) && r.has_header(USER_AGENT))
            .match_header("I-Am", "A teapot")
            .with_header(CONTENT_TYPE, "application/json")
            .with_body(
                serde_json::to_string(&json!({ "headers": { "I-Am": "A teapot" } })).unwrap(),
            )
            .create_async()
            .await;

        let post_mock = server
            .mock("POST", "/post")
            .match_request(|r| r.has_header(ACCEPT) && r.has_header(USER_AGENT))
            .with_status(200)
            .with_header(CONTENT_TYPE, "application/json")
            .with_body(
                serde_json::to_string(
                    &json!({ "data": serde_json::to_string(&json!({ "foo": "bar" })).unwrap() }),
                )
                .unwrap(),
            )
            .create_async()
            .await;

        for block in blocks {
            let block = block.replace("https://httpbingo.org", &server.url());
            let store = Store::default();
            let e = Evaluation::builder(&*block, empty())
                .store(store)
                .build()
                .unwrap();
            if let Err(err) = e.evaluate_async().call().await {
                let mut f = String::new();
                let _ = e.write_errors(&mut f, vec![&err]);
                eprintln!("{f}");
                panic!("evaluation failed");
            }
        }

        post_mock.assert_async().await;
        headers_mock.assert_async().await;
    }

    #[test]
    fn migrations() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn state_key_from_str() {
        let _ = StateKey::from("key");
    }
}
