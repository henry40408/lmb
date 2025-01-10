use bon::{bon, Builder};
use lazy_regex::{lazy_regex, Lazy, Regex};
use miette::{miette, LabeledSpan};
use mlua::prelude::*;
use std::fmt::Write;
use string_offsets::StringOffsets;

use crate::Error;

static LUA_ERROR_REGEX: Lazy<Regex> = lazy_regex!(r"\[[^\]]+\]:(\d+):(.+)");

/// Container holding name and script of Lua script.
#[derive(Builder, Clone, Debug)]
pub struct LuaSource {
    /// Script.
    #[builder(start_fn, into)]
    pub script: String,
    /// Name.
    pub name: Option<String>,
}

impl From<String> for LuaSource {
    fn from(value: String) -> Self {
        LuaSource::builder(value).build()
    }
}

impl From<&str> for LuaSource {
    fn from(value: &str) -> Self {
        LuaSource::builder(value).build()
    }
}

#[bon]
impl LuaSource {
    /// Check the syntax of the script.
    ///
    /// # Errors
    ///
    /// This function will return an error if the script contains syntax errors.
    ///
    /// ```rust
    /// use lmb::LuaSource;
    ///
    /// let check = LuaSource::builder("ret true").build();
    /// assert!(check.check().is_err());
    /// ```
    pub fn check(&self) -> Result<full_moon::ast::Ast, Vec<Error>> {
        full_moon::parse(self.script.as_ref()).map_err(|errs| {
            errs.into_iter()
                .map(|e| Error::LuaSyntax(Box::new(e)))
                .collect::<Vec<Error>>()
        })
    }

    /// Render [`crate::error::Error`] to a writer.
    ///
    /// # Errors
    ///
    /// This function will return an [`std::fmt::Error`]
    /// if there is an issue writing the error to the provided writer.
    #[builder]
    pub fn write_errors<W>(
        &self,
        #[builder(start_fn)] mut f: W,
        #[builder(start_fn)] errors: Vec<&Error>,
    ) -> Result<(), std::fmt::Error>
    where
        W: Write,
    {
        for error in &errors {
            let (message, start, end) = match error {
                Error::Lua(
                    LuaError::RuntimeError(message) | LuaError::SyntaxError { message, .. },
                ) => {
                    let first_line = message.lines().next().unwrap_or_default();
                    let captures = LUA_ERROR_REGEX.captures(first_line);
                    let line_number = captures
                        .as_ref()
                        .and_then(|c| c.get(1))
                        .map(|m| m.as_str())
                        .and_then(|l| l.parse::<usize>().ok());
                    let message = captures.as_ref().and_then(|c| c.get(2)).map(|m| m.as_str());
                    let offsets = StringOffsets::new(&self.script);
                    match (line_number, message) {
                        (Some(line_number), Some(message)) => {
                            let span = offsets.line_to_chars(line_number - 1);
                            (message.to_string(), span.start, span.end)
                        }
                        (Some(line_number), None) => {
                            let span = offsets.line_to_chars(line_number - 1);
                            (first_line.to_string(), span.start, span.end)
                        }
                        (None, Some(message)) => (message.to_string(), 0usize, 1usize),
                        (None, None) => (first_line.to_string(), 0usize, 1usize),
                    }
                }
                Error::LuaSyntax(e) => match **e {
                    full_moon::Error::AstError(ref e) => (
                        e.error_message().to_string(),
                        e.token().start_position().bytes(),
                        e.token().end_position().bytes(),
                    ),
                    full_moon::Error::TokenizerError(ref e) => (
                        e.error().to_string(),
                        e.position().bytes(),
                        e.position().bytes() + 1,
                    ),
                },
                _ => continue,
            };
            let report = miette!(
                labels = vec![LabeledSpan::at(start..end, message)],
                "{message}"
            )
            .with_source_code(self.script.clone());
            write!(f, "{:?}", report)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use crate::{Error, Evaluation, LuaSource};

    #[test]
    fn runtime_error() {
        let script = "return nil+1";
        let source: LuaSource = script.into();
        let e = Evaluation::builder(source.clone(), empty())
            .build()
            .unwrap();
        let err = e.evaluate().call().unwrap_err();
        let mut buf = String::new();
        source.write_errors(&mut buf, vec![&err]).call().unwrap();
        assert!(buf.contains("attempt to perform arithmetic (add) on nil and number"));
    }

    #[test]
    fn syntax() {
        let script = "ret true";
        let source = LuaSource::builder(script).build();
        assert!(source.check().is_err());

        let script = "return true";
        let source = LuaSource::builder(script).build();
        assert!(source.check().is_ok());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true";
        let source = LuaSource::builder(script).build();
        let errors = source.check().unwrap_err();
        let errors: Vec<&Error> = errors.iter().collect();
        let mut buf = String::new();
        source.write_errors(&mut buf, errors).call().unwrap();
    }
}
