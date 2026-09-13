#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiOutputKind {
    Text,
    Markdown,
    Tool,
    Terminal,
    Diff,
    Test,
    Error,
    Warning,
    Status,
    Artifact,
    ResearchEvidence,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiStructuredPayload {
    Tool {
        command: String,
        stdout: Vec<String>,
        stderr: Vec<String>,
        summary: String,
        duration: Option<String>,
    },
    Code {
        language: String,
        file_path: Option<String>,
        line_range: Option<String>,
        line_count: usize,
    },
    Diff {
        file_path: String,
        additions: usize,
        deletions: usize,
        hunks: usize,
    },
    Error {
        error_type: String,
        message: String,
        location: Option<String>,
        fix: Option<String>,
    },
    Test {
        pass_count: usize,
        total: usize,
        failures: Vec<String>,
        duration: Option<String>,
    },
    Warning {
        warning_type: String,
        message: String,
        location: Option<String>,
        suggestion: Option<String>,
    },
    Status {
        action: String,
        metrics: Vec<String>,
    },
    ResearchEvidence {
        topic: String,
        source: String,
        quote: String,
        confidence: String,
        citations: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiBlockStatus {
    Running,
    Passed,
    Failed,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiOutputBlock {
    pub kind: TuiOutputKind,
    pub title: String,
    pub status: TuiBlockStatus,
    pub lines: Vec<String>,
    pub fold: FoldState,
    pub payload: Option<TuiStructuredPayload>,
}

impl TuiOutputBlock {
    pub fn new(
        kind: TuiOutputKind,
        title: impl Into<String>,
        status: TuiBlockStatus,
        lines: Vec<String>,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            status,
            lines,
            fold: FoldState::Expanded,
            payload: None,
        }
    }

    pub fn visible_lines(&self) -> &[String] {
        match self.fold {
            FoldState::Expanded => &self.lines,
            FoldState::Folded { visible_lines, .. } => {
                let end = visible_lines.min(self.lines.len());
                &self.lines[..end]
            }
        }
    }

    fn with_payload(mut self, payload: Option<TuiStructuredPayload>) -> Self {
        self.payload = payload;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldState {
    Expanded,
    Folded {
        visible_lines: usize,
        hidden_count: usize,
        summary: String,
    },
}

impl FoldState {
    pub fn is_folded(&self) -> bool {
        matches!(self, Self::Folded { .. })
    }

    pub fn hidden_count(&self) -> usize {
        match self {
            Self::Expanded => 0,
            Self::Folded { hidden_count, .. } => *hidden_count,
        }
    }

    pub fn summary(&self) -> &str {
        match self {
            Self::Expanded => "",
            Self::Folded { summary, .. } => summary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldPolicy {
    Expanded,
    FoldLong { visible_lines: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventState {
    Running,
    Passed,
    Failed,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiOutputEvent {
    pub kind: TuiOutputKind,
    pub label: String,
    pub state: EventState,
    pub body: String,
}

impl TuiOutputEvent {
    pub fn new(
        kind: TuiOutputKind,
        label: impl Into<String>,
        state: EventState,
        body: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            state,
            body: body.into(),
        }
    }

    pub fn text(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Text, label, EventState::Info, body)
    }

    pub fn markdown(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Markdown, label, EventState::Info, body)
    }

    pub fn tool(label: impl Into<String>, state: EventState, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Tool, label, state, body)
    }

    pub fn terminal(label: impl Into<String>, state: EventState, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Terminal, label, state, body)
    }

    pub fn diff(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Diff, label, EventState::Info, body)
    }

    pub fn test(label: impl Into<String>, state: EventState, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Test, label, state, body)
    }

    pub fn error(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Error, label, EventState::Failed, body)
    }

    pub fn artifact(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Artifact, label, EventState::Info, body)
    }

    pub fn research_evidence(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(
            TuiOutputKind::ResearchEvidence,
            label,
            EventState::Info,
            body,
        )
    }

    pub fn warning(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Warning, label, EventState::Warn, body)
    }

    pub fn status(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Status, label, EventState::Info, body)
    }

    pub fn code(label: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(TuiOutputKind::Code, label, EventState::Info, body)
    }
}

