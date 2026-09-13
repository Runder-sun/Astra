use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::fmt::Write as _;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

/// Stream-safe terminal Markdown renderer for the inline REPL.
///
/// Adapted from the MIT-licensed Claw-Code Rust renderer and reduced to the
/// subset Astra needs immediately: readable headings/lists/tables, fenced code
/// boxes, syntax highlighting, and streaming boundaries that do not render half
/// a code fence.
#[derive(Debug)]
pub struct TerminalMarkdownRenderer {
    syntax_set: SyntaxSet,
    syntax_theme: Theme,
}

impl TerminalMarkdownRenderer {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let syntax_theme = ThemeSet::load_defaults()
            .themes
            .remove("base16-ocean.dark")
            .unwrap_or_default();
        Self {
            syntax_set,
            syntax_theme,
        }
    }

    pub fn render(&self, markdown: &str) -> String {
        let markdown = render_pipe_tables_as_plain(markdown);
        let mut output = String::new();
        let mut state = RenderState::default();
        let mut code_language = String::new();
        let mut code_buffer = String::new();
        let mut table = TableBuffer::default();
        let mut in_code = false;

        for event in Parser::new_ext(&markdown, Options::all()) {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    state.heading = Some(level as u8);
                }
                Event::End(TagEnd::Heading(..)) => {
                    state.heading = None;
                    output.push_str("\n\n");
                }
                Event::Start(Tag::List(first)) => {
                    state
                        .lists
                        .push(first.map_or(ListKind::Unordered, |next| ListKind::Ordered { next }));
                }
                Event::End(TagEnd::List(..)) => {
                    state.lists.pop();
                    output.push('\n');
                }
                Event::Start(Tag::Item) => {
                    let indent = "  ".repeat(state.lists.len().saturating_sub(1));
                    match state.lists.last_mut() {
                        Some(ListKind::Ordered { next }) => {
                            let current = *next;
                            *next += 1;
                            let _ = write!(output, "{indent}{current}. ");
                        }
                        _ => {
                            let _ = write!(output, "{indent}• ");
                        }
                    }
                }
                Event::End(TagEnd::Item) | Event::SoftBreak | Event::HardBreak => {
                    output.push('\n');
                }
                Event::Start(Tag::BlockQuote(..)) => {
                    state.quote += 1;
                    output.push_str("\x1b[2m│ ");
                }
                Event::End(TagEnd::BlockQuote(..)) => {
                    state.quote = state.quote.saturating_sub(1);
                    output.push_str("\x1b[0m\n");
                }
                Event::Start(Tag::Strong) => state.strong += 1,
                Event::End(TagEnd::Strong) => state.strong = state.strong.saturating_sub(1),
                Event::Start(Tag::Emphasis) => state.emphasis += 1,
                Event::End(TagEnd::Emphasis) => state.emphasis = state.emphasis.saturating_sub(1),
                Event::Start(Tag::CodeBlock(kind)) => {
                    in_code = true;
                    code_language = match kind {
                        CodeBlockKind::Fenced(lang) => lang.to_string(),
                        CodeBlockKind::Indented => "text".to_string(),
                    };
                    code_buffer.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    output.push_str(&self.render_code_block(&code_language, &code_buffer));
                    in_code = false;
                    code_language.clear();
                    code_buffer.clear();
                }
                Event::Text(text) => {
                    if in_code {
                        code_buffer.push_str(&text);
                    } else if table.active {
                        table.push_text(&text);
                    } else {
                        output.push_str(&state.style_text(&text));
                    }
                }
                Event::Code(code) => {
                    let _ = write!(output, "\x1b[32m`{code}`\x1b[0m");
                }
                Event::Start(Tag::Table(_)) => {
                    table.active = true;
                }
                Event::End(TagEnd::Table) => {
                    output.push_str(&table.render());
                    table = TableBuffer::default();
                }
                Event::Start(Tag::TableHead) => table.in_head = true,
                Event::End(TagEnd::TableHead) => table.in_head = false,
                Event::Start(Tag::TableRow) => table.start_row(),
                Event::End(TagEnd::TableRow) => table.finish_row(),
                Event::Start(Tag::TableCell) => table.start_cell(),
                Event::End(TagEnd::TableCell) => table.finish_cell(),
                _ => {}
            }
        }

        output.trim_end().to_string()
    }

    fn render_code_block(&self, language: &str, code: &str) -> String {
        let language = if language.trim().is_empty() {
            "text"
        } else {
            language.trim()
        };
        let mut output = format!("\n\x1b[38;5;245m╭─ {language}\x1b[0m\n");
        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.syntax_theme);
        for line in LinesWithEndings::from(code) {
            match highlighter.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    output.push_str(&as_24_bit_terminal_escaped(&ranges[..], false));
                }
                Err(_) => output.push_str(line),
            }
        }
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("\x1b[38;5;245m╰────\x1b[0m\n");
        output
    }
}

