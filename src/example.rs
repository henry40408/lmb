use std::sync::LazyLock;

use bon::Builder;
use full_moon::{tokenizer::TokenType, visitors::Visitor};
use include_dir::{include_dir, Dir};
use toml::{Table, Value};

use crate::LuaSource;

/// Lua example.
#[derive(Builder, Debug)]
pub struct Example {
    /// Source code.
    #[builder(start_fn)]
    pub source: LuaSource,
    /// Description.
    #[builder(default)]
    pub description: String,
    #[builder(default)]
    done: bool,
}

impl Visitor for Example {
    /// Extract the description from the first multi-line comment of a Lua script.
    fn visit_multi_line_comment(&mut self, token: &full_moon::tokenizer::Token) {
        if self.done {
            return;
        }
        let TokenType::MultiLineComment { comment, .. } = token.token_type() else {
            return;
        };
        let comment = comment
            .split('\n')
            .map(|s| s.trim_start_matches('-'))
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(parsed) = comment.trim_end_matches('-').to_string().parse::<Table>() else {
            return;
        };
        let Value::String(description) = &parsed["description"] else {
            return;
        };
        self.description = description.to_string();
        self.done = true;
    }
}

impl Example {
    /// Return name or empty string.
    pub fn name(&self) -> &str {
        self.source.name.as_deref().unwrap_or_default()
    }
}

static EXAMPLES_DIR: Dir<'_> = include_dir!("lua-examples");

/// Embedded Lua examples.
pub static EXAMPLES: LazyLock<Vec<Example>> = LazyLock::new(|| {
    let mut examples = vec![];
    for f in EXAMPLES_DIR
        .find("**/*.lua")
        .expect("failed to list Lua examples")
    {
        let Some(name) = f.path().file_stem().map(|f| f.to_string_lossy()) else {
            continue;
        };
        let Some(script) = f.as_file().and_then(|handle| handle.contents_utf8()) else {
            continue;
        };
        let source = LuaSource::builder(script).name(name.to_string()).build();
        let mut example = Example::builder(source).build();
        let Ok(ast) = full_moon::parse(script) else {
            continue;
        };
        example.visit_ast(&ast);
        examples.push(example);
    }
    examples.sort_by(|a, b| a.source.name.cmp(&b.source.name));
    examples
});

#[cfg(test)]
mod tests {
    use crate::EXAMPLES;

    #[test]
    fn description_of_examples() {
        for e in EXAMPLES.iter() {
            let name = e.name();
            assert!(!e.description.is_empty(), "{name} has no description");
        }
    }
}