pub fn blocks_from_events<I>(events: I, fold_policy: FoldPolicy) -> Vec<TuiOutputBlock>
where
    I: IntoIterator<Item = TuiOutputEvent>,
{
    events
        .into_iter()
        .map(|event| {
            let title = typed_title(event.kind, &event.label);
            let lines = split_event_body(&event.body);
            let payload = structured_payload_from_event(event.kind, &event.label, &lines);
            apply_fold(
                TuiOutputBlock::new(event.kind, title, event_state_status(event.state), lines)
                    .with_payload(payload),
                fold_policy,
            )
        })
        .collect()
}

pub fn blocks_from_legacy_text(body: &str, fold_policy: FoldPolicy) -> Vec<TuiOutputBlock> {
    let lines = body.lines().map(str::trim_end).collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }

        if let Some(language) = fence_language(line) {
            let start = index;
            index += 1;
            let mut fenced_lines = vec![lines[start].to_string()];
            while index < lines.len() {
                let next = lines[index];
                fenced_lines.push(next.to_string());
                if is_fence(next) {
                    index += 1;
                    break;
                }
                index += 1;
            }

            let kind = if is_diff_language(&language) {
                TuiOutputKind::Diff
            } else if is_markdown_language(&language) {
                TuiOutputKind::Markdown
            } else {
                TuiOutputKind::Code
            };
            let title = if kind == TuiOutputKind::Diff {
                "Diff".to_string()
            } else if kind == TuiOutputKind::Markdown {
                "Markdown".to_string()
            } else if language.is_empty() {
                "Code".to_string()
            } else {
                format!("Code {language}")
            };
            let payload = if kind == TuiOutputKind::Diff {
                Some(diff_payload(&fenced_lines))
            } else if kind == TuiOutputKind::Code {
                Some(code_payload(&language, &fenced_lines))
            } else {
                None
            };
            blocks.push(apply_fold(
                TuiOutputBlock::new(kind, title, TuiBlockStatus::Info, fenced_lines)
                    .with_payload(payload),
                fold_policy,
            ));
            continue;
        }

        if is_unified_diff_start(&lines, index) {
            let (diff_lines, next_index) = collect_unified_diff(&lines, index);
            let payload = diff_payload(&diff_lines);
            blocks.push(apply_fold(
                TuiOutputBlock::new(
                    TuiOutputKind::Diff,
                    "Diff",
                    TuiBlockStatus::Info,
                    diff_lines,
                )
                .with_payload(Some(payload)),
                fold_policy,
            ));
            index = next_index;
            continue;
        }

        if is_simple_diff_pair_start(&lines, index) {
            let (diff_lines, next_index) = collect_simple_diff_pair(&lines, index);
            let payload = diff_payload(&diff_lines);
            blocks.push(apply_fold(
                TuiOutputBlock::new(
                    TuiOutputKind::Diff,
                    "Diff",
                    TuiBlockStatus::Info,
                    diff_lines,
                )
                .with_payload(Some(payload)),
                fold_policy,
            ));
            index = next_index;
            continue;
        }

        let kind = classify_legacy_line(line);
        let mut block_lines = vec![line.to_string()];
        index += 1;
        while index < lines.len()
            && !lines[index].trim().is_empty()
            && fence_language(lines[index]).is_none()
            && !is_unified_diff_start(&lines, index)
            && classify_legacy_line(lines[index]) == kind
            && legacy_kind_allows_merge(kind)
        {
            block_lines.push(lines[index].to_string());
            index += 1;
        }

        let title = legacy_title(kind, block_lines.first().map(String::as_str).unwrap_or(""));
        let status = legacy_status(kind, &block_lines);
        let payload = structured_payload_from_legacy(kind, &title, status, &block_lines);
        blocks.push(apply_fold(
            TuiOutputBlock::new(kind, title, status, block_lines).with_payload(payload),
            fold_policy,
        ));
    }

    merge_adjacent_text_blocks(blocks)
}

