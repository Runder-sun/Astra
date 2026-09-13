use crate::host_surface::{TuiCommandModel, TuiSkillModel};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::{Hint, Hinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{
    Cmd, CompletionType, ConditionalEventHandler, Config, Context, EditMode, Editor, Event,
    EventContext, EventHandler, Helper, KeyCode, KeyEvent, Modifiers, RepeatCount,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

// Current default inline TUI input layer.
//
// Edit this file for terminal editing, IME/cursor behavior, `/` command
// dropdowns, `$` skill dropdowns, and arrow-key candidate selection. Runtime
// execution, command side effects, response rendering, and thinking spinner
// behavior belong in src/tui.rs.
const DROPDOWN_VISIBLE_ROWS: usize = 8;

/// Completion catalog for the Claw-Code-style inline REPL.
///
/// This is adapted from the MIT-licensed Claw-Code Rust input layer, but uses
/// Astra's projected command and skill models as the source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplCompletionCatalog {
    candidates: Vec<String>,
}

impl ReplCompletionCatalog {
    pub fn new(
        command_model: &TuiCommandModel,
        skill_model: &TuiSkillModel,
        dynamic_candidates: Vec<String>,
    ) -> Self {
        let mut seen = BTreeSet::new();
        for group in &command_model.groups {
            for command in &group.commands {
                insert_candidate(&mut seen, &command.typed);
            }
        }
        for skill in &skill_model.entries {
            if skill.enabled && !skill.degraded {
                insert_candidate(&mut seen, &skill.typed);
            }
        }
        for candidate in dynamic_candidates {
            insert_candidate(&mut seen, &candidate);
        }
        Self {
            candidates: seen.into_iter().collect(),
        }
    }

    pub fn candidates(&self) -> Vec<String> {
        self.candidates.clone()
    }
}

fn insert_candidate(seen: &mut BTreeSet<String>, candidate: &str) {
    let candidate = candidate.trim();
    if candidate.starts_with('/') || candidate.starts_with('$') {
        seen.insert(candidate.to_string());
    }
}

fn normalize_completion_candidates(completions: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for completion in completions {
        insert_candidate(&mut seen, &completion);
    }
    seen.into_iter().collect()
}

#[derive(Debug, Clone)]
pub struct ReplCompletionHelper {
    completions: Arc<Mutex<Vec<String>>>,
    current_line: RefCell<String>,
    dropdown: Arc<Mutex<ReplDropdownState>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplDropdownHint {
    display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ReplDropdownState {
    prefix: String,
    selected: usize,
}

impl Hint for ReplDropdownHint {
    fn display(&self) -> &str {
        &self.display
    }

    fn completion(&self) -> Option<&str> {
        None
    }
}

impl ReplCompletionHelper {
    pub fn new(completions: Vec<String>) -> Self {
        Self::with_dropdown(
            completions,
            Arc::new(Mutex::new(ReplDropdownState::default())),
        )
    }

    fn with_dropdown(completions: Vec<String>, dropdown: Arc<Mutex<ReplDropdownState>>) -> Self {
        Self {
            completions: Arc::new(Mutex::new(normalize_completion_candidates(completions))),
            current_line: RefCell::new(String::new()),
            dropdown,
        }
    }

    pub fn matching_candidates(&self, line: &str, pos: usize) -> Vec<String> {
        self.matching_candidates_from(line, pos, &self.completions())
    }

    pub fn dropdown_hint(&self, line: &str, pos: usize) -> Option<ReplDropdownHint> {
        let prefix = completion_prefix(line, pos)?;
        let matches = self.matching_candidates(line, pos);
        if matches.is_empty() {
            return None;
        }
        let selected = selected_dropdown_index(&self.dropdown, prefix, matches.len());
        Some(ReplDropdownHint {
            display: render_dropdown_hint(prefix, &matches, selected),
        })
    }

    fn set_completions(&mut self, completions: Vec<String>) {
        let normalized = normalize_completion_candidates(completions);
        if let Ok(mut current) = self.completions.lock() {
            *current = normalized;
        }
        if let Ok(mut dropdown) = self.dropdown.lock() {
            dropdown.selected = dropdown
                .selected
                .min(self.completions().len().saturating_sub(1));
        }
    }

    fn matching_candidates_from(
        &self,
        line: &str,
        pos: usize,
        completions: &[String],
    ) -> Vec<String> {
        let Some(prefix) = completion_prefix(line, pos) else {
            return Vec::new();
        };
        completions
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn completions(&self) -> Vec<String> {
        self.completions
            .lock()
            .map(|completions| completions.clone())
            .unwrap_or_default()
    }

    fn reset_current_line(&self) {
        self.current_line.borrow_mut().clear();
    }

    fn current_line(&self) -> String {
        self.current_line.borrow().clone()
    }

    fn set_current_line(&self, line: &str) {
        let mut current = self.current_line.borrow_mut();
        current.clear();
        current.push_str(line);
    }
}

impl Completer for ReplCompletionHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let matches = self
            .matching_candidates(line, pos)
            .into_iter()
            .map(|candidate| Pair {
                display: candidate.clone(),
                replacement: candidate,
            })
            .collect();
        Ok((0, matches))
    }
}

impl Hinter for ReplCompletionHelper {
    type Hint = ReplDropdownHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        self.dropdown_hint(line, pos)
    }
}

impl Highlighter for ReplCompletionHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        self.set_current_line(line);
        Cow::Borrowed(line)
    }

    fn highlight_char(&self, line: &str, _pos: usize, _kind: CmdKind) -> bool {
        self.set_current_line(line);
        false
    }
}

