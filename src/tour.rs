//! Guided tour display module for lmb CLI.
//!
//! This module provides functionality to display the guided-tour.md documentation
//! directly in the terminal with syntax highlighting and proper formatting.

use std::io::{self, Write};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
    util::as_24_bit_terminal_escaped,
};
use termimad::{MadSkin, crossterm::style::Color};

/// Embedded guided tour documentation
const GUIDED_TOUR: &str = include_str!("../docs/guided-tour.md");

/// Color mode for terminal output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit RGB true color (default for modern terminals)
    TrueColor,
    /// No color output
    None,
}

impl ColorMode {
    /// Detect color mode based on environment
    pub fn detect(no_color: bool) -> Self {
        if no_color || std::env::var("NO_COLOR").is_ok() {
            ColorMode::None
        } else {
            ColorMode::TrueColor
        }
    }
}

/// Section information extracted from the markdown
#[derive(Debug, Clone)]
pub struct Section {
    /// Section title (heading text)
    pub title: String,
    /// Heading level (1-6)
    pub level: u8,
    /// Start byte offset in the source
    pub start: usize,
    /// End byte offset in the source
    pub end: usize,
}

/// Extract all sections from the guided tour markdown
fn extract_sections(source: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let parser = Parser::new_ext(source, Options::all());

    let mut current_heading: Option<(String, u8, usize)> = None;
    let mut in_heading = false;
    let mut heading_text = String::new();
    let mut heading_level = 0u8;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Close previous section
                if let Some((title, lvl, start)) = current_heading.take() {
                    sections.push(Section {
                        title,
                        level: lvl,
                        start,
                        end: range.start,
                    });
                }
                in_heading = true;
                heading_level = level as u8;
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                current_heading = Some((heading_text.clone(), heading_level, range.start));
            }
            Event::Text(text) if in_heading => {
                heading_text.push_str(&text);
            }
            _ => {}
        }
    }

    // Close last section
    if let Some((title, level, start)) = current_heading {
        sections.push(Section {
            title,
            level,
            start,
            end: source.len(),
        });
    }

    sections
}

/// List all available sections
pub fn list_sections() -> Vec<Section> {
    extract_sections(GUIDED_TOUR)
}

/// Find a section by name (case-insensitive partial match)
pub fn find_section(name: &str) -> Option<(Section, String)> {
    let sections = extract_sections(GUIDED_TOUR);
    let name_lower = name.to_lowercase();

    // First try exact match
    for section in &sections {
        if section.title.to_lowercase() == name_lower {
            let content = &GUIDED_TOUR[section.start..section.end];
            return Some((
                section.clone(),
                format!("# {}\n\n{}", section.title, content),
            ));
        }
    }

    // Then try partial match
    for section in &sections {
        if section.title.to_lowercase().contains(&name_lower) {
            let content = &GUIDED_TOUR[section.start..section.end];
            return Some((
                section.clone(),
                format!("# {}\n\n{}", section.title, content),
            ));
        }
    }

    None
}

/// Render markdown with syntax highlighting for code blocks
pub fn render(content: &str, color_mode: ColorMode) -> anyhow::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    if color_mode == ColorMode::None {
        // Plain text output without colors
        render_plain(&mut handle, content)?;
    } else {
        // Colored output with syntax highlighting
        render_colored(&mut handle, content)?;
    }

    Ok(())
}

/// Render plain text without colors
fn render_plain<W: Write>(writer: &mut W, content: &str) -> io::Result<()> {
    let parser = Parser::new_ext(content, Options::all());
    let mut in_code_block = false;
    let mut code_content = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_content.clear();
                writeln!(writer)?;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                // Indent code block
                for line in code_content.lines() {
                    writeln!(writer, "    {line}")?;
                }
                writeln!(writer)?;
            }
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else {
                    write!(writer, "{text}")?;
                }
            }
            Event::Code(code) => {
                write!(writer, "`{code}`")?;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                writeln!(writer)?;
                for _ in 0..level as usize {
                    write!(writer, "#")?;
                }
                write!(writer, " ")?;
            }
            Event::End(TagEnd::Heading(_) | TagEnd::Paragraph | TagEnd::List(_) | TagEnd::Item)
            | Event::SoftBreak
            | Event::HardBreak => {
                writeln!(writer)?;
            }
            Event::Start(Tag::Item) => {
                write!(writer, "  - ")?;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                write!(writer, "[")?;
                // Store URL for later
                let _ = dest_url;
            }
            Event::End(TagEnd::Link) => {
                write!(writer, "]")?;
            }
            _ => {}
        }
    }

    writer.flush()
}