fn structured_payload_from_event(
    kind: TuiOutputKind,
    label: &str,
    lines: &[String],
) -> Option<TuiStructuredPayload> {
    match kind {
        TuiOutputKind::Tool | TuiOutputKind::Terminal => Some(tool_payload(label, lines)),
        TuiOutputKind::Code => Some(code_payload(label, lines)),
        TuiOutputKind::Diff => Some(diff_payload(lines)),
        TuiOutputKind::Test => Some(test_payload(lines, None)),
        TuiOutputKind::Error => Some(error_payload(label, lines)),
        TuiOutputKind::Warning => Some(warning_payload(label, lines)),
        TuiOutputKind::Status => Some(status_payload(label, lines)),
        TuiOutputKind::ResearchEvidence => Some(research_payload(label, lines)),
        TuiOutputKind::Text | TuiOutputKind::Markdown | TuiOutputKind::Artifact => None,
    }
}

fn structured_payload_from_legacy(
    kind: TuiOutputKind,
    title: &str,
    status: TuiBlockStatus,
    lines: &[String],
) -> Option<TuiStructuredPayload> {
    match kind {
        TuiOutputKind::Tool | TuiOutputKind::Terminal => Some(tool_payload(title, lines)),
        TuiOutputKind::Diff => Some(diff_payload(lines)),
        TuiOutputKind::Test => Some(test_payload(lines, Some(status))),
        TuiOutputKind::Error => Some(error_payload(title, lines)),
        TuiOutputKind::Warning => Some(warning_payload(title, lines)),
        TuiOutputKind::Status => Some(status_payload(title, lines)),
        TuiOutputKind::ResearchEvidence => Some(research_payload(title, lines)),
        TuiOutputKind::Code => Some(code_payload(title, lines)),
        TuiOutputKind::Text | TuiOutputKind::Markdown | TuiOutputKind::Artifact => None,
    }
}

fn split_event_body(body: &str) -> Vec<String> {
    if body.is_empty() {
        Vec::new()
    } else {
        body.lines()
            .map(|line| line.trim_end().to_string())
            .collect()
    }
}

fn event_state_status(state: EventState) -> TuiBlockStatus {
    match state {
        EventState::Running => TuiBlockStatus::Running,
        EventState::Passed => TuiBlockStatus::Passed,
        EventState::Failed => TuiBlockStatus::Failed,
        EventState::Warn => TuiBlockStatus::Warn,
        EventState::Info => TuiBlockStatus::Info,
    }
}

fn typed_title(kind: TuiOutputKind, label: &str) -> String {
    let base = match kind {
        TuiOutputKind::Text => "Text",
        TuiOutputKind::Markdown => "Markdown",
        TuiOutputKind::Tool => "Tool",
        TuiOutputKind::Terminal => "Terminal",
        TuiOutputKind::Diff => "Diff",
        TuiOutputKind::Test => "Test",
        TuiOutputKind::Error => "Error",
        TuiOutputKind::Warning => "Warning",
        TuiOutputKind::Status => "Status",
        TuiOutputKind::Artifact => "Artifact",
        TuiOutputKind::ResearchEvidence => "Research Evidence",
        TuiOutputKind::Code => "Code",
    };
    if label.trim().is_empty() {
        base.to_string()
    } else {
        format!("{base}: {}", label.trim())
    }
}

fn tool_payload(label: &str, lines: &[String]) -> TuiStructuredPayload {
    let command = lines
        .iter()
        .find_map(|line| line.trim().strip_prefix("$ ").map(str::to_string))
        .or_else(|| lines.first().map(|line| clean_tool_command(line)))
        .unwrap_or_else(|| label.trim().to_string());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        if let Some(value) = trimmed.strip_prefix("stderr:") {
            stderr.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("STDERR:") {
            stderr.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("stdout:") {
            stdout.push(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("STDOUT:") {
            stdout.push(value.trim().to_string());
        } else if !trimmed.starts_with("$ ") {
            stdout.push(line.clone());
        }
    }
    TuiStructuredPayload::Tool {
        command,
        summary: first_non_empty(lines).unwrap_or_else(|| label.trim().to_string()),
        stdout,
        stderr,
        duration: duration_from_lines(lines),
    }
}

fn clean_tool_command(line: &str) -> String {
    line.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .strip_prefix("tool:")
        .or_else(|| line.trim().strip_prefix("Tool:"))
        .or_else(|| line.trim().strip_prefix("terminal:"))
        .or_else(|| line.trim().strip_prefix("Terminal:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| line.trim())
        .to_string()
}

fn code_payload(language_hint: &str, lines: &[String]) -> TuiStructuredPayload {
    let language = lines
        .first()
        .and_then(|line| fence_language(line))
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let value = language_hint
                .trim()
                .strip_prefix("Code")
                .unwrap_or(language_hint)
                .trim()
                .to_string();
            (!value.is_empty()).then_some(value)
        })
        .unwrap_or_else(|| "text".to_string());
    let line_count = code_content_lines(lines).len();
    TuiStructuredPayload::Code {
        language,
        file_path: file_path_from_lines(lines),
        line_range: line_range_from_lines(lines),
        line_count,
    }
}

fn diff_payload(lines: &[String]) -> TuiStructuredPayload {
    let additions = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('+') && !trimmed.starts_with("+++")
        })
        .count();
    let deletions = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('-') && !trimmed.starts_with("---")
        })
        .count();
    let hunks = lines
        .iter()
        .filter(|line| line.trim_start().starts_with("@@"))
        .count();
    TuiStructuredPayload::Diff {
        file_path: diff_file_path(lines).unwrap_or_else(|| "unknown".to_string()),
        additions,
        deletions,
        hunks,
    }
}

