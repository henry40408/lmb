use bon::builder;
use lazy_regex::*;
use miette::{GraphicalReportHandler, GraphicalTheme, LabeledSpan, NamedSource, Report, miette};
use mlua::{AsChunk, prelude::*};
use no_color::is_no_color;
use string_offsets::StringOffsets;

use crate::{LmbError, LmbResult};

static ERROR_MESSAGE: Lazy<Regex> = lazy_regex!(r"^.*:([0-9]+):\s*(.*)$");

/// Represents an error report that can either be a simple string or a detailed report with source code.
#[derive(Debug)]
pub enum ErrorReport {
    /// A simple string representation of the error
    String(String),
    /// A report containing the error message and the source code
    Report(Report),
}

fn extract_line_number_from_traceback<'a>(name: &'a str, traceback: &'a str) -> Option<&'a str> {
    let name = name.trim_start_matches("@").trim_start_matches("=");
    traceback
        .lines()
        .find(|l| l.trim().starts_with(name))
        .map(|l| l.trim())
}

fn parse_message<'a>(
    name: &'a str,
    message: &'a str,
    traceback: Option<&'a String>,
) -> Option<(usize, &'a str)> {
    if let Some(traceback) = traceback {
        let line_with_line_number = extract_line_number_from_traceback(name, traceback)?;
        let captures = ERROR_MESSAGE.captures(line_with_line_number)?;
        let line_number = captures.get(1)?.as_str().parse::<usize>().ok()?;
        Some((line_number, message))
    } else {
        let captures = ERROR_MESSAGE.captures(message)?;
        let line_number = captures.get(1)?.as_str().parse::<usize>().ok()?;
        let message = captures.get(2)?.as_str();
        Some((line_number, message))
    }
}

/// Writes an error message to a string, extracting the line number and message from the Lua source.
#[builder]
pub fn build_report<S>(
    #[builder(start_fn)] source: S,
    #[builder(start_fn)] error: &LmbError,
    #[builder(into)] default_name: Option<String>,
) -> LmbResult<ErrorReport>
where
    S: AsChunk,
{
    let name = source
        .name()
        .unwrap_or_else(|| default_name.unwrap_or_else(|| "-".to_string()));
    let source = source.source()?;
    let Some(source) = std::str::from_utf8(&source).ok().map(|s| s.to_string()) else {
        return Ok(ErrorReport::String(error.to_string()));
    };
    let (message, traceback) = match &error {
        LmbError::Lua(LuaError::CallbackError { traceback, cause }) => {
            (&cause.to_string(), Some(traceback))
        }
        LmbError::Lua(LuaError::RuntimeError(message) | LuaError::SyntaxError { message, .. }) => {
            (message, None)
        }
        _ => return Ok(ErrorReport::String(error.to_string())),
    };
    let Some((line_number, message)) = parse_message(&name, message, traceback) else {
        return Ok(ErrorReport::String(error.to_string()));
    };

    let offsets: StringOffsets = StringOffsets::new(&source);
    let range = offsets.line_to_chars(line_number - 1);

    let source = NamedSource::new(&name, source).with_language("lua");
    let report = miette!(labels = vec![LabeledSpan::at(range, message)], "{message}")
        .with_source_code(source);
    Ok(ErrorReport::Report(report))
}

/// Renders a report to a string using the graphical report handler.
pub fn render_report<W>(writer: &mut W, report: &Report)
where
    W: std::fmt::Write,
{
    let theme = if is_no_color() {
        GraphicalTheme::none()
    } else {
        GraphicalTheme::ascii()
    };
    GraphicalReportHandler::new_themed(theme)
        .render_report(writer, report.as_ref())
        .expect("failed to render report");
}
