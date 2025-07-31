use std::io::Cursor;

use full_moon::{tokenizer::TokenType, visitors::Visitor};
use lmb::Runner;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use toml::Table;

struct CommentVisitor {
    name: String,
    input: String,
    assert_return: String,
}

impl CommentVisitor {
    fn new() -> Self {
        Self {
            name: String::new(),
            input: String::new(),
            assert_return: String::new(),
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
        if let Some(toml::Value::String(assert_return)) = parsed.get("assert_return") {
            self.assert_return.push_str(assert_return);
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
        let runner = Runner::builder(&source, input).build().unwrap();
        let value = runner.invoke().call().await.unwrap().result.unwrap();
        if visitor.assert_return.is_empty() {
            continue;
        }

        let name = visitor.name.trim();
        if let serde_json::Value::String(value) = &value {
            assert_eq!(&visitor.assert_return, value, "name: {name}");
        } else {
            assert_eq!(visitor.assert_return, value.to_string(), "name: {source}");
        }
    }
}
