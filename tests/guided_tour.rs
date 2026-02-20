use std::{io::Cursor, str::FromStr, time::Duration};

use curl_parser::ParsedRequest;
use full_moon::{tokenizer::TokenType, visitors::Visitor};
use lmb::{
    Runner, State,
    permission::{EnvPermissions, NetPermissions, Permissions, ReadPermissions, WritePermissions},
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use rusqlite::Connection;
use rustc_hash::FxHashSet;
use serde_json::{Value, json};
use tempfile::TempDir;
use toml::Table;

#[derive(Default)]
struct CommentVisitor {
    allow_read: bool,
    allow_write: bool,
    curl: Option<String>,
    name: String,
    assert_return: Option<Value>,
    input: String,
    state: Option<Value>,
    store: bool,
    timeout: Option<Duration>,
}

impl Visitor for CommentVisitor {
    fn visit_multi_line_comment(&mut self, token: &full_moon::tokenizer::Token) {
        let TokenType::MultiLineComment { comment, .. } = token.token_type() else {
            return;
        };
        let comment = comment
            .split('\n')
            .map(|s| s.trim_start_matches('-'))
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(parsed) = comment.trim_end_matches('-').to_owned().parse::<Table>() else {
            return;
        };
        self.name = parsed
            .get("name")
            .expect("name is required")
            .as_str()
            .expect("expect a string")
            .to_string();
        if let Some(assert_return) = parsed.get("assert_return") {
            let assert_return =
                serde_json::to_string(assert_return).expect("failed to serialize assert_return");
            self.assert_return =
                Some(serde_json::from_str(&assert_return).expect("failed to parse assert_return"));
        }
        if let Some(toml::Value::String(input)) = parsed.get("input") {
            self.input.push_str(input);
        }
        if let Some(state) = parsed.get("state") {
            let state = serde_json::to_string(state).expect("failed to serialize state");
            self.state = Some(serde_json::from_str(&state).expect("failed to parse state"));
        }
        if let Some(toml::Value::Boolean(store)) = parsed.get("store") {
            self.store = *store;
        }
        if let Some(toml::Value::Integer(timeout)) = parsed.get("timeout") {
            self.timeout = Some(Duration::from_millis(*timeout as u64));
        }
        if let Some(toml::Value::String(curl)) = parsed.get("curl") {
            self.curl = Some(curl.clone());
        }
        if let Some(toml::Value::Boolean(allow_read)) = parsed.get("allow_read") {
            self.allow_read = *allow_read;
        }
        if let Some(toml::Value::Boolean(allow_write)) = parsed.get("allow_write") {
            self.allow_write = *allow_write;
        }
    }
}

#[tokio::test]
async fn test_guided_tour() {
    let content = include_str!("../docs/guided-tour.md");
    let parser = Parser::new_ext(content, Options::all());

    let sources = {
        let mut sources = Vec::new();

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
                        sources.push(text.clone());
                        text.clear();
                        is_code = false;
                    }
                }
                _ => {}
            }
        }
        sources
    };

    for source in sources {
        let parsed = full_moon::parse(&source).unwrap();

        let mut visitor = CommentVisitor::default();
        visitor.visit_ast(&parsed);

        let mut server = mockito::Server::new_async().await;
        let _ = server
            .mock("GET", "/get")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(r#"{"a":1}"#)
            .create_async()
            .await;
        if let Some(ref mut state) = visitor.state
            && let Some(state) = state.as_object_mut()
        {
            state.insert("url".to_string(), json!(server.url()));
        }

        let conn = if visitor.store {
            Some(Connection::open_in_memory().unwrap())
        } else {
            None
        };

        // Create temp dir and permissions when fs access is requested
        let _temp_dir: Option<TempDir> = if visitor.allow_read || visitor.allow_write {
            Some(TempDir::new().expect("failed to create temp dir"))
        } else {
            None
        };
        let permissions = if visitor.allow_read || visitor.allow_write {
            let temp_path = std::fs::canonicalize(_temp_dir.as_ref().unwrap().path())
                .expect("failed to canonicalize temp dir");
            // Inject temp_dir into state
            let state = visitor.state.get_or_insert_with(|| json!({}));
            if let Some(obj) = state.as_object_mut() {
                obj.insert("temp_dir".to_string(), json!(temp_path.to_string_lossy()));
            }
            let read = if visitor.allow_read {
                ReadPermissions::All {
                    denied: FxHashSet::default(),
                }
            } else {
                ReadPermissions::Some {
                    allowed: FxHashSet::default(),
                    denied: FxHashSet::default(),
                }
            };
            let write = if visitor.allow_write {
                WritePermissions::Some {
                    allowed: [temp_path].into_iter().collect(),
                    denied: FxHashSet::default(),
                }
            } else {
                WritePermissions::Some {
                    allowed: FxHashSet::default(),
                    denied: FxHashSet::default(),
                }
            };
            Some(Permissions::Some {
                env: EnvPermissions::All {
                    denied: FxHashSet::default(),
                },
                net: NetPermissions::All {
                    denied: FxHashSet::default(),
                },
                read,
                write,
            })
        } else {
            None
        };

        let (request, input) = match &visitor.curl {
            Some(curl) => {
                let parsed = ParsedRequest::from_str(curl).unwrap();
                let headers = {
                    let mut m = json!({});
                    for (k, v) in parsed.headers.iter() {
                        m[k.as_str()] = json!(v.to_str().unwrap());
                    }
                    m
                };
                let request = json!({
                    "method": parsed.method.as_str(),
                    "path": parsed.url.path(),
                    "headers": headers,
                });
                (Some(request), Cursor::new(parsed.body.concat()))
            }
            None => (None, Cursor::new(visitor.input)),
        };
        let runner = Runner::builder(&source, input)
            .maybe_permissions(permissions)
            .maybe_store(conn)
            .maybe_timeout(visitor.timeout)
            .build()
            .unwrap();
        let state = State::builder()
            .maybe_request(request)
            .maybe_state(visitor.state)
            .build();
        let value = runner
            .invoke()
            .state(state)
            .call()
            .await
            .unwrap()
            .result
            .unwrap();

        let name = visitor.name.trim();
        if let Some(assert_return) = visitor.assert_return {
            assert_eq!(assert_return, value, "name: {name}");
        }
    }
}
