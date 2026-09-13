/// Semantic composer input events after terminal-specific key decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    InsertText(String),
    Backspace,
    Delete,
    MoveLeft,
    MoveRight,
    MoveHome,
    MoveEnd,
    HistoryPrevious,
    HistoryNext,
    Newline,
    RequestCompletion,
}

/// Result of applying a composer action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerOutcome {
    Edited,
    Unchanged,
    CompletionRequested(CompletionRequest),
}

/// Snapshot emitted when the UI wants completions for the current cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    pub text: String,
    pub cursor: usize,
}

/// Pure editable composer state.
///
/// The cursor is a byte offset into `text`, always maintained at a UTF-8 char
/// boundary so callers can use it directly with Rust string slicing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposerState {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_cursor: Option<usize>,
    draft_before_history: Option<String>,
}

impl ComposerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
            history: Vec::new(),
            history_cursor: None,
            draft_before_history: None,
        }
    }

    pub fn with_history(history: Vec<String>) -> Self {
        Self {
            history,
            ..Self::default()
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    pub fn replace_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.finish_history_navigation();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.finish_history_navigation();
    }

    pub fn record_current_submission(&mut self) {
        let submitted = self.text.clone();
        self.record_submission(submitted);
        self.clear();
    }

    pub fn record_submission(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        if self.history.last() != Some(&text) {
            self.history.push(text);
        }
        self.history_cursor = None;
        self.draft_before_history = None;
    }

    pub fn apply(&mut self, action: ComposerAction) -> ComposerOutcome {
        match action {
            ComposerAction::InsertText(text) => self.insert_text(&text),
            ComposerAction::Backspace => self.backspace(),
            ComposerAction::Delete => self.delete(),
            ComposerAction::MoveLeft => self.move_left(),
            ComposerAction::MoveRight => self.move_right(),
            ComposerAction::MoveHome => self.move_home(),
            ComposerAction::MoveEnd => self.move_end(),
            ComposerAction::HistoryPrevious => self.history_previous(),
            ComposerAction::HistoryNext => self.history_next(),
            ComposerAction::Newline => self.insert_text("\n"),
            ComposerAction::RequestCompletion => {
                ComposerOutcome::CompletionRequested(CompletionRequest {
                    text: self.text.clone(),
                    cursor: self.cursor,
                })
            }
        }
    }

    fn insert_text(&mut self, inserted: &str) -> ComposerOutcome {
        if inserted.is_empty() {
            return ComposerOutcome::Unchanged;
        }
        self.text.insert_str(self.cursor, inserted);
        self.cursor += inserted.len();
        self.finish_history_navigation();
        ComposerOutcome::Edited
    }

    fn backspace(&mut self) -> ComposerOutcome {
        let Some(previous) = self.previous_boundary(self.cursor) else {
            return ComposerOutcome::Unchanged;
        };
        self.text.drain(previous..self.cursor);
        self.cursor = previous;
        self.finish_history_navigation();
        ComposerOutcome::Edited
    }

    fn delete(&mut self) -> ComposerOutcome {
        if self.cursor == self.text.len() {
            return ComposerOutcome::Unchanged;
        }
        let next = self.next_boundary(self.cursor);
        self.text.drain(self.cursor..next);
        self.finish_history_navigation();
        ComposerOutcome::Edited
    }

    fn move_left(&mut self) -> ComposerOutcome {
        let Some(previous) = self.previous_boundary(self.cursor) else {
            return ComposerOutcome::Unchanged;
        };
        self.cursor = previous;
        ComposerOutcome::Edited
    }

    fn move_right(&mut self) -> ComposerOutcome {
        if self.cursor == self.text.len() {
            return ComposerOutcome::Unchanged;
        }
        self.cursor = self.next_boundary(self.cursor);
        ComposerOutcome::Edited
    }

    fn move_home(&mut self) -> ComposerOutcome {
        let line_start = self.line_start();
        if self.cursor == line_start {
            return ComposerOutcome::Unchanged;
        }
        self.cursor = line_start;
        ComposerOutcome::Edited
    }

    fn move_end(&mut self) -> ComposerOutcome {
        let line_end = self.line_end();
        if self.cursor == line_end {
            return ComposerOutcome::Unchanged;
        }
        self.cursor = line_end;
        ComposerOutcome::Edited
    }

    fn history_previous(&mut self) -> ComposerOutcome {
        if self.history.is_empty() {
            return ComposerOutcome::Unchanged;
        }
        let next_index = match self.history_cursor {
            Some(0) => 0,
            Some(index) => index - 1,
            None => {
                self.draft_before_history = Some(self.text.clone());
                self.history.len() - 1
            }
        };
        self.history_cursor = Some(next_index);
        self.text.clone_from(&self.history[next_index]);
        self.cursor = self.text.len();
        ComposerOutcome::Edited
    }

    fn history_next(&mut self) -> ComposerOutcome {
        let Some(index) = self.history_cursor else {
            return ComposerOutcome::Unchanged;
        };
        if index + 1 < self.history.len() {
            let next_index = index + 1;
            self.history_cursor = Some(next_index);
            self.text.clone_from(&self.history[next_index]);
            self.cursor = self.text.len();
            return ComposerOutcome::Edited;
        }

        self.history_cursor = None;
        self.text = self.draft_before_history.take().unwrap_or_default();
        self.cursor = self.text.len();
        ComposerOutcome::Edited
    }

    fn previous_boundary(&self, from: usize) -> Option<usize> {
        if from == 0 {
            return None;
        }
        self.text[..from]
            .char_indices()
            .last()
            .map(|(index, _)| index)
    }

    fn next_boundary(&self, from: usize) -> usize {
        if from >= self.text.len() {
            return self.text.len();
        }
        self.text[from..]
            .char_indices()
            .nth(1)
            .map(|(offset, _)| from + offset)
            .unwrap_or(self.text.len())
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map(|offset| self.cursor + offset)
            .unwrap_or(self.text.len())
    }

    fn finish_history_navigation(&mut self) {
        self.history_cursor = None;
        self.draft_before_history = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_chinese_text_and_edits_at_utf8_boundaries() {
        let mut composer = ComposerState::new();

        assert_eq!(
            composer.apply(ComposerAction::InsertText("你好世界".to_string())),
            ComposerOutcome::Edited
        );
        composer.apply(ComposerAction::MoveLeft);
        composer.apply(ComposerAction::Backspace);
        composer.apply(ComposerAction::InsertText("，".to_string()));

        assert_eq!(composer.text(), "你好，界");
        assert_eq!(composer.cursor(), "你好，".len());
    }

    #[test]
    fn cursor_editing_supports_left_right_home_end_and_delete() {
        let mut composer = ComposerState::from_text("alpha\nbravo");

        composer.apply(ComposerAction::MoveLeft);
        composer.apply(ComposerAction::MoveLeft);
        composer.apply(ComposerAction::MoveHome);
        composer.apply(ComposerAction::InsertText("> ".to_string()));
        composer.apply(ComposerAction::MoveEnd);
        composer.apply(ComposerAction::Delete);
        composer.apply(ComposerAction::MoveLeft);
        composer.apply(ComposerAction::Delete);
        composer.apply(ComposerAction::MoveRight);

        assert_eq!(composer.text(), "alpha\n> brav");
        assert_eq!(composer.cursor(), composer.text().len());
    }

    #[test]
    fn history_prev_next_restores_draft_and_allows_recording_submission() {
        let mut composer = ComposerState::new();
        composer.record_submission("first");
        composer.record_submission("second");
        composer.apply(ComposerAction::InsertText("draft".to_string()));

        composer.apply(ComposerAction::HistoryPrevious);
        assert_eq!(composer.text(), "second");
        composer.apply(ComposerAction::HistoryPrevious);
        assert_eq!(composer.text(), "first");
        composer.apply(ComposerAction::HistoryPrevious);
        assert_eq!(composer.text(), "first");

        composer.apply(ComposerAction::HistoryNext);
        assert_eq!(composer.text(), "second");
        composer.apply(ComposerAction::HistoryNext);
        assert_eq!(composer.text(), "draft");

        composer.replace_text("second");
        composer.record_current_submission();
        assert_eq!(
            composer.history(),
            &["first".to_string(), "second".to_string()]
        );
        assert_eq!(composer.text(), "");
    }

    #[test]
    fn newline_inserts_multiline_text_at_cursor() {
        let mut composer = ComposerState::from_text("hello world");
        composer.apply(ComposerAction::MoveHome);
        composer.apply(ComposerAction::MoveRight);
        composer.apply(ComposerAction::MoveRight);
        composer.apply(ComposerAction::Newline);

        assert_eq!(composer.text(), "he\nllo world");
        assert_eq!(composer.cursor(), "he\n".len());
    }

    #[test]
    fn tab_returns_completion_request_without_mutating_text() {
        let mut composer = ComposerState::from_text("/pro");
        composer.apply(ComposerAction::MoveLeft);

        let outcome = composer.apply(ComposerAction::RequestCompletion);

        assert_eq!(
            outcome,
            ComposerOutcome::CompletionRequested(CompletionRequest {
                text: "/pro".to_string(),
                cursor: "/pr".len(),
            })
        );
        assert_eq!(composer.text(), "/pro");
        assert_eq!(composer.cursor(), "/pr".len());
    }
}