/// Render with colors and syntax highlighting
fn render_colored<W: Write>(writer: &mut W, content: &str) -> io::Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    let parser = Parser::new_ext(content, Options::all());
    let mut in_code_block = false;
    let mut code_content = String::new();
    let mut code_lang = String::new();

    let _skin = create_skin();

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_content.clear();
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                writeln!(writer)?;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;

                // Determine syntax for highlighting
                let syntax = if code_lang == "luau" || code_lang == "lua" {
                    ss.find_syntax_by_extension("lua")
                } else if code_lang.is_empty() {
                    None
                } else {
                    ss.find_syntax_by_extension(&code_lang)
                        .or_else(|| ss.find_syntax_by_name(&code_lang))
                };

                if let Some(syntax) = syntax {
                    let mut highlighter = HighlightLines::new(syntax, theme);
                    for line in code_content.lines() {
                        let ranges: Vec<(Style, &str)> =
                            highlighter.highlight_line(line, &ss).unwrap_or_default();
                        let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                        writeln!(writer, "    {escaped}\x1b[0m")?;
                    }
                } else {
                    // No syntax found, render as plain code
                    for line in code_content.lines() {
                        writeln!(writer, "    {line}")?;
                    }
                }
                writeln!(writer)?;
            }
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else {
                    write!(writer, "{text}")?;
                }
            }
            Event::Code(code) => {
                // Inline code with background
                write!(writer, "\x1b[48;5;238m{code}\x1b[0m")?;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                writeln!(writer)?;
                // Color headings based on level
                let color_code = match level {
                    pulldown_cmark::HeadingLevel::H1 => "\x1b[1;36m", // Bold cyan
                    pulldown_cmark::HeadingLevel::H2 => "\x1b[1;33m", // Bold yellow
                    pulldown_cmark::HeadingLevel::H3 => "\x1b[1;32m", // Bold green
                    _ => "\x1b[1;37m",                                // Bold white
                };
                write!(writer, "{color_code}")?;
                for _ in 0..level as usize {
                    write!(writer, "#")?;
                }
                write!(writer, " ")?;
            }
            Event::End(TagEnd::Heading(_)) => {
                writeln!(writer, "\x1b[0m")?;
            }
            Event::End(TagEnd::Paragraph | TagEnd::List(_) | TagEnd::Item)
            | Event::SoftBreak
            | Event::HardBreak => {
                writeln!(writer)?;
            }
            Event::Start(Tag::Item) => {
                write!(writer, "  \x1b[33m-\x1b[0m ")?;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                write!(writer, "\x1b[4;34m")?; // Underline blue
                let _ = dest_url;
            }
            Event::End(TagEnd::Link | TagEnd::Strong | TagEnd::Emphasis) => {
                write!(writer, "\x1b[0m")?;
            }
            Event::Start(Tag::Strong) => {
                write!(writer, "\x1b[1m")?; // Bold
            }
            Event::Start(Tag::Emphasis) => {
                write!(writer, "\x1b[3m")?; // Italic
            }
            _ => {}
        }
    }

    writer.flush()
}

/// Create a termimad skin for rendering (kept for potential future use)
fn create_skin() -> MadSkin {
    let mut skin = MadSkin::default();
    skin.headers[0].set_fg(Color::Cyan);
    skin.headers[1].set_fg(Color::Yellow);
    skin.headers[2].set_fg(Color::Green);
    skin.bold.set_fg(Color::White);
    skin.italic.set_fg(Color::Magenta);
    skin.code_block.set_bg(Color::AnsiValue(235));
    skin
}

/// Display the full guided tour
pub fn display_tour(color_mode: ColorMode) -> anyhow::Result<()> {
    render(GUIDED_TOUR, color_mode)
}

/// Display a specific section of the guided tour
pub fn display_section(section_name: &str, color_mode: ColorMode) -> anyhow::Result<()> {
    if let Some((_, content)) = find_section(section_name) {
        render(&content, color_mode)
    } else {
        anyhow::bail!(
            "Section '{}' not found. Use 'lmb tour --list' to see available sections.",
            section_name
        )
    }
}