impl Default for TerminalMarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct MarkdownStreamState {
    pending: String,
}

impl MarkdownStreamState {
    pub fn push(&mut self, renderer: &TerminalMarkdownRenderer, delta: &str) -> Option<String> {
        self.pending.push_str(delta);
        let boundary = stream_safe_boundary(&self.pending)?;
        let ready: String = self.pending.drain(..boundary).collect();
        Some(renderer.render(&ready))
    }

    pub fn finish(&mut self, renderer: &TerminalMarkdownRenderer) -> Option<String> {
        if self.pending.trim().is_empty() {
            self.pending.clear();
            return None;
        }
        let pending = std::mem::take(&mut self.pending);
        Some(renderer.render(&pending))
    }
}

fn stream_safe_boundary(markdown: &str) -> Option<usize> {
    if markdown.is_empty() || fence_is_open(markdown) {
        return None;
    }
    if markdown.ends_with('\n') {
        return Some(markdown.len());
    }
    markdown.rfind("\n\n").map(|index| index + 2)
}

fn fence_is_open(markdown: &str) -> bool {
    let mut open = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            open = !open;
        }
    }
    open
}

fn render_pipe_tables_as_plain(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut output = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if index + 1 < lines.len()
            && is_pipe_table_row(lines[index])
            && is_table_separator(lines[index + 1])
        {
            let headers = parse_pipe_row(lines[index]);
            let mut rows = Vec::new();
            index += 2;
            while index < lines.len() && is_pipe_table_row(lines[index]) {
                rows.push(parse_pipe_row(lines[index]));
                index += 1;
            }
            output.extend(format_plain_table(&headers, &rows));
            continue;
        }
        output.push(lines[index].to_string());
        index += 1;
    }
    let mut rendered = output.join("\n");
    if markdown.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn is_pipe_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.matches('|').count() >= 2
}

fn is_table_separator(line: &str) -> bool {
    if !is_pipe_table_row(line) {
        return false;
    }
    parse_pipe_row(line).into_iter().all(|cell| {
        let cell = cell.trim();
        !cell.is_empty()
            && cell.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
            && cell.contains('-')
    })
}

fn parse_pipe_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn format_plain_table(headers: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let columns = headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let mut widths = vec![0usize; columns];
    for (index, value) in headers.iter().enumerate() {
        widths[index] = widths[index].max(value.chars().count());
    }
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.chars().count());
        }
    }
    let mut output = Vec::new();
    output.push(format_table_row(headers, &widths, true));
    for row in rows {
        output.push(format_table_row(row, &widths, false));
    }
    output
}

#[derive(Debug, Default)]
struct RenderState {
    heading: Option<u8>,
    strong: usize,
    emphasis: usize,
    quote: usize,
    lists: Vec<ListKind>,
}

#[derive(Debug)]
enum ListKind {
    Unordered,
    Ordered { next: u64 },
}

