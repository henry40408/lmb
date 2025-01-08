use bon::bon;
use std::{io::Error as IoError, io::Write};

use ariadne::{CharSet, ColorGenerator, Config, Label, Report, ReportKind, Source};
use bon::Builder;

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
    pub fn check(&self) -> Result<full_moon::ast::Ast, Vec<full_moon::Error>> {
        full_moon::parse(self.script.as_ref())
    }

    /// Render an error from [`full_moon`] to a writer.
    ///
    /// # Errors
    ///
    /// This function will return an [`std::io::Error`]
    /// if there is an issue writing the error to the provided writer.
    #[builder]
    pub fn write_lua_errors<W>(
        &self,
        #[builder(start_fn)] mut f: W,
        #[builder(start_fn)] errors: Vec<full_moon::Error>,
        no_color: bool,
    ) -> Result<(), IoError>
    where
        W: Write,
    {
        let mut colors = ColorGenerator::new();
        let color = colors.next();
        let name = &self.name.as_deref().unwrap_or_default();

        let span = errors
            .iter()
            .min_by_key(|e| match e {
                full_moon::Error::AstError(e) => e.token().start_position().bytes(),
                full_moon::Error::TokenizerError(e) => e.position().bytes(),
            })
            .map(|e| match e {
                full_moon::Error::AstError(e) => {
                    let token = e.token();
                    token.start_position().bytes()..token.end_position().bytes()
                }
                full_moon::Error::TokenizerError(e) => e.position().bytes()..e.position().bytes(),
            });
        let mut report = Report::build(ReportKind::Error, (name, span.unwrap_or_else(|| 0..0)))
            .with_config(
                Config::default()
                    .with_char_set(CharSet::Ascii)
                    .with_compact(true)
                    .with_color(!no_color),
            );
        for error in errors {
            let (message, start, end) = match error {
                full_moon::Error::AstError(e) => (
                    e.error_message().to_string(),
                    e.token().start_position().bytes(),
                    e.token().end_position().bytes(),
                ),
                full_moon::Error::TokenizerError(e) => (
                    e.error().to_string(),
                    e.position().bytes(),
                    e.position().bytes() + 1,
                ),
            };
            let span = start..end;
            report = report
                .with_label(
                    Label::new((name, span))
                        .with_color(color)
                        .with_message(&message),
                )
                .with_message(&message);
        }
        report
            .finish()
            .write((name, Source::from(&self.script)), &mut f)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::LuaSource;

    #[test]
    fn syntax() {
        let script = "ret true";
        let lua_source = LuaSource::builder(script).build();
        assert!(matches!(
            lua_source.check().unwrap_err().get(0),
            Some(full_moon::Error::AstError { .. })
        ));

        let script = "return true";
        let lua_source = LuaSource::builder(script).build();
        assert!(lua_source.check().is_ok());
    }

    #[test]
    fn syntax_error() {
        let script = "ret true";
        let lua_source = LuaSource::builder(script).build();
        let errors = lua_source.check().unwrap_err();
        let mut buf = Vec::new();
        lua_source
            .write_lua_errors(&mut buf, errors)
            .no_color(true)
            .call()
            .unwrap();
    }
}