impl Validator for ReplCompletionHelper {}
impl Helper for ReplCompletionHelper {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplLineOutcome {
    Submit(String),
    Cancel,
    Exit,
}

/// Rustyline-backed input layer adapted from Claw-Code's inline REPL.
///
/// The surrounding TUI owns product copy, command routing, and response
/// rendering. This editor owns terminal text editing, history, IME-friendly
/// cursor placement, and Tab completion.
pub struct ReplLineEditor {
    prompt: String,
    editor: Editor<ReplCompletionHelper, DefaultHistory>,
    dropdown: Arc<Mutex<ReplDropdownState>>,
}

impl ReplLineEditor {
    pub fn new(prompt: impl Into<String>, completions: Vec<String>) -> io::Result<Self> {
        let dropdown = Arc::new(Mutex::new(ReplDropdownState::default()));
        let completions = Arc::new(Mutex::new(normalize_completion_candidates(completions)));
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .edit_mode(EditMode::Emacs)
            .build();
        let mut editor = Editor::<ReplCompletionHelper, DefaultHistory>::with_config(config)
            .map_err(io::Error::other)?;
        editor.set_helper(Some(ReplCompletionHelper {
            completions: Arc::clone(&completions),
            current_line: RefCell::new(String::new()),
            dropdown: Arc::clone(&dropdown),
        }));
        editor.bind_sequence(KeyEvent(KeyCode::Char('J'), Modifiers::CTRL), Cmd::Newline);
        editor.bind_sequence(KeyEvent(KeyCode::Enter, Modifiers::SHIFT), Cmd::Newline);
        bind_dropdown_keys(&mut editor, Arc::clone(&dropdown), Arc::clone(&completions));
        Ok(Self {
            prompt: prompt.into(),
            editor,
            dropdown,
        })
    }

    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = prompt.into();
    }

    pub fn set_completions(&mut self, completions: Vec<String>) {
        if let Some(helper) = self.editor.helper_mut() {
            helper.set_completions(completions);
        }
    }

    pub fn push_history(&mut self, entry: impl Into<String>) {
        let entry = entry.into();
        if !entry.trim().is_empty() {
            let _ = self.editor.add_history_entry(entry);
        }
    }

    pub fn read_line(&mut self) -> io::Result<ReplLineOutcome> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return self.read_line_fallback();
        }
        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }
        reset_dropdown(&self.dropdown);
        match self.editor.readline(&self.prompt) {
            Ok(line) => Ok(ReplLineOutcome::Submit(line)),
            Err(ReadlineError::Interrupted) => {
                let has_input = !self.current_line().trim().is_empty();
                self.finish_interrupted_read()?;
                if has_input {
                    Ok(ReplLineOutcome::Cancel)
                } else {
                    Ok(ReplLineOutcome::Exit)
                }
            }
            Err(ReadlineError::Eof) => {
                self.finish_interrupted_read()?;
                Ok(ReplLineOutcome::Exit)
            }
            Err(error) => Err(io::Error::other(error)),
        }
    }

    fn current_line(&self) -> String {
        self.editor
            .helper()
            .map_or_else(String::new, ReplCompletionHelper::current_line)
    }

    fn finish_interrupted_read(&mut self) -> io::Result<()> {
        if let Some(helper) = self.editor.helper_mut() {
            helper.reset_current_line();
        }
        writeln!(io::stdout())
    }

    fn read_line_fallback(&self) -> io::Result<ReplLineOutcome> {
        let mut stdout = io::stdout();
        write!(stdout, "{}", self.prompt)?;
        stdout.flush()?;
        let mut buffer = String::new();
        let bytes = io::stdin().read_line(&mut buffer)?;
        if bytes == 0 {
            return Ok(ReplLineOutcome::Exit);
        }
        while matches!(buffer.chars().last(), Some('\n' | '\r')) {
            buffer.pop();
        }
        Ok(ReplLineOutcome::Submit(buffer))
    }
}