fn test_payload(lines: &[String], status: Option<TuiBlockStatus>) -> TuiStructuredPayload {
    let mut pass_count = 0usize;
    let mut total = 0usize;
    let mut failures = Vec::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("test ") && lower.ends_with(" ... ok") {
            pass_count += 1;
            total += 1;
        } else if lower.starts_with("test ") && lower.ends_with(" ... failed") {
            total += 1;
            failures.push(line.trim().to_string());
        } else if lower.starts_with("test result:") {
            let (summary_passed, summary_failed) = parse_test_result_counts(&lower);
            if summary_passed + summary_failed > 0 {
                pass_count = summary_passed;
                total = summary_passed + summary_failed;
            }
        }
    }
    if total == 0 && status == Some(TuiBlockStatus::Passed) && !lines.is_empty() {
        total = 1;
        pass_count = 1;
    }
    TuiStructuredPayload::Test {
        pass_count,
        total,
        failures,
        duration: duration_from_lines(lines),
    }
}

fn error_payload(label: &str, lines: &[String]) -> TuiStructuredPayload {
    TuiStructuredPayload::Error {
        error_type: label.trim().to_string(),
        message: first_non_empty(lines).unwrap_or_else(|| label.trim().to_string()),
        location: location_from_lines(lines),
        fix: fix_from_lines(lines),
    }
}

fn warning_payload(label: &str, lines: &[String]) -> TuiStructuredPayload {
    TuiStructuredPayload::Warning {
        warning_type: label.trim().to_string(),
        message: first_non_empty(lines).unwrap_or_else(|| label.trim().to_string()),
        location: location_from_lines(lines),
        suggestion: fix_from_lines(lines),
    }
}

fn status_payload(label: &str, lines: &[String]) -> TuiStructuredPayload {
    TuiStructuredPayload::Status {
        action: label.trim().to_string(),
        metrics: lines
            .iter()
            .flat_map(|line| line.split(" | ").map(str::trim).map(str::to_string))
            .filter(|part| !part.is_empty())
            .collect(),
    }
}

fn research_payload(label: &str, lines: &[String]) -> TuiStructuredPayload {
    TuiStructuredPayload::ResearchEvidence {
        topic: label.trim().to_string(),
        source: source_from_lines(lines).unwrap_or_else(|| "unknown".to_string()),
        quote: first_non_empty(lines).unwrap_or_default(),
        confidence: confidence_from_lines(lines).unwrap_or_else(|| "unknown".to_string()),
        citations: lines
            .iter()
            .filter(|line| line.to_ascii_lowercase().contains("citation"))
            .count(),
    }
}

