use std::io::Cursor;

use full_moon::{tokenizer::TokenType, visitors::Visitor};
use lmb::Runner;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use rusqlite::Connection;
use serde_json::Value;
use toml::Table;

struct CommentVisitor {
    name: String,
    input: String,
    store: bool,
    state: Option<Value>,
    assert_return: Option<Value>,
}

impl CommentVisitor {
    fn new() -> Self {
        Self {
            name: String::new(),
            input: String::new(),
            store: false,
            state: None,
            assert_return: None,
        }
    }
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
        if let Some(toml::Value::String(input)) = parsed.get("input") {
            self.input.push_str(input);
        }
        if let Some(toml::Value::Boolean(store)) = parsed.get("store") {
            self.store = *store;
        }
        if let Some(state) = parsed.get("state") {
            let state = serde_json::to_string(state).expect("failed to serialize state");
            self.state = Some(serde_json::from_str(&state).expect("failed to parse state"));
        }
        if let Some(assert_return) = parsed.get("assert_return") {
            let assert_return =
                serde_json::to_string(assert_return).expect("failed to serialize assert_return");
            self.assert_return =
                Some(serde_json::from_str(&assert_return).expect("failed to parse assert_return"));
        }
    }
}

#[tokio::test]
async fn test_guided_tour() {
    let content = include_str!("../GUIDED_TOUR.md");
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

        let mut visitor = CommentVisitor::new();
        visitor.visit_ast(&parsed);

        let input = Cursor::new(visitor.input);
        let conn = if visitor.store {
            Some(Connection::open_in_memory().unwrap())
        } else {
            None
        };
        let runner = Runner::builder(&source, input)
            .maybe_store(conn)
            .build()
            .unwrap();
        let value = runner
            .invoke()
            .maybe_state(visitor.state)
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