#[derive(Clone)]
struct DropdownKeyHandler {
    dropdown: Arc<Mutex<ReplDropdownState>>,
    completions: Arc<Mutex<Vec<String>>>,
}

impl DropdownKeyHandler {
    fn new(dropdown: Arc<Mutex<ReplDropdownState>>, completions: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            dropdown,
            completions,
        }
    }
}

impl ConditionalEventHandler for DropdownKeyHandler {
    fn handle(
        &self,
        evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        ctx: &EventContext,
    ) -> Option<Cmd> {
        let KeyEvent(code, modifiers) = evt.get(0).copied()?;
        if modifiers != Modifiers::NONE {
            return None;
        }
        let (line, pos) = (ctx.line(), ctx.pos());
        let completions = self.completions.lock().ok()?.clone();
        match code {
            KeyCode::Down => {
                if move_dropdown_selection(&self.dropdown, line, pos, &completions, 1) {
                    Some(Cmd::Repaint)
                } else {
                    None
                }
            }
            KeyCode::Up => {
                if move_dropdown_selection(&self.dropdown, line, pos, &completions, -1) {
                    Some(Cmd::Repaint)
                } else {
                    None
                }
            }
            KeyCode::Enter => {
                let candidate =
                    selected_dropdown_candidate(&self.dropdown, line, pos, &completions)?;
                if candidate == line {
                    reset_dropdown(&self.dropdown);
                    None
                } else if let Some(suffix) = dropdown_candidate_suffix(line, &candidate) {
                    reset_dropdown(&self.dropdown);
                    Some(Cmd::Insert(1, suffix))
                } else {
                    reset_dropdown(&self.dropdown);
                    Some(Cmd::Replace(
                        rustyline::Movement::WholeLine,
                        Some(candidate),
                    ))
                }
            }
            _ => None,
        }
    }
}

fn bind_dropdown_keys(
    editor: &mut Editor<ReplCompletionHelper, DefaultHistory>,
    dropdown: Arc<Mutex<ReplDropdownState>>,
    completions: Arc<Mutex<Vec<String>>>,
) {
    let handler = DropdownKeyHandler::new(dropdown, completions);
    editor.bind_sequence(
        KeyEvent(KeyCode::Down, Modifiers::NONE),
        EventHandler::Conditional(Box::new(handler.clone())),
    );
    editor.bind_sequence(
        KeyEvent(KeyCode::Up, Modifiers::NONE),
        EventHandler::Conditional(Box::new(handler.clone())),
    );
    editor.bind_sequence(
        KeyEvent(KeyCode::Enter, Modifiers::NONE),
        EventHandler::Conditional(Box::new(handler)),
    );
}

fn completion_prefix(line: &str, pos: usize) -> Option<&str> {
    if pos != line.len() {
        return None;
    }
    let prefix = &line[..pos];
    if prefix.starts_with('/') || prefix.starts_with('$') {
        Some(prefix)
    } else {
        None
    }
}

fn selected_dropdown_index(
    dropdown: &Arc<Mutex<ReplDropdownState>>,
    prefix: &str,
    match_count: usize,
) -> usize {
    let Ok(mut dropdown) = dropdown.lock() else {
        return 0;
    };
    if dropdown.prefix != prefix {
        dropdown.prefix = prefix.to_string();
        dropdown.selected = 0;
    }
    dropdown.selected = dropdown.selected.min(match_count.saturating_sub(1));
    dropdown.selected
}

fn move_dropdown_selection(
    dropdown: &Arc<Mutex<ReplDropdownState>>,
    line: &str,
    pos: usize,
    completions: &[String],
    delta: isize,
) -> bool {
    let Some(prefix) = completion_prefix(line, pos) else {
        return false;
    };
    let count = completions
        .iter()
        .filter(|candidate| candidate.starts_with(prefix))
        .count();
    if count == 0 {
        return false;
    }
    let Ok(mut dropdown) = dropdown.lock() else {
        return false;
    };
    if dropdown.prefix != prefix {
        dropdown.prefix = prefix.to_string();
        dropdown.selected = 0;
    }
    let selected = dropdown.selected.min(count.saturating_sub(1));
    dropdown.selected = if delta < 0 {
        selected.saturating_sub(1)
    } else {
        (selected + 1).min(count.saturating_sub(1))
    };
    true
}

