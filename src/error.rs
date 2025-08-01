use lazy_regex::*;
use miette::{GraphicalReportHandler, GraphicalTheme, LabeledSpan, NamedSource, Report, miette};
use mlua::{AsChunk, prelude::*};
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

/// Writes an error message to a string, extracting the line number and message from the Lua source.
pub fn build_report<S>(source: S, error: &LmbError) -> LmbResult<ErrorReport>
where
    S: AsChunk,
{
    let name = source.name().unwrap_or_default();
    let source = source.source()?;
    let Some(source) = std::str::from_utf8(&source).ok().map(|s| s.to_string()) else {
        return Ok(ErrorReport::String(error.to_string()));
    };
    let message = match &error {
        LmbError::Lua(LuaError::RuntimeError(message) | LuaError::SyntaxError { message, .. }) => {
            message
        }
        _ => return Ok(ErrorReport::String(error.to_string())),
    };
    let Some(first_line) = message.lines().next() else {
        return Ok(ErrorReport::String(error.to_string()));
    };
    let (line_number, message) = if let Some(caps) = ERROR_MESSAGE.captures(first_line) {
        let Some(line_number) = caps
            .get(1)
            .map(|m| m.as_str())
            .and_then(|s| s.parse::<usize>().ok())
        else {
            return Ok(ErrorReport::String(error.to_string()));
        };
        let Some(message) = caps.get(2).map(|m| m.as_str()) else {
            return Ok(ErrorReport::String(error.to_string()));
        };
        (line_number, message)
    } else {
        return Ok(ErrorReport::String(error.to_string()));
    };

    let offsets: StringOffsets = StringOffsets::new(&source);
    let range = offsets.line_to_chars(line_number - 1);

    let source = NamedSource::new(name, source).with_language("lua");
    let report = miette!(labels = vec![LabeledSpan::at(range, message)], "{message}")
        .with_source_code(source);
    Ok(ErrorReport::Report(report))
}

/// Renders a report to a string using the graphical report handler.
pub fn render_report<W>(writer: &mut W, report: &Report)
where
    W: std::fmt::Write,
{
    GraphicalReportHandler::new_themed(GraphicalTheme::ascii())
        .render_report(writer, report.as_ref())
        .expect("Failed to render report");
}
