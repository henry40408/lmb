use bat::{
    assets::HighlightingAssets,
    controller::Controller,
    input::Input as BatInput,
    style::{StyleComponent, StyleComponents},
};
use bon::{Builder, bon};
use console::Term;
use lazy_regex::{Lazy, Regex, lazy_regex};
use miette::{LabeledSpan, miette};
use mlua::prelude::*;
use std::{
    fmt::Write,
    io::{IsTerminal as _, stdout},
};
use string_offsets::StringOffsets;

use crate::{Error, PrintOptions};

static LUA_ERROR_REGEX: Lazy<Regex> = lazy_regex!(r"\[[^\]]+\]:(\d+):(.+)");

/// Container holding name and script of Lua script.
#[derive(Builder, Clone, Debug)]
pub struct LuaSource {
    /// Script.
    #[builder(start_fn, into)]
    pub script: Box<str>,
    /// Name.
    pub name: Option<Box<str>>,
    /// Next source that can be called by the current source.
    pub next: Option<Box<LuaSource>>,
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

impl From<Box<str>> for LuaSource {
    fn from(value: Box<str>) -> Self {
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
                    let Some(line_number) = captures
                        .as_ref()
                        .and_then(|c| c.get(1))
                        .map(|m| m.as_str())
                        .and_then(|l| l.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    let Some(message) =
                        captures.as_ref().and_then(|c| c.get(2)).map(|m| m.as_str())
                    else {
                        continue;
                    };
                    let offsets = StringOffsets::new(&self.script);
                    let span = offsets.line_to_chars(line_number - 1);
                    (message.to_owned().into_boxed_str(), span.start, span.end)
                }
                Error::LuaSyntax(e) => match **e {
                    full_moon::Error::AstError(ref e) => (
                        e.error_message().to_owned().into_boxed_str(),
                        e.token().start_position().bytes(),
                        e.token().end_position().bytes(),
                    ),
                    full_moon::Error::TokenizerError(ref e) => (
                        e.error().to_string().into_boxed_str(),
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
            .with_source_code(self.script.to_string());
            write!(f, "{:?}", report)?;
        }
        Ok(())
    }

    /// Render the script.
    ///
    /// ```rust
    /// # use std::io::empty;
    /// use lmb::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let source: LuaSource = "return 1".into();
    /// let mut buf = String::new();
    /// let print_options = PrintOptions::builder().no_color(true).build();
    /// source.write_script(&mut buf, &print_options)?;
    /// assert!(buf.contains("return 1"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_script<W>(&self, mut f: W, options: &PrintOptions) -> crate::Result<bool>
    where
        W: Write,
    {
        let (style_components, colored_output) = if stdout().is_terminal() {
            let components = &[StyleComponent::Grid, StyleComponent::LineNumbers];
            (StyleComponents::new(components), !options.no_color)
        } else {
            (StyleComponents::new(&[]), false)
        };
        let mut config = bat::config::Config {
            colored_output,
            language: Some("lua"),
            style_components,
            true_color: true,
            // required to print line numbers
            term_width: Term::stdout().size().1 as usize,
            ..Default::default()
        };
        if let Some(theme) = &options.theme {
            config.theme = theme.to_string();
        }
        let assets = HighlightingAssets::from_binary();
        let reader = Box::new(self.script.as_bytes());
        let inputs = vec![BatInput::from_reader(reader)];
        let controller = Controller::new(&config, &assets);
        Ok(controller.run(inputs, Some(&mut f))?)
    }
}

#[cfg(test)]
mod tests {
    use std::io::empty;

    use crate::{Error, Evaluation, LuaSource};

    #[test]
    fn runtime_error() {
        let source: LuaSource = "return nil+1".into();
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