impl RenderState {
    fn style_text(&self, text: &str) -> String {
        if self.heading.is_some() {
            return format!("\x1b[1;36m{text}\x1b[0m");
        }
        if self.strong > 0 {
            return format!("\x1b[1;33m{text}\x1b[0m");
        }
        if self.emphasis > 0 {
            return format!("\x1b[35m{text}\x1b[0m");
        }
        if self.quote > 0 {
            return format!("\x1b[2m{text}\x1b[0m");
        }
        text.to_string()
    }
}

#[derive(Debug, Default)]
struct TableBuffer {
    active: bool,
    in_head: bool,
    current_cell: String,
    current_row: Vec<String>,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl TableBuffer {
    fn start_row(&mut self) {
        self.current_row.clear();
    }

    fn finish_row(&mut self) {
        if self.current_row.is_empty() {
            return;
        }
        if self.in_head {
            self.headers = std::mem::take(&mut self.current_row);
        } else {
            self.rows.push(std::mem::take(&mut self.current_row));
        }
    }

    fn start_cell(&mut self) {
        self.current_cell.clear();
    }

    fn push_text(&mut self, text: &str) {
        self.current_cell.push_str(text);
    }

    fn finish_cell(&mut self) {
        self.current_row.push(self.current_cell.trim().to_string());
        self.current_cell.clear();
    }

    fn render(&self) -> String {
        let mut output = String::new();
        let columns = self
            .headers
            .len()
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0));
        if columns == 0 {
            return output;
        }
        let mut widths = vec![0usize; columns];
        for (index, value) in self.headers.iter().enumerate() {
            widths[index] = widths[index].max(value.chars().count());
        }
        for row in &self.rows {
            for (index, value) in row.iter().enumerate() {
                widths[index] = widths[index].max(value.chars().count());
            }
        }
        output.push('\n');
        if !self.headers.is_empty() {
            output.push_str(&format_table_row(&self.headers, &widths, true));
            output.push('\n');
        }
        for row in &self.rows {
            output.push_str(&format_table_row(row, &widths, false));
            output.push('\n');
        }
        output
    }
}

fn format_table_row(row: &[String], widths: &[usize], header: bool) -> String {
    let mut cells = Vec::new();
    for (index, width) in widths.iter().enumerate() {
        let value = row.get(index).map(String::as_str).unwrap_or("");
        cells.push(format!("{value:<width$}"));
    }
    if header {
        format!("\x1b[36m{}\x1b[0m", cells.join(" │ "))
    } else {
        cells.join(" │ ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(input: &str) -> String {
        let mut output = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                output.push(ch);
            }
        }
        output
    }

    #[test]
    fn markdown_renderer_preserves_code_tables_and_lists_as_structured_terminal_text() {
        let renderer = TerminalMarkdownRenderer::new();
        let rendered = renderer.render(
            "# Plan\n\n- test\n\n| File | State |\n| --- | --- |\n| src/lib.rs | ok |\n\n```rust\nfn main() {}\n```",
        );
        let plain = strip_ansi(&rendered);

        assert!(plain.contains("Plan"));
        assert!(plain.contains("• test"));
        assert!(plain.contains("File"));
        assert!(plain.contains("src/lib.rs"));
        assert!(plain.contains("╭─ rust"));
        assert!(plain.contains("fn main()"));
        assert!(rendered.contains('\u{1b}'));
    }

    #[test]
    fn markdown_stream_state_waits_until_safe_boundary_before_rendering_code_fence() {
        let renderer = TerminalMarkdownRenderer::new();
        let mut stream = MarkdownStreamState::default();

        assert!(stream.push(&renderer, "```rust\nfn main()").is_none());
        assert!(stream.push(&renderer, " {}\n").is_none());
        let rendered = stream
            .push(&renderer, "```\n\nDone\n")
            .expect("safe boundary");
        let plain = strip_ansi(&rendered);

        assert!(plain.contains("fn main()"));
        assert!(plain.contains("Done"));
        assert!(stream.finish(&renderer).is_none());
    }
}