fn selected_dropdown_candidate(
    dropdown: &Arc<Mutex<ReplDropdownState>>,
    line: &str,
    pos: usize,
    completions: &[String],
) -> Option<String> {
    let prefix = completion_prefix(line, pos)?;
    let matches = completions
        .iter()
        .filter(|candidate| candidate.starts_with(prefix))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }
    let selected = selected_dropdown_index(dropdown, prefix, matches.len());
    matches.get(selected).map(|candidate| (*candidate).clone())
}

fn dropdown_candidate_suffix(line: &str, candidate: &str) -> Option<String> {
    candidate
        .strip_prefix(line)
        .map(|suffix| suffix.to_string())
}

fn reset_dropdown(dropdown: &Arc<Mutex<ReplDropdownState>>) {
    if let Ok(mut dropdown) = dropdown.lock() {
        dropdown.prefix.clear();
        dropdown.selected = 0;
    }
}

fn visible_dropdown_window(selected: usize, total: usize) -> std::ops::Range<usize> {
    if total <= DROPDOWN_VISIBLE_ROWS {
        return 0..total;
    }
    let half = DROPDOWN_VISIBLE_ROWS / 2;
    let mut start = selected.saturating_sub(half);
    if start + DROPDOWN_VISIBLE_ROWS > total {
        start = total - DROPDOWN_VISIBLE_ROWS;
    }
    start..(start + DROPDOWN_VISIBLE_ROWS)
}