/// Print the list of available sections
pub fn print_section_list() {
    let sections = list_sections();

    println!("Available sections in the guided tour:\n");

    for section in sections {
        let indent = "  ".repeat((section.level - 1) as usize);
        println!("{indent}{}", section.title);
    }

    println!("\nUse 'lmb tour -s <section>' to view a specific section.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_sections() {
        let sections = extract_sections(GUIDED_TOUR);
        assert!(!sections.is_empty());
        assert_eq!(sections[0].title, "Guided tour");
    }

    #[test]
    fn test_extract_sections_custom_markdown() {
        let markdown = r#"# First Heading

Some content here.

## Second Heading

More content.

### Third Heading

Even more content.
"#;
        let sections = extract_sections(markdown);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].title, "First Heading");
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[1].title, "Second Heading");
        assert_eq!(sections[1].level, 2);
        assert_eq!(sections[2].title, "Third Heading");
        assert_eq!(sections[2].level, 3);
    }

    #[test]
    fn test_extract_sections_empty_markdown() {
        let sections = extract_sections("");
        assert!(sections.is_empty());
    }

    #[test]
    fn test_extract_sections_no_headings() {
        let markdown = "Just some plain text without any headings.";
        let sections = extract_sections(markdown);
        assert!(sections.is_empty());
    }

    #[test]
    fn test_find_section() {
        let result = find_section("hello");
        assert!(result.is_some());
        let (section, _) = result.expect("Section should be found");
        assert!(section.title.to_lowercase().contains("hello"));
    }

    #[test]
    fn test_find_section_crypto() {
        let result = find_section("crypto");
        assert!(result.is_some());
    }

    #[test]
    fn test_find_section_not_found() {
        let result = find_section("nonexistent_section_xyz_12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_section_exact_match() {
        let result = find_section("Guided tour");
        assert!(result.is_some());
        let (section, content) = result.unwrap();
        assert_eq!(section.title, "Guided tour");
        assert!(content.contains("# Guided tour"));
    }

    #[test]
    fn test_find_section_case_insensitive() {
        let result = find_section("GUIDED TOUR");
        assert!(result.is_some());
        let (section, _) = result.unwrap();
        assert_eq!(section.title, "Guided tour");
    }

    #[test]
    fn test_list_sections() {
        let sections = list_sections();
        assert!(!sections.is_empty());
        // First section should be "Guided tour"
        assert_eq!(sections[0].title, "Guided tour");
        assert_eq!(sections[0].level, 1);
    }

    #[test]
    fn test_color_mode_detect() {
        assert_eq!(ColorMode::detect(true), ColorMode::None);
    }

    #[test]
    fn test_color_mode_detect_no_flag() {
        // When no_color is false, result depends on NO_COLOR env var
        let result = ColorMode::detect(false);
        if std::env::var("NO_COLOR").is_ok() {
            assert_eq!(result, ColorMode::None);
        } else {
            assert_eq!(result, ColorMode::TrueColor);
        }
    }

    #[test]
    fn test_color_mode_equality() {
        assert_eq!(ColorMode::TrueColor, ColorMode::TrueColor);
        assert_eq!(ColorMode::None, ColorMode::None);
        assert_ne!(ColorMode::TrueColor, ColorMode::None);
    }

    #[test]
    fn test_color_mode_clone() {
        let mode = ColorMode::TrueColor;
        let cloned = mode;
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_color_mode_debug() {
        let mode = ColorMode::TrueColor;
        let debug_str = format!("{:?}", mode);
        assert_eq!(debug_str, "TrueColor");
    }

    #[test]
    fn test_section_clone() {
        let section = Section {
            title: "Test".to_string(),
            level: 1,
            start: 0,
            end: 10,
        };
        let cloned = section.clone();
        assert_eq!(section.title, cloned.title);
        assert_eq!(section.level, cloned.level);
        assert_eq!(section.start, cloned.start);
        assert_eq!(section.end, cloned.end);
    }

    #[test]
    fn test_section_debug() {
        let section = Section {
            title: "Test".to_string(),
            level: 1,
            start: 0,
            end: 10,
        };
        let debug_str = format!("{:?}", section);
        assert!(debug_str.contains("Test"));
        assert!(debug_str.contains("level: 1"));
    }

    #[test]
    fn test_render_plain_basic() {
        let content = "# Hello\n\nThis is a test.";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("# Hello"));
        assert!(result.contains("This is a test."));
    }

    #[test]
    fn test_render_plain_with_code_block() {
        let content = "```rust\nlet x = 1;\n```";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_render_plain_with_inline_code() {
        let content = "Use `code` here.";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("`code`"));
    }

    #[test]
    fn test_render_plain_with_list() {
        let content = "- Item 1\n- Item 2";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("- Item 1"));
        assert!(result.contains("- Item 2"));
    }

    #[test]
    fn test_render_plain_with_headings() {
        let content = "# H1\n## H2\n### H3";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("# H1"));
        assert!(result.contains("## H2"));
        assert!(result.contains("### H3"));
    }

    #[test]
    fn test_render_plain_with_link() {
        let content = "[link text](https://example.com)";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("[link text]"));
    }

    #[test]
    fn test_render_colored_basic() {
        let content = "# Hello\n\nThis is a test.";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("Hello"));
        assert!(result.contains("This is a test."));
        // Should contain ANSI escape codes
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn test_render_colored_with_code_block_lua() {
        let content = "```lua\nlocal x = 1\n```";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("local"));
        assert!(result.contains("1"));
    }

    #[test]
    fn test_render_colored_with_code_block_luau() {
        let content = "```luau\nlocal x = 1\n```";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("local"));
    }

    #[test]
    fn test_render_colored_with_code_block_unknown_lang() {
        let content = "```unknown_lang\nsome code\n```";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("some code"));
    }

    #[test]
    fn test_render_colored_with_code_block_no_lang() {
        let content = "```\nplain code\n```";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("plain code"));
    }

    #[test]
    fn test_render_colored_with_inline_code() {
        let content = "Use `code` here.";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("code"));
        // Should have background color escape code
        assert!(result.contains("\x1b[48;5;238m"));
    }

    #[test]
    fn test_render_colored_headings() {
        let content = "# H1\n## H2\n### H3\n#### H4";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        // Check for different heading colors
        assert!(result.contains("\x1b[1;36m")); // H1 cyan
        assert!(result.contains("\x1b[1;33m")); // H2 yellow
        assert!(result.contains("\x1b[1;32m")); // H3 green
        assert!(result.contains("\x1b[1;37m")); // H4 white
    }

    #[test]
    fn test_render_colored_with_list() {
        let content = "- Item 1\n- Item 2";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("Item 1"));
        assert!(result.contains("Item 2"));
        // Should have colored bullet
        assert!(result.contains("\x1b[33m-\x1b[0m"));
    }

    #[test]
    fn test_render_colored_with_link() {
        let content = "[link text](https://example.com)";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("link text"));
        // Should have underline blue color
        assert!(result.contains("\x1b[4;34m"));
    }

    #[test]
    fn test_render_colored_with_bold() {
        let content = "**bold text**";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("bold text"));
        // Should have bold escape code
        assert!(result.contains("\x1b[1m"));
    }

    #[test]
    fn test_render_colored_with_italic() {
        let content = "*italic text*";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("italic text"));
        // Should have italic escape code
        assert!(result.contains("\x1b[3m"));
    }

    #[test]
    fn test_create_skin() {
        let skin = create_skin();
        // Just verify it doesn't panic and returns a valid skin
        assert!(!skin.headers.is_empty());
    }

    #[test]
    fn test_display_section_not_found() {
        let result = display_section("nonexistent_section_xyz_12345", ColorMode::None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_render_none_mode() {
        // Test the render function with ColorMode::None
        // This will write to stdout, so we just verify it doesn't panic
        let content = "# Test\n\nSimple content.";
        let result = render(content, ColorMode::None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_truecolor_mode() {
        // Test the render function with ColorMode::TrueColor
        let content = "# Test\n\nSimple content.";
        let result = render(content, ColorMode::TrueColor);
        assert!(result.is_ok());
    }

    #[test]
    fn test_display_tour_no_color() {
        // Just verify display_tour doesn't panic
        let result = display_tour(ColorMode::None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_plain_with_soft_break() {
        let content = "Line 1\nLine 2";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
    }

    #[test]
    fn test_render_colored_with_soft_break() {
        let content = "Line 1\nLine 2";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
    }

    #[test]
    fn test_render_plain_multiline_code() {
        let content = "```\nline1\nline2\nline3\n```";
        let mut output = Vec::new();
        render_plain(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
    }

    #[test]
    fn test_render_colored_multiline_code() {
        let content = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let mut output = Vec::new();
        render_colored(&mut output, content).unwrap();
        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("fn"));
        assert!(result.contains("main"));
    }
}