fn first_non_empty(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn code_content_lines(lines: &[String]) -> Vec<&str> {
    let mut content = lines.iter().map(String::as_str).collect::<Vec<_>>();
    if content.first().map(|line| is_fence(line)).unwrap_or(false) {
        content.remove(0);
    }
    if content.last().map(|line| is_fence(line)).unwrap_or(false) {
        content.pop();
    }
    content
}

fn diff_file_path(lines: &[String]) -> Option<String> {
    for line in lines {
        let trimmed = line.trim_start();
        if let Some(path) = trimmed.strip_prefix("+++ b/") {
            return Some(path.trim().to_string());
        }
        if let Some(path) = trimmed.strip_prefix("--- a/") {
            return Some(path.trim().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("diff --git ") {
            return rest
                .split_whitespace()
                .nth(1)
                .map(|path| path.trim_start_matches("b/").to_string());
        }
    }
    None
}

fn file_path_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("file:")
            .or_else(|| trimmed.strip_prefix("File:"))
            .or_else(|| trimmed.strip_prefix("path:"))
            .or_else(|| trimmed.strip_prefix("Path:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn line_range_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("lines:")
            .or_else(|| trimmed.strip_prefix("Lines:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn location_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("-->")
            .or_else(|| trimmed.strip_prefix("at "))
            .or_else(|| trimmed.strip_prefix("location:"))
            .or_else(|| trimmed.strip_prefix("Location:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn fix_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("help:")
            .or_else(|| trimmed.strip_prefix("suggestion:"))
            .or_else(|| trimmed.strip_prefix("Suggestion:"))
            .or_else(|| trimmed.strip_prefix("fix:"))
            .or_else(|| trimmed.strip_prefix("Fix:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn source_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("source:")
            .or_else(|| trimmed.strip_prefix("Source:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn confidence_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("confidence:")
            .or_else(|| trimmed.strip_prefix("Confidence:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn duration_from_lines(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        lower
            .split_whitespace()
            .find(|token| is_duration_token(token))
            .map(str::to_string)
    })
}

fn is_duration_token(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| ch == ',' || ch == ';' || ch == '.');
    if let Some(number) = token.strip_suffix("ms") {
        return !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit());
    }
    if let Some(number) = token.strip_suffix('s') {
        return !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit() || ch == '.');
    }
    false
}

fn parse_test_result_counts(lower: &str) -> (usize, usize) {
    let parts = lower.split_whitespace().collect::<Vec<_>>();
    let mut passed = 0usize;
    let mut failed = 0usize;
    for window in parts.windows(2) {
        if let [number, label] = window {
            if let Ok(value) = number.trim_end_matches([',', ';', '.']).parse::<usize>() {
                match label.trim_end_matches([',', ';', '.']) {
                    "passed" | "ok" => passed = value,
                    "failed" => failed = value,
                    _ => {}
                }
            }
        }
    }
    (passed, failed)
}

fn apply_fold(mut block: TuiOutputBlock, fold_policy: FoldPolicy) -> TuiOutputBlock {
    match fold_policy {
        FoldPolicy::Expanded => block,
        FoldPolicy::FoldLong { visible_lines } => {
            if visible_lines > 0 && block.lines.len() > visible_lines {
                let hidden_count = block.lines.len() - visible_lines;
                block.fold = FoldState::Folded {
                    visible_lines,
                    hidden_count,
                    summary: format!(
                        "{}: {} visible, {} folded",
                        block.title, visible_lines, hidden_count
                    ),
                };
            }
            block
        }
    }
}

fn fence_language(line: &str) -> Option<String> {
    line.trim_start()
        .strip_prefix("```")
        .map(|language| language.trim().to_ascii_lowercase())
}

fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn is_diff_language(language: &str) -> bool {
    matches!(language, "diff" | "patch" | "udiff")
}

fn is_markdown_language(language: &str) -> bool {
    matches!(language, "md" | "markdown")
}

fn is_unified_diff_start(lines: &[&str], index: usize) -> bool {
    let current = lines[index].trim_start();
    if current.starts_with("diff --git ") {
        return true;
    }
    current.starts_with("--- ")
        && lines
            .get(index + 1)
            .map(|next| next.trim_start().starts_with("+++ "))
            .unwrap_or(false)
}

fn collect_unified_diff(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut collected = Vec::new();
    let mut index = start;
    while index < lines.len() {
        let line = lines[index];
        if index > start
            && line.trim().is_empty()
            && lines
                .get(index + 1)
                .map(|next| !is_diff_body_line(next.trim_start()))
                .unwrap_or(true)
        {
            break;
        }
        if index > start
            && !is_diff_body_line(line.trim_start())
            && !is_diff_continuation_context(&collected)
        {
            break;
        }
        collected.push(line.to_string());
        index += 1;
    }
    (collected, index)
}

fn is_simple_diff_pair_start(lines: &[&str], index: usize) -> bool {
    let current = lines[index].trim_start();
    if !(current.starts_with("+ ") || current.starts_with("- ")) {
        return false;
    }
    lines
        .get(index + 1)
        .map(|next| opposite_diff_delta(current, next.trim_start()))
        .unwrap_or(false)
        || index
            .checked_sub(1)
            .and_then(|previous| lines.get(previous))
            .map(|previous| opposite_diff_delta(current, previous.trim_start()))
            .unwrap_or(false)
}

fn collect_simple_diff_pair(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut collected = Vec::new();
    let mut index = start;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if !(trimmed.starts_with("+ ") || trimmed.starts_with("- ")) {
            break;
        }
        collected.push(lines[index].to_string());
        index += 1;
    }
    (collected, index)
}

fn opposite_diff_delta(current: &str, other: &str) -> bool {
    (current.starts_with("+ ") && other.starts_with("- "))
        || (current.starts_with("- ") && other.starts_with("+ "))
}

fn is_diff_body_line(trimmed: &str) -> bool {
    trimmed.starts_with("diff --git ")
        || trimmed.starts_with("index ")
        || trimmed.starts_with("--- ")
        || trimmed.starts_with("+++ ")
        || trimmed.starts_with("@@")
        || trimmed.starts_with('+')
        || trimmed.starts_with('-')
        || trimmed.starts_with(' ')
        || trimmed.starts_with('\\')
}

fn is_diff_continuation_context(collected: &[String]) -> bool {
    collected
        .last()
        .map(|line| line.trim_start().starts_with("@@"))
        .unwrap_or(false)
}

fn classify_legacy_line(line: &str) -> TuiOutputKind {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();

    if is_markdown_line(trimmed) {
        TuiOutputKind::Markdown
    } else if is_test_line(&lower) {
        TuiOutputKind::Test
    } else if is_error_line(&lower) {
        TuiOutputKind::Error
    } else if is_warning_line(&lower) {
        TuiOutputKind::Warning
    } else if is_research_evidence_line(trimmed, &lower) {
        TuiOutputKind::ResearchEvidence
    } else if is_status_line(trimmed, &lower) {
        TuiOutputKind::Status
    } else if is_command_line(trimmed, &lower) || is_terminal_label(trimmed) {
        TuiOutputKind::Terminal
    } else if is_tool_label(trimmed) {
        TuiOutputKind::Tool
    } else {
        TuiOutputKind::Text
    }
}

fn is_markdown_line(trimmed: &str) -> bool {
    trimmed.starts_with("#")
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("1. ")
}

fn is_test_line(lower: &str) -> bool {
    lower.starts_with("running ") && lower.ends_with(" tests")
        || lower.starts_with("test ")
            && (lower.ends_with(" ... ok") || lower.ends_with(" ... failed"))
        || lower.starts_with("test result:")
        || lower.starts_with("failures:")
}

fn is_error_line(lower: &str) -> bool {
    lower.starts_with("error:")
        || lower.starts_with("error[")
        || lower.starts_with("thread '") && lower.contains("' panicked at ")
}

fn is_warning_line(lower: &str) -> bool {
    lower.starts_with("warning:")
}

fn is_status_line(trimmed: &str, lower: &str) -> bool {
    lower.starts_with("session ") && trimmed.contains(" | ") && lower.contains("provider ")
        || lower.starts_with("permission ") && trimmed.contains(" | ")
}

fn is_research_evidence_line(trimmed: &str, lower: &str) -> bool {
    lower.starts_with("research evidence:")
        || lower.starts_with("research context:")
        || trimmed.starts_with("[research evidence:")
}

fn is_command_line(trimmed: &str, lower: &str) -> bool {
    lower.starts_with("cargo ")
        || lower.starts_with("running cargo ")
        || lower.starts_with("compiling ")
        || lower.starts_with("finished ")
        || lower.starts_with("run ")
        || trimmed.starts_with("$ ")
}

fn is_terminal_label(trimmed: &str) -> bool {
    trimmed.starts_with("$ ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("terminal:")
        || trimmed.starts_with("Terminal:")
}

fn is_tool_label(trimmed: &str) -> bool {
    trimmed.starts_with("tool:")
        || trimmed.starts_with("Tool:")
        || trimmed.starts_with("[tool:")
        || trimmed.starts_with("[Tool:")
}

fn legacy_title(kind: TuiOutputKind, first_line: &str) -> String {
    match kind {
        TuiOutputKind::Text => "Text".to_string(),
        TuiOutputKind::Markdown => "Markdown".to_string(),
        TuiOutputKind::Tool => title_with_label("Tool", first_line),
        TuiOutputKind::Terminal => title_with_label("Terminal", first_line),
        TuiOutputKind::Diff => "Diff".to_string(),
        TuiOutputKind::Test => "Test".to_string(),
        TuiOutputKind::Error => "Error".to_string(),
        TuiOutputKind::Warning => "Warning".to_string(),
        TuiOutputKind::Status => "Status".to_string(),
        TuiOutputKind::Artifact => "Artifact".to_string(),
        TuiOutputKind::ResearchEvidence => "Research Evidence".to_string(),
        TuiOutputKind::Code => "Code".to_string(),
    }
}

fn title_with_label(base: &str, first_line: &str) -> String {
    let label = first_line
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split_once(':')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or("");
    if label.is_empty() {
        base.to_string()
    } else {
        format!("{base}: {label}")
    }
}

fn legacy_status(kind: TuiOutputKind, lines: &[String]) -> TuiBlockStatus {
    match kind {
        TuiOutputKind::Error => TuiBlockStatus::Failed,
        TuiOutputKind::Warning => TuiBlockStatus::Warn,
        TuiOutputKind::Test => {
            let has_failed = lines
                .iter()
                .any(|line| line.to_ascii_lowercase().contains("failed"));
            if has_failed {
                TuiBlockStatus::Failed
            } else {
                TuiBlockStatus::Passed
            }
        }
        _ => TuiBlockStatus::Info,
    }
}

fn legacy_kind_allows_merge(kind: TuiOutputKind) -> bool {
    matches!(
        kind,
        TuiOutputKind::Text
            | TuiOutputKind::Markdown
            | TuiOutputKind::Test
            | TuiOutputKind::Error
            | TuiOutputKind::Warning
            | TuiOutputKind::Tool
            | TuiOutputKind::Terminal
    )
}

fn merge_adjacent_text_blocks(blocks: Vec<TuiOutputBlock>) -> Vec<TuiOutputBlock> {
    let mut merged: Vec<TuiOutputBlock> = Vec::new();
    for block in blocks {
        if let Some(previous) = merged.last_mut() {
            if previous.kind == block.kind
                && matches!(block.kind, TuiOutputKind::Text | TuiOutputKind::Markdown)
                && previous.fold == FoldState::Expanded
                && block.fold == FoldState::Expanded
            {
                previous.lines.extend(block.lines);
                continue;
            }
        }
        merged.push(block);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_text_keeps_plain_prose_as_text_even_with_trigger_words() {
        let body = "This warning appears in a design note, not a diagnostic.\n\
The session failed to include enough context in the paragraph.\n\
Provider logs and traces are mentioned as concepts.";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TuiOutputKind::Text);
        assert_eq!(blocks[0].status, TuiBlockStatus::Info);
        assert_eq!(blocks[0].lines.len(), 3);
    }

    #[test]
    fn legacy_text_keeps_explicit_command_status_and_small_diff_blocks() {
        let body =
            "cargo test\n+ added line\n- removed line\nsession s | turn t | provider fixture";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Terminal));
        assert!(blocks.iter().any(|block| block.kind == TuiOutputKind::Diff));
        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Status));
    }

    #[test]
    fn legacy_text_preserves_markdown_as_markdown_block() {
        let body = "# Notes\n- failed runs should be retried\n> warning is a quoted word";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TuiOutputKind::Markdown);
        assert_eq!(blocks[0].title, "Markdown");
    }

    #[test]
    fn legacy_text_recognizes_diff_fenced_blocks() {
        let body = "Patch:\n```diff\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n```\nDone.";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert!(blocks.iter().any(|block| block.kind == TuiOutputKind::Text));
        let diff = blocks
            .iter()
            .find(|block| block.kind == TuiOutputKind::Diff)
            .expect("diff fenced block");
        assert_eq!(diff.title, "Diff");
        assert!(diff
            .lines
            .first()
            .is_some_and(|line| line.starts_with("```")));
        assert!(diff.lines.iter().any(|line| line == "+new"));
    }

    #[test]
    fn legacy_text_groups_unified_diff() {
        let body = "diff --git a/a.txt b/a.txt\nindex 111..222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n old\n-removed\n+added";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TuiOutputKind::Diff);
        assert_eq!(blocks[0].lines.len(), 8);
    }

    #[test]
    fn legacy_text_recognizes_test_summary_without_catching_plain_failure_words() {
        let body = "running 3 tests\n\
test alpha ... ok\n\
test beta ... FAILED\n\
test result: FAILED. 1 passed; 1 failed; 0 ignored";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TuiOutputKind::Test);
        assert_eq!(blocks[0].status, TuiBlockStatus::Failed);
    }

    #[test]
    fn typed_events_map_directly_to_domain_blocks() {
        let events = [
            TuiOutputEvent::tool(
                "apply_patch",
                EventState::Passed,
                "updated src/tui_output.rs",
            ),
            TuiOutputEvent::terminal("cargo test tui_output", EventState::Running, "running"),
            TuiOutputEvent::artifact("report.md", "paper draft"),
            TuiOutputEvent::research_evidence("claim-a", "bench result supports claim"),
            TuiOutputEvent::error("compile", "expected item"),
        ];

        let blocks = blocks_from_events(events, FoldPolicy::Expanded);

        assert_eq!(
            blocks.iter().map(|block| block.kind).collect::<Vec<_>>(),
            vec![
                TuiOutputKind::Tool,
                TuiOutputKind::Terminal,
                TuiOutputKind::Artifact,
                TuiOutputKind::ResearchEvidence,
                TuiOutputKind::Error,
            ]
        );
        assert_eq!(blocks[1].status, TuiBlockStatus::Running);
        assert_eq!(blocks[4].status, TuiBlockStatus::Failed);
        assert!(matches!(
            blocks[0].payload,
            Some(TuiStructuredPayload::Tool { .. })
        ));
        assert!(matches!(
            blocks[3].payload,
            Some(TuiStructuredPayload::ResearchEvidence { .. })
        ));
    }

    #[test]
    fn structured_payloads_are_derived_for_legacy_diff_code_and_tests() {
        let body = "```rust\nfn main() {}\n```\n\
diff --git a/src/lib.rs b/src/lib.rs\n\
--- a/src/lib.rs\n\
+++ b/src/lib.rs\n\
@@ -1 +1 @@\n\
-old\n\
+new\n\
running 2 tests\n\
test alpha ... ok\n\
test beta ... failed\n\
test result: failed. 1 passed; 1 failed";

        let blocks = blocks_from_legacy_text(body, FoldPolicy::Expanded);

        let code = blocks
            .iter()
            .find(|block| block.kind == TuiOutputKind::Code)
            .expect("code block");
        assert_eq!(
            code.payload,
            Some(TuiStructuredPayload::Code {
                language: "rust".to_string(),
                file_path: None,
                line_range: None,
                line_count: 1,
            })
        );

        let diff = blocks
            .iter()
            .find(|block| block.kind == TuiOutputKind::Diff)
            .expect("diff block");
        assert_eq!(
            diff.payload,
            Some(TuiStructuredPayload::Diff {
                file_path: "src/lib.rs".to_string(),
                additions: 1,
                deletions: 1,
                hunks: 1,
            })
        );

        let test = blocks
            .iter()
            .find(|block| {
                block.kind == TuiOutputKind::Test
                    && block
                        .lines
                        .iter()
                        .any(|line| line.starts_with("test result:"))
            })
            .expect("test block");
        assert_eq!(
            test.payload,
            Some(TuiStructuredPayload::Test {
                pass_count: 1,
                total: 2,
                failures: vec!["test beta ... failed".to_string()],
                duration: None,
            })
        );
    }

    #[test]
    fn folding_keeps_summary_and_hidden_count() {
        let body = (0..8)
            .map(|index| format!("test case_{index} ... ok"))
            .collect::<Vec<_>>()
            .join("\n");

        let blocks = blocks_from_legacy_text(&body, FoldPolicy::FoldLong { visible_lines: 3 });

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].fold.is_folded());
        assert_eq!(blocks[0].visible_lines().len(), 3);
        assert_eq!(blocks[0].fold.hidden_count(), 5);
        assert!(blocks[0].fold.summary().contains("5"));
    }
}