fn render_dropdown_hint(prefix: &str, matches: &[String], selected: usize) -> String {
    let title = if prefix.starts_with('$') {
        "技能"
    } else {
        "命令"
    };
    let mut lines = vec![format!(
        "\n\x1b[2m╭─\x1b[0m \x1b[38;5;208m{title}\x1b[0m \x1b[2m↑↓ 选择，Enter 采用，继续输入过滤\x1b[0m"
    )];
    let selected = selected.min(matches.len().saturating_sub(1));
    let window = visible_dropdown_window(selected, matches.len());
    for index in window.clone() {
        let candidate = &matches[index];
        if index == selected {
            lines.push(format!(
                "\x1b[2m│\x1b[0m \x1b[48;5;230m\x1b[38;5;130m▌ {:<28}\x1b[0m",
                candidate
            ));
        } else {
            lines.push(format!("\x1b[2m│\x1b[0m   {candidate}"));
        }
    }
    if matches.len() > DROPDOWN_VISIBLE_ROWS {
        lines.push(format!(
            "\x1b[2m│  {}/{} · ↑↓ 滚动查看更多\x1b[0m",
            selected + 1,
            matches.len()
        ));
    }
    lines.push("\x1b[2m╰────────────────────────────────\x1b[0m".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_surface::{
        TuiCommandEntry, TuiCommandGroup, TuiCommandModel, TuiSkillEntry, TuiSkillModel,
    };
    use rustyline::hint::Hinter;

    fn command_model() -> TuiCommandModel {
        TuiCommandModel {
            command_prefix: "/".to_string(),
            groups: vec![TuiCommandGroup {
                group_id: "core".to_string(),
                label: "Core".to_string(),
                commands: vec![
                    TuiCommandEntry {
                        typed: "/help".to_string(),
                        action_id: "open_help".to_string(),
                        label: "Help".to_string(),
                        summary: "Show help".to_string(),
                        gate: "always".to_string(),
                    },
                    TuiCommandEntry {
                        typed: "/model".to_string(),
                        action_id: "select_model".to_string(),
                        label: "Model".to_string(),
                        summary: "Select model".to_string(),
                        gate: "always".to_string(),
                    },
                ],
            }],
        }
    }

    fn skill_model() -> TuiSkillModel {
        TuiSkillModel {
            skill_prefix: "$".to_string(),
            total_count: 2,
            degraded_count: 0,
            entries: vec![
                TuiSkillEntry {
                    typed: "$research-lit".to_string(),
                    skill_id: "research-lit".to_string(),
                    description: "Research retrieval and analysis".to_string(),
                    enabled: true,
                    degraded: false,
                    source: "fixture".to_string(),
                },
                TuiSkillEntry {
                    typed: "$research-review".to_string(),
                    skill_id: "research-review".to_string(),
                    description: "Review research risks".to_string(),
                    enabled: true,
                    degraded: false,
                    source: "fixture".to_string(),
                },
            ],
        }
    }

    #[test]
    fn repl_completion_candidates_include_commands_skills_and_dynamic_arguments() {
        let candidates = ReplCompletionCatalog::new(
            &command_model(),
            &skill_model(),
            vec![
                "/model gpt-5.5".to_string(),
                "/model gpt-5.5".to_string(),
                "/reasoning high".to_string(),
                "/resume latest".to_string(),
            ],
        )
        .candidates();

        assert!(candidates.contains(&"/help".to_string()));
        assert!(candidates.contains(&"/model".to_string()));
        assert!(candidates.contains(&"/model gpt-5.5".to_string()));
        assert!(candidates.contains(&"/reasoning high".to_string()));
        assert!(candidates.contains(&"/resume latest".to_string()));
        assert!(candidates.contains(&"$research-lit".to_string()));
        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.as_str() == "/model gpt-5.5")
                .count(),
            1
        );
    }

    #[test]
    fn slash_prefix_completion_only_runs_at_end_of_line() {
        let helper = ReplCompletionHelper::new(vec![
            "/help".to_string(),
            "/model gpt-5.5".to_string(),
            "$research-lit".to_string(),
        ]);

        assert_eq!(helper.matching_candidates("/mo", 3), vec!["/model gpt-5.5"]);
        assert_eq!(
            helper.matching_candidates("$research", 9),
            vec!["$research-lit"]
        );
        assert!(helper.matching_candidates("ask /mo", 7).is_empty());
        assert!(helper.matching_candidates("/model", 2).is_empty());
    }

    #[test]
    fn slash_and_dollar_prefixes_render_visible_dropdown_without_tab() {
        let helper = ReplCompletionHelper::new(vec![
            "/help".to_string(),
            "/model gpt-5.5".to_string(),
            "$research-lit".to_string(),
            "$research-review".to_string(),
        ]);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);

        let slash_hint = helper.hint("/", 1, &ctx).expect("slash should show menu");
        assert!(slash_hint.display().contains("╭─"));
        assert!(slash_hint.display().contains("命令"));
        assert!(slash_hint.display().contains("/help"));
        assert!(slash_hint.display().contains("/model gpt-5.5"));
        assert!(slash_hint.completion().is_none());

        let skill_hint = helper.hint("$", 1, &ctx).expect("dollar should show menu");
        assert!(skill_hint.display().contains("╭─"));
        assert!(skill_hint.display().contains("技能"));
        assert!(skill_hint.display().contains("$research-lit"));
        assert!(skill_hint.display().contains("$research-review"));
        assert!(skill_hint.completion().is_none());
    }

    #[test]
    fn dropdown_selection_scrolls_visible_window_and_highlights_current_row() {
        let matches = (0..12)
            .map(|index| format!("/cmd-{index:02}"))
            .collect::<Vec<_>>();

        let rendered = render_dropdown_hint("/", &matches, 10);

        assert!(!rendered.contains("/cmd-00"));
        assert!(!rendered.contains("/cmd-01"));
        assert!(!rendered.contains("/cmd-03"));
        assert!(rendered.contains("/cmd-04"));
        assert!(rendered.contains("/cmd-10"));
        assert!(rendered.contains("/cmd-11"));
        assert!(rendered.contains("11/12"));
        assert!(rendered.contains("▌ /cmd-10"));
        assert_eq!(rendered.matches("▌ ").count(), 1);
    }

    #[test]
    fn dropdown_selection_moves_within_filtered_matches_without_wrapping() {
        let dropdown = Arc::new(Mutex::new(ReplDropdownState::default()));
        let completions = (0..12)
            .map(|index| format!("/cmd-{index:02}"))
            .collect::<Vec<_>>();

        for _ in 0..15 {
            assert!(move_dropdown_selection(
                &dropdown,
                "/cmd",
                4,
                &completions,
                1,
            ));
        }
        assert_eq!(
            selected_dropdown_candidate(&dropdown, "/cmd", 4, &completions).as_deref(),
            Some("/cmd-11")
        );

        for _ in 0..20 {
            assert!(move_dropdown_selection(
                &dropdown,
                "/cmd",
                4,
                &completions,
                -1,
            ));
        }
        assert_eq!(
            selected_dropdown_candidate(&dropdown, "/cmd", 4, &completions).as_deref(),
            Some("/cmd-00")
        );
    }

    #[test]
    fn dropdown_adopts_candidate_by_inserting_suffix_at_cursor() {
        assert_eq!(
            dropdown_candidate_suffix("/fo", "/fold").as_deref(),
            Some("ld")
        );
        assert_eq!(
            dropdown_candidate_suffix("$research", "$research-review").as_deref(),
            Some("-review")
        );
        assert_eq!(
            dropdown_candidate_suffix("/fold", "/fold").as_deref(),
            Some("")
        );
        assert!(dropdown_candidate_suffix("/x", "/fold").is_none());
    }
}
