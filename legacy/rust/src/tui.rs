use crate::host_surface::{HostResearchSummary, TuiInlineView, TuiLaunchResult};
use crate::runtime::cancel::{runtime_interrupt_pair, RuntimeCancelToken, RuntimeInterruptHandle};
use crate::surface_commands::{
    parse_surface_command, product_command_specs, SurfaceCommandKind, SurfaceCommandRequest,
    SurfaceCommandSpec,
};
use crate::tui_composer::{ComposerAction, ComposerState};
use crate::tui_markdown::{MarkdownStreamState, TerminalMarkdownRenderer};
use crate::tui_repl::{ReplCompletionCatalog, ReplLineEditor, ReplLineOutcome};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
use std::env;
#[cfg(test)]
use std::fs;
use std::io::{self, IsTerminal, Read, Stdout, Write};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
use unicode_width::UnicodeWidthStr;

// Current product TUI path:
// runtime -> run_inline_tui_with_cancellable_streaming_and_action_executors
// -> InlineReplBackend::ClawRichInline
// -> run_claw_rich_inline_repl_with_action_executors
// -> tui_repl::ReplLineEditor.
//
// See docs/tui-current-architecture.md before changing the TUI. This file still
// contains historical raw-mode/fullscreen helpers for tests and future cleanup;
// do not treat every "rich inline" function as part of the current default path.
static INLINE_REPL_SIGINT_REQUESTED: AtomicBool = AtomicBool::new(false);
const SELECTED_LINE_PREFIX: &str = "\x1b[48;5;230m\x1b[38;5;130m";
const ANSI_RESET: &str = "\x1b[0m";
const INLINE_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const INLINE_SPINNER_TICK: Duration = Duration::from_millis(160);

extern "C" fn inline_repl_sigint_handler(_signal: libc::c_int) {
    INLINE_REPL_SIGINT_REQUESTED.store(true, Ordering::SeqCst);
}

pub fn run_inline_tui(result: &TuiLaunchResult) -> io::Result<String> {
    run_inline_tui_with_executor(result, |_prompt| {
        Ok(TuiCommandExecution::new(
            "提示已接收。当前 TUI 启动未绑定 CLI 回合执行器。".to_string(),
        ))
    })
}

pub fn run_inline_tui_with_executor<F>(result: &TuiLaunchResult, executor: F) -> io::Result<String>
where
    F: FnMut(&str) -> Result<TuiCommandExecution, String> + Send + 'static,
{
    let mut executor = executor;
    run_inline_tui_with_streaming_executor(result, move |prompt, _stream| executor(prompt))
}

pub fn run_inline_tui_with_streaming_executor<F>(
    result: &TuiLaunchResult,
    executor: F,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender) -> Result<TuiCommandExecution, String> + Send + 'static,
{
    let mut executor = executor;
    run_inline_tui_with_cancellable_streaming_executor(result, move |prompt, stream, _cancel| {
        executor(prompt, stream)
    })
}

pub fn run_inline_tui_with_cancellable_streaming_executor<F>(
    result: &TuiLaunchResult,
    executor: F,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
{
    let executor = Arc::new(Mutex::new(executor));
    run_inline_tui_with_cancellable_streaming_and_config_executor(result, executor, |_action| {
        Err("TUI configuration executor is not bound".to_string())
    })
}

pub fn run_inline_tui_with_cancellable_streaming_and_config_executor<F, C>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: C,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
{
    run_inline_tui_with_cancellable_streaming_and_action_executors(
        result,
        executor,
        config_executor,
        |_action| Err("TUI permission executor is not bound".to_string()),
    )
}

pub fn run_inline_tui_with_cancellable_streaming_and_action_executors<F, C, P>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: C,
    permission_executor: P,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let config_executor = Arc::new(Mutex::new(config_executor));
    let permission_executor = Arc::new(Mutex::new(permission_executor));
    let stdin_is_terminal = io::stdin().is_terminal();
    let stdout_is_terminal = io::stdout().is_terminal();
    match select_inline_repl_backend(&result.launch_mode, stdin_is_terminal, stdout_is_terminal) {
        InlineReplBackend::Stdin => {
            return run_non_tty_inline_repl_with_action_executors(
                result,
                executor,
                config_executor,
                permission_executor,
            );
        }
        InlineReplBackend::Snapshot => {
            return Ok(render_inline_snapshot(result, "degraded_non_tty"))
        }
        InlineReplBackend::ClawRichInline => {
            return run_claw_rich_inline_repl_with_action_executors(
                result,
                executor,
                config_executor,
                permission_executor,
            );
        }
        InlineReplBackend::Fullscreen => {}
    }
    if !stdin_is_terminal || !stdout_is_terminal {
        return Ok(render_inline_snapshot(result, "degraded_non_tty"));
    }

    run_fullscreen_ratatui_with_action_executors(
        result,
        executor,
        config_executor,
        permission_executor,
    )
}

fn run_fullscreen_ratatui_with_action_executors<F, C, P>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: Arc<Mutex<C>>,
    permission_executor: Arc<Mutex<P>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let mut active_terminal = ActiveTuiTerminal::enter()?;
    let terminal = active_terminal.terminal_mut();
    let mut model = terminal_frame_model(result);
    let mut interaction = TuiInteractionState::from_result(result);
    apply_interaction_to_model(&mut model, &interaction);

    let exit_key = loop {
        refresh_streaming_turn(&mut interaction);
        apply_interaction_to_model(&mut model, &interaction);
        terminal.draw(|frame| render_ratatui_frame(frame, &model))?;
        if event::poll(Duration::from_millis(
            if interaction.running_turn.is_some() {
                80
            } else {
                250
            },
        ))? {
            let input = read_crossterm_input()?;
            match handle_tui_input_streaming_with_config(
                &mut interaction,
                &input,
                &executor,
                &config_executor,
                &permission_executor,
            ) {
                TuiInputOutcome::Exit(exit_key) => break exit_key,
                TuiInputOutcome::Continue => {}
            }
        }
    };

    active_terminal.restore()?;
    Ok(format!(
        "{} 已退出\nraw_mode: active_terminal_restored\nexit_key: {exit_key}",
        tui_title(result)
    ))
}

fn launch_mode_uses_inline_repl_backend(launch_mode: &str) -> bool {
    launch_mode != "fullscreen_split_pane_projection_renderer"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineReplBackend {
    ClawRichInline,
    Stdin,
    Snapshot,
    Fullscreen,
}

fn select_inline_repl_backend(
    launch_mode: &str,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> InlineReplBackend {
    if !launch_mode_uses_inline_repl_backend(launch_mode) {
        return InlineReplBackend::Fullscreen;
    }
    if !stdin_is_terminal {
        InlineReplBackend::Stdin
    } else if !stdout_is_terminal {
        InlineReplBackend::Snapshot
    } else {
        InlineReplBackend::ClawRichInline
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplReadOutcome {
    Submit(String),
    Cancel,
    Exit,
}

// Historical raw-mode inline REPL. The interactive product path no longer calls
// this; it uses run_claw_rich_inline_repl_with_action_executors plus
// ReplLineEditor instead. Keep this isolated until the legacy fullscreen/raw
// composer tests are either migrated or deleted.
#[allow(dead_code)]
fn run_rich_inline_repl_with_action_executors<F, C, P>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: Arc<Mutex<C>>,
    permission_executor: Arc<Mutex<P>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let mut state = TuiInteractionState::from_result(result);
    let renderer = TerminalMarkdownRenderer::new();
    println!("{}", render_rich_inline_banner(result, &state));

    loop {
        match read_rich_inline_submission(result, &mut state)? {
            ReplReadOutcome::Submit(input) => {
                let input = input.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                if typed_exit_requested(&input) {
                    return Ok("Astra Code rich inline TUI 已退出\nraw_mode: rich_inline_repl\nexit_key: slash-exit".to_string());
                }
                push_input_history_entry(&mut state, &input);
                if typed_interrupt_requested(&input) && state.running_turn.is_some() {
                    interrupt_running_turn(&mut state);
                    println!(
                        "{}",
                        state.language.text("已请求中断。", "Interrupt requested.")
                    );
                    continue;
                }
                println!("\x1b[2mYou\x1b[0m {input}");
                run_inline_repl_submission(
                    &mut state,
                    &input,
                    &executor,
                    &config_executor,
                    &permission_executor,
                    &renderer,
                )?;
            }
            ReplReadOutcome::Cancel => {
                println!("{}", state.language.text("输入已清空。", "Input cleared."));
            }
            ReplReadOutcome::Exit => {
                return Ok("Astra Code rich inline TUI 已退出\nraw_mode: rich_inline_repl\nexit_key: ctrl-d".to_string());
            }
        }
    }
}

// Current interactive inline product loop. UI/input changes for the default
// terminal experience should normally start here or in src/tui_repl.rs.
fn run_claw_rich_inline_repl_with_action_executors<F, C, P>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: Arc<Mutex<C>>,
    permission_executor: Arc<Mutex<P>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let mut state = TuiInteractionState::from_result(result);
    let renderer = TerminalMarkdownRenderer::new();
    let mut editor = ReplLineEditor::new(
        claw_line_editor_prompt(state.language),
        repl_completion_candidates(result, &state),
    )?;
    println!("{}", render_inline_repl_banner(result, &state));

    loop {
        editor.set_prompt(claw_line_editor_prompt(state.language));
        editor.set_completions(repl_completion_candidates(result, &state));
        println!("{}", render_claw_line_editor_context(&state));
        match editor.read_line()? {
            ReplLineOutcome::Submit(input) => {
                let input = input.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                if typed_exit_requested(&input) {
                    return Ok(state
                        .language
                        .text("Astra 已退出", "Astra exited")
                        .to_string());
                }
                push_input_history_entry(&mut state, &input);
                editor.push_history(input.clone());
                if typed_interrupt_requested(&input) && state.running_turn.is_some() {
                    interrupt_running_turn(&mut state);
                    println!(
                        "{}",
                        state.language.text("已请求中断。", "Interrupt requested.")
                    );
                    continue;
                }
                run_inline_repl_submission(
                    &mut state,
                    &input,
                    &executor,
                    &config_executor,
                    &permission_executor,
                    &renderer,
                )?;
            }
            ReplLineOutcome::Cancel => {
                println!("{}", state.language.text("输入已清空。", "Input cleared."));
            }
            ReplLineOutcome::Exit => {
                return Ok(state
                    .language
                    .text("Astra 已退出", "Astra exited")
                    .to_string());
            }
        }
    }
}

struct InlineRawModeGuard;

impl InlineRawModeGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(Self)
    }
}

impl Drop for InlineRawModeGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        let _ = disable_raw_mode();
    }
}

#[allow(dead_code)]
fn read_rich_inline_submission(
    result: &TuiLaunchResult,
    state: &mut TuiInteractionState,
) -> io::Result<ReplReadOutcome> {
    let _raw_mode = InlineRawModeGuard::enter()?;
    let mut stdout = io::stdout();
    let mut rendered_lines = 0usize;
    let mut composer = composer_state_from_interaction(state);
    let mut dirty = true;
    loop {
        if dirty {
            sync_interaction_from_composer(state, &composer);
            rendered_lines = redraw_rich_inline_prompt(
                result,
                state,
                rendered_lines,
                &mut stdout,
                composer.cursor(),
            )?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let key = match event::read()? {
            Event::Paste(text) => {
                composer.apply(ComposerAction::InsertText(text));
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                dirty = true;
                continue;
            }
            Event::Key(key) => key,
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                clear_rich_inline_prompt(rendered_lines, &mut stdout)?;
                return Ok(ReplReadOutcome::Exit);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if composer.text().is_empty() {
                    clear_rich_inline_prompt(rendered_lines, &mut stdout)?;
                    return Ok(ReplReadOutcome::Exit);
                }
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                clear_rich_inline_prompt(rendered_lines, &mut stdout)?;
                return Ok(ReplReadOutcome::Cancel);
            }
            KeyCode::Esc => {
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                composer.apply(ComposerAction::Newline);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                composer.apply(ComposerAction::Newline);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Enter => {
                sync_interaction_from_composer(state, &composer);
                if typed_exit_requested(&state.composer) {
                    clear_rich_inline_prompt(rendered_lines, &mut stdout)?;
                    return Ok(ReplReadOutcome::Submit(state.composer.clone()));
                }
                if (command_palette_active(&state.composer)
                    || skill_palette_active(&state.composer))
                    && select_overlay_candidate(state)
                    && overlay_candidate_needs_more_input(&state.composer)
                {
                    composer.replace_text(state.composer.clone());
                    continue;
                }
                let input = state.composer.trim().to_string();
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                clear_rich_inline_prompt(rendered_lines, &mut stdout)?;
                return Ok(ReplReadOutcome::Submit(input));
            }
            KeyCode::Backspace => {
                composer.apply(ComposerAction::Backspace);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Delete => {
                composer.apply(ComposerAction::Delete);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Left => {
                composer.apply(ComposerAction::MoveLeft);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Right => {
                composer.apply(ComposerAction::MoveRight);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Home => {
                composer.apply(ComposerAction::MoveHome);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::End => {
                composer.apply(ComposerAction::MoveEnd);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Tab => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    select_overlay_candidate(state);
                    composer.replace_text(state.composer.clone());
                }
            }
            KeyCode::Up => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    move_overlay_selection(state, OverlayDirection::Up);
                } else {
                    composer.apply(ComposerAction::HistoryPrevious);
                    sync_interaction_from_composer(state, &composer);
                }
            }
            KeyCode::Down => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    move_overlay_selection(state, OverlayDirection::Down);
                } else {
                    composer.apply(ComposerAction::HistoryNext);
                    sync_interaction_from_composer(state, &composer);
                }
            }
            KeyCode::PageUp => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    for _ in 0..5 {
                        move_overlay_selection(state, OverlayDirection::Up);
                    }
                }
            }
            KeyCode::PageDown => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    for _ in 0..5 {
                        move_overlay_selection(state, OverlayDirection::Down);
                    }
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                composer.apply(ComposerAction::InsertText(ch.to_string()));
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            _ => {}
        }
        dirty = true;
    }
}

#[allow(dead_code)]
fn read_claw_rich_inline_submission(
    result: &TuiLaunchResult,
    state: &mut TuiInteractionState,
) -> io::Result<ReplReadOutcome> {
    let _raw_mode = InlineRawModeGuard::enter()?;
    let mut stdout = io::stdout();
    let mut rendered_prompt = InlinePromptRenderState::default();
    let mut composer = composer_state_from_interaction(state);
    let mut dirty = true;
    loop {
        if dirty {
            sync_interaction_from_composer(state, &composer);
            rendered_prompt = redraw_claw_rich_inline_prompt(
                result,
                state,
                rendered_prompt,
                &mut stdout,
                composer.cursor(),
            )?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let key = match event::read()? {
            Event::Paste(text) => {
                composer.apply(ComposerAction::InsertText(text));
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                dirty = true;
                continue;
            }
            Event::Key(key) => key,
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                clear_claw_rich_inline_prompt(rendered_prompt, &mut stdout)?;
                return Ok(ReplReadOutcome::Exit);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if composer.text().is_empty() {
                    clear_claw_rich_inline_prompt(rendered_prompt, &mut stdout)?;
                    return Ok(ReplReadOutcome::Exit);
                }
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                clear_claw_rich_inline_prompt(rendered_prompt, &mut stdout)?;
                return Ok(ReplReadOutcome::Cancel);
            }
            KeyCode::Esc => {
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                composer.apply(ComposerAction::Newline);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                composer.apply(ComposerAction::Newline);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Enter => {
                sync_interaction_from_composer(state, &composer);
                if typed_exit_requested(&state.composer) {
                    clear_claw_rich_inline_prompt(rendered_prompt, &mut stdout)?;
                    return Ok(ReplReadOutcome::Submit(state.composer.clone()));
                }
                if (command_palette_active(&state.composer)
                    || skill_palette_active(&state.composer))
                    && select_overlay_candidate(state)
                    && overlay_candidate_needs_more_input(&state.composer)
                {
                    composer.replace_text(state.composer.clone());
                    continue;
                }
                let input = state.composer.trim().to_string();
                composer.clear();
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
                clear_claw_rich_inline_prompt(rendered_prompt, &mut stdout)?;
                return Ok(ReplReadOutcome::Submit(input));
            }
            KeyCode::Backspace => {
                composer.apply(ComposerAction::Backspace);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Delete => {
                composer.apply(ComposerAction::Delete);
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            KeyCode::Left => {
                composer.apply(ComposerAction::MoveLeft);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Right => {
                composer.apply(ComposerAction::MoveRight);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Home => {
                composer.apply(ComposerAction::MoveHome);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::End => {
                composer.apply(ComposerAction::MoveEnd);
                sync_interaction_from_composer(state, &composer);
            }
            KeyCode::Tab => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    select_overlay_candidate(state);
                    composer.replace_text(state.composer.clone());
                }
            }
            KeyCode::Up => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    move_overlay_selection(state, OverlayDirection::Up);
                } else {
                    composer.apply(ComposerAction::HistoryPrevious);
                    sync_interaction_from_composer(state, &composer);
                }
            }
            KeyCode::Down => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    move_overlay_selection(state, OverlayDirection::Down);
                } else {
                    composer.apply(ComposerAction::HistoryNext);
                    sync_interaction_from_composer(state, &composer);
                }
            }
            KeyCode::PageUp => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    for _ in 0..5 {
                        move_overlay_selection(state, OverlayDirection::Up);
                    }
                }
            }
            KeyCode::PageDown => {
                sync_interaction_from_composer(state, &composer);
                if command_palette_active(&state.composer) || skill_palette_active(&state.composer)
                {
                    for _ in 0..5 {
                        move_overlay_selection(state, OverlayDirection::Down);
                    }
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                composer.apply(ComposerAction::InsertText(ch.to_string()));
                sync_interaction_from_composer(state, &composer);
                state.overlay_selected = 0;
            }
            _ => {}
        }
        dirty = true;
    }
}

fn composer_state_from_interaction(state: &TuiInteractionState) -> ComposerState {
    let mut composer = ComposerState::new();
    if state.input_history.is_empty() {
        for entry in &state.prompt_history {
            composer.record_submission(entry.text.clone());
        }
    } else {
        for entry in &state.input_history {
            composer.record_submission(entry.clone());
        }
    }
    composer.replace_text(state.composer.clone());
    composer
}

#[allow(dead_code)]
fn sync_interaction_from_composer(state: &mut TuiInteractionState, composer: &ComposerState) {
    state.composer = composer.text().to_string();
}

fn write_terminal_text(stdout: &mut impl Write, text: &str) -> io::Result<()> {
    stdout.write_all(raw_mode_line_endings(text).as_bytes())?;
    stdout.flush()
}

fn print_terminal_text(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    write_terminal_text(&mut stdout, text)
}

fn print_terminal_control_text(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}

fn raw_mode_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

#[allow(dead_code)]
fn redraw_rich_inline_prompt(
    result: &TuiLaunchResult,
    state: &TuiInteractionState,
    previous_lines: usize,
    stdout: &mut impl Write,
    composer_cursor: usize,
) -> io::Result<usize> {
    clear_rich_inline_prompt(previous_lines, stdout)?;
    let mut model = terminal_frame_model(result);
    apply_interaction_to_model(&mut model, state);
    let width = terminal::size()
        .map(|(cols, _)| usize::from(cols).clamp(20, 132))
        .unwrap_or(96);
    let frame =
        render_rich_inline_prompt_frame_with_width_and_cursor(&model, width, composer_cursor);
    write_terminal_text(stdout, &format!("{frame}\n"))?;
    Ok(frame.lines().count())
}

#[allow(dead_code)]
fn redraw_claw_rich_inline_prompt(
    result: &TuiLaunchResult,
    state: &TuiInteractionState,
    previous: InlinePromptRenderState,
    stdout: &mut impl Write,
    composer_cursor: usize,
) -> io::Result<InlinePromptRenderState> {
    clear_claw_rich_inline_prompt(previous, stdout)?;
    let width = terminal::size()
        .map(|(cols, _)| usize::from(cols).clamp(20, 132))
        .unwrap_or(96);
    let mut model = terminal_frame_model(result);
    apply_interaction_to_model(&mut model, state);
    let rendered = render_claw_rich_inline_prompt_layout(&model, width, composer_cursor);
    write_terminal_text(stdout, &rendered.frame)?;
    move_to_claw_prompt_cursor(stdout, &rendered)?;
    Ok(rendered.state)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code)]
struct InlinePromptRenderState {
    lines: usize,
    cursor_row: usize,
}

#[allow(dead_code)]
fn clear_rich_inline_prompt(lines: usize, stdout: &mut impl Write) -> io::Result<()> {
    if lines == 0 {
        return Ok(());
    }
    execute!(
        stdout,
        crossterm::cursor::MoveUp(u16::try_from(lines).unwrap_or(u16::MAX)),
        terminal::Clear(terminal::ClearType::FromCursorDown)
    )?;
    stdout.flush()
}

#[allow(dead_code)]
fn clear_claw_rich_inline_prompt(
    previous: InlinePromptRenderState,
    stdout: &mut impl Write,
) -> io::Result<()> {
    if previous.lines == 0 {
        return Ok(());
    }
    execute!(
        stdout,
        crossterm::cursor::MoveToColumn(0),
        crossterm::cursor::MoveUp(u16::try_from(previous.cursor_row).unwrap_or(u16::MAX)),
        terminal::Clear(terminal::ClearType::FromCursorDown)
    )?;
    stdout.flush()
}

#[allow(dead_code)]
fn move_to_claw_prompt_cursor(
    stdout: &mut impl Write,
    rendered: &ClawPromptRender,
) -> io::Result<()> {
    if rendered.state.lines == 0 {
        return Ok(());
    }
    let rows_up = rendered
        .state
        .lines
        .saturating_sub(1)
        .saturating_sub(rendered.state.cursor_row);
    execute!(
        stdout,
        crossterm::cursor::MoveUp(u16::try_from(rows_up).unwrap_or(u16::MAX)),
        crossterm::cursor::MoveToColumn(u16::try_from(rendered.cursor_col).unwrap_or(u16::MAX)),
        crossterm::cursor::Show
    )?;
    stdout.flush()
}

#[allow(dead_code)]
fn move_prompt_history(
    state: &mut TuiInteractionState,
    history_cursor: &mut Option<usize>,
    direction: OverlayDirection,
) {
    if state.prompt_history.is_empty() {
        return;
    }
    match direction {
        OverlayDirection::Up => {
            let next = history_cursor
                .map(|index| index.saturating_sub(1))
                .unwrap_or_else(|| state.prompt_history.len().saturating_sub(1));
            *history_cursor = Some(next);
            if let Some(entry) = state.prompt_history.get(next) {
                state.composer = entry.text.clone();
            }
        }
        OverlayDirection::Down => {
            let Some(current) = *history_cursor else {
                return;
            };
            let next = current + 1;
            if next >= state.prompt_history.len() {
                *history_cursor = None;
                state.composer.clear();
            } else {
                *history_cursor = Some(next);
                if let Some(entry) = state.prompt_history.get(next) {
                    state.composer = entry.text.clone();
                }
            }
        }
    }
    state.overlay_selected = 0;
}

#[allow(dead_code)]
fn render_rich_inline_banner(result: &TuiLaunchResult, state: &TuiInteractionState) -> String {
    let mut model = terminal_frame_model(result);
    apply_interaction_to_model(&mut model, state);
    render_rich_inline_status_strip(&model)
}

#[allow(dead_code)]
fn render_rich_inline_status_strip(model: &TerminalFrameModel) -> String {
    let info = research_info_segments(model);
    let mut lines = Vec::new();
    lines.push(format!(
        "\x1b[38;5;208mAstra Code\x1b[0m \x1b[2m{}\x1b[0m",
        model.language.text("代码智能体", "coding agent")
    ));
    lines.push(format!(
        "\x1b[2m{}\x1b[0m {}  \x1b[2m{}\x1b[0m {}  \x1b[2m{}\x1b[0m {}",
        model.language.text("项目", "project"),
        truncate_plain(&model.project_id, 30),
        "model",
        truncate_plain(&model.model_label, 24),
        model.language.text("权限", "permission"),
        compact_permission_mode(&model.permission_mode, model.language)
    ));
    lines.push(format!(
        "\x1b[2m{}\x1b[0m {}  \x1b[2m{}\x1b[0m {}",
        info.thread_label.trim(),
        info.thread,
        info.stage_label.trim(),
        info.stage
    ));
    lines.join("\n")
}

#[cfg(test)]
fn render_rich_inline_prompt_frame_with_width(model: &TerminalFrameModel, width: usize) -> String {
    render_rich_inline_prompt_frame_with_width_and_cursor(model, width, model.composer.len())
}

fn render_rich_inline_prompt_frame_with_width_and_cursor(
    model: &TerminalFrameModel,
    width: usize,
    composer_cursor: usize,
) -> String {
    let width = width.clamp(20, 132);
    let inner = width.saturating_sub(4).max(1);
    let mut lines = Vec::new();
    lines.push(top_border(width));
    lines.push(row(
        width,
        &format!(
            "{}  {}",
            accent("Astra"),
            model.language.text(
                "输入需求，/ 打开命令，$ 调用技能",
                "type a request, / commands, $ skills"
            )
        ),
    ));
    let status = format!(
        "{} {}  ·  {} {}  ·  {} {}",
        model.language.text("模型", "model"),
        truncate_plain(&model.model_label, 20),
        model.language.text("权限", "permission"),
        compact_permission_mode(&model.permission_mode, model.language),
        model.language.text("主题", "theme"),
        theme_indicator(model)
    );
    lines.push(row(width, &status));
    if command_palette_active(&model.composer) {
        lines.push(separator(width));
        lines.extend(
            command_palette_lines(model)
                .into_iter()
                .map(|line| row(width, &fit(&line, inner))),
        );
    } else if skill_palette_active(&model.composer) {
        lines.push(separator(width));
        lines.extend(
            skill_palette_lines(model)
                .into_iter()
                .map(|line| row(width, &fit(&line, inner))),
        );
    }
    lines.push(separator(width));
    lines.extend(
        composer_lines_with_cursor(model, composer_cursor)
            .into_iter()
            .map(|line| row(width, &fit(&line, inner))),
    );
    lines.push(row(width, &localized_footer(model)));
    lines.push(bottom_border(width));
    lines.join("\n")
}

#[allow(dead_code)]
fn render_claw_rich_inline_prompt_with_width_and_cursor(
    model: &TerminalFrameModel,
    width: usize,
    composer_cursor: usize,
) -> String {
    render_claw_rich_inline_prompt_layout(model, width, composer_cursor).frame
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ClawPromptRender {
    frame: String,
    cursor_col: usize,
    state: InlinePromptRenderState,
}

#[allow(dead_code)]
fn render_claw_rich_inline_prompt_layout(
    model: &TerminalFrameModel,
    width: usize,
    composer_cursor: usize,
) -> ClawPromptRender {
    let width = width.clamp(20, 132);
    let inner = width.saturating_sub(4).max(1);
    let mut lines = Vec::new();
    if command_palette_active(&model.composer) {
        lines.extend(
            command_palette_lines(model)
                .into_iter()
                .map(|line| fit(&line, width.saturating_sub(2).max(1))),
        );
    } else if skill_palette_active(&model.composer) {
        lines.extend(
            skill_palette_lines(model)
                .into_iter()
                .map(|line| fit(&line, width.saturating_sub(2).max(1))),
        );
    }

    let input_top_row = lines.len();
    let prompt = claw_prompt_box(model, composer_cursor, inner);
    lines.extend(prompt.lines);
    let cursor_row = input_top_row + prompt.cursor_row;
    let cursor_col = prompt.cursor_col;
    let frame = lines.join("\n");
    ClawPromptRender {
        state: InlinePromptRenderState {
            lines: frame.lines().count(),
            cursor_row,
        },
        frame,
        cursor_col,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ClawPromptBox {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

#[allow(dead_code)]
fn claw_prompt_box(model: &TerminalFrameModel, cursor: usize, inner: usize) -> ClawPromptBox {
    let mut lines = Vec::new();
    let title = format!(
        "\x1b[38;5;208mAstra\x1b[0m \x1b[2m{}\x1b[0m",
        model.language.text("输入", "input")
    );
    lines.push(format!(
        "\x1b[2m╭─\x1b[0m {title} \x1b[2m{}\x1b[0m",
        "─".repeat(inner.saturating_sub(visible_width("Astra input") + 4))
    ));
    let prompt = claw_prompt_content(model, cursor);
    for line in &prompt.lines {
        lines.push(format!("\x1b[2m│\x1b[0m {}", fit(line, inner)));
    }
    lines.push(format!(
        "\x1b[2m╰─ {} · {} · {} · {}\x1b[0m",
        model.language.text("/ 命令", "/ commands"),
        model.language.text("$ 技能", "$ skills"),
        model.language.text("Enter 发送", "Enter send"),
        model.language.text("Esc 清空", "Esc clear")
    ));
    ClawPromptBox {
        lines,
        cursor_row: 1 + prompt.cursor_row,
        cursor_col: 2 + prompt.cursor_col,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ClawPromptContent {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

#[allow(dead_code)]
fn claw_prompt_content(model: &TerminalFrameModel, cursor: usize) -> ClawPromptContent {
    let prefix = "> ";
    if model.composer.is_empty() {
        let placeholder = model.language.text(
            "描述要改的代码、错误或研究任务",
            "Describe the change, bug, or research task",
        );
        return ClawPromptContent {
            lines: vec![format!("{prefix}\x1b[2m{placeholder}\x1b[0m")],
            cursor_row: 0,
            cursor_col: visible_width(prefix),
        };
    }
    let cursor = clamp_to_char_boundary(&model.composer, cursor);
    let before_cursor = &model.composer[..cursor];
    let cursor_row = before_cursor.matches('\n').count();
    let cursor_col_in_line = before_cursor
        .rsplit_once('\n')
        .map_or(before_cursor, |(_, tail)| tail);
    let cursor_col = visible_width(if cursor_row == 0 { prefix } else { "  " })
        + UnicodeWidthStr::width(cursor_col_in_line);
    let lines = model
        .composer
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{prefix}{line}")
            } else {
                format!("  {line}")
            }
        })
        .collect();
    ClawPromptContent {
        lines,
        cursor_row,
        cursor_col,
    }
}

#[allow(dead_code)]
fn render_rich_inline_running_turn_card(
    state: &TuiInteractionState,
    prompt: &str,
    width: usize,
) -> String {
    let width = width.clamp(20, 132);
    let mut lines = Vec::new();
    lines.push(top_border(width));
    lines.push(row(
        width,
        &format!(
            "{}  {}",
            accent("Astra"),
            state
                .language
                .text("正在执行回合", "running the current turn")
        ),
    ));
    lines.push(row(
        width,
        &format!(
            "{} {}",
            state.language.text("任务", "prompt"),
            truncate_plain(prompt, 72)
        ),
    ));
    lines.push(row(
        width,
        state.language.text(
            "流式输出中；可按 Esc/Ctrl-C，或直接输入 /exc 中断",
            "Streaming output; press Esc/Ctrl-C, or type /exc to interrupt",
        ),
    ));
    lines.push(bottom_border(width));
    lines.join("\n")
}

fn render_inline_turn_status_line(
    state: &TuiInteractionState,
    prompt: &str,
    frame_index: usize,
    turn_start: Instant,
) -> String {
    render_inline_turn_status_line_with_width(
        state,
        prompt,
        frame_index,
        turn_start.elapsed().as_secs(),
        inline_status_line_width(),
    )
}

fn render_inline_turn_status_line_with_width(
    state: &TuiInteractionState,
    prompt: &str,
    frame_index: usize,
    elapsed_secs: u64,
    width: usize,
) -> String {
    let frame = INLINE_SPINNER_FRAMES[frame_index % INLINE_SPINNER_FRAMES.len()];
    let breathing = breathing_progress(frame_index);
    let bar = render_mini_progress_bar(breathing);
    let timer = format!("{elapsed_secs}s");
    let prompt_prefix = format!(
        "\x1b[38;2;212;136;10m{frame} Astra\x1b[0m \x1b[2m{} \x1b[38;2;212;136;10m{bar}\x1b[0m \x1b[38;2;212;136;10m{timer}\x1b[0m \x1b[2m· {} ",
        state.language.text("思考中...", "Thinking..."),
        state.language.text("任务", "prompt"),
    );
    let suffix = format!(
        " · Esc {}\x1b[0m",
        state.language.text("或 /exc", "or /exc")
    );
    let fixed_width = visible_width(&prompt_prefix) + visible_width(&suffix);
    let prompt_width = width.saturating_sub(fixed_width);
    let prompt = fit_display_width(&single_line_plain(prompt), prompt_width);
    let line = format!("{prompt_prefix}{prompt}{suffix}");
    if visible_width(&line) <= width {
        line
    } else {
        fit_display_width(&strip_ansi(&line), width)
    }
}

fn breathing_progress(frame_index: usize) -> usize {
    let phase = frame_index % 20;
    match phase {
        0..=4 => 2,
        5..=9 => 3,
        10..=14 => 4,
        15..=19 => 3,
        _ => 2,
    }
}

fn render_mini_progress_bar(filled: usize) -> String {
    let filled = filled.min(10);
    format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
}

fn inline_status_line_width() -> usize {
    terminal::size()
        .map(|(cols, _)| usize::from(cols).saturating_sub(1).clamp(1, 96))
        .unwrap_or(79)
}

fn redraw_inline_turn_status_line(
    state: &TuiInteractionState,
    prompt: &str,
    frame_index: usize,
    turn_start: Instant,
) -> io::Result<()> {
    print_terminal_control_text(&format!(
        "\r\x1b[2K{}",
        render_inline_turn_status_line(state, prompt, frame_index, turn_start)
    ))
}

fn clear_inline_turn_status_line() -> io::Result<()> {
    print_terminal_control_text("\r\x1b[2K")
}

fn run_non_tty_inline_repl_with_action_executors<F, C, P>(
    result: &TuiLaunchResult,
    executor: Arc<Mutex<F>>,
    config_executor: Arc<Mutex<C>>,
    permission_executor: Arc<Mutex<P>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let Some(input) = read_non_tty_repl_submission_from_reader(io::stdin().lock())? else {
        return Ok(render_inline_snapshot(result, "degraded_non_tty"));
    };
    if typed_exit_requested(&input) {
        return Ok(
            "Astra Code inline REPL 已退出\nraw_mode: stdin_inline_repl\nexit_key: slash-exit"
                .to_string(),
        );
    }

    let mut state = TuiInteractionState::from_result(result);
    let output = run_non_tty_inline_repl_submission(
        &mut state,
        &input,
        &executor,
        &config_executor,
        &permission_executor,
    )?;
    if output.trim().is_empty() {
        Ok(
            "Astra Code inline REPL 已退出\nraw_mode: stdin_inline_repl\nexit_key: stdin-eof"
                .to_string(),
        )
    } else {
        Ok(output)
    }
}

fn read_non_tty_repl_submission_from_reader<R: Read>(mut reader: R) -> io::Result<Option<String>> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    Ok(non_tty_repl_submission_from_text(&buffer))
}

fn non_tty_repl_submission_from_text(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn run_non_tty_inline_repl_submission<F, C, P>(
    state: &mut TuiInteractionState,
    input: &str,
    executor: &Arc<Mutex<F>>,
    config_executor: &Arc<Mutex<C>>,
    permission_executor: &Arc<Mutex<P>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    if let Some(response) =
        apply_language_command(state, input).or_else(|| apply_theme_command(state, input))
    {
        return Ok(response);
    }

    match prompt_payload_or_plain_text(input) {
        Some(Ok(prompt)) => {
            push_prompt_history_entry(state, &prompt);
            run_non_tty_inline_repl_prompt_turn(&prompt, executor)
        }
        Some(Err(message)) => Ok(message),
        None => Ok(route_typed_command_mut_with_executors(
            state,
            input,
            &mut |action| {
                config_executor
                    .lock()
                    .map_err(|_| "TUI configuration executor lock poisoned".to_string())
                    .and_then(|mut locked| (*locked)(action))
            },
            &mut |action| {
                permission_executor
                    .lock()
                    .map_err(|_| "TUI permission executor lock poisoned".to_string())
                    .and_then(|mut locked| (*locked)(action))
            },
        )),
    }
}

fn run_non_tty_inline_repl_prompt_turn<F>(
    prompt: &str,
    executor: &Arc<Mutex<F>>,
) -> io::Result<String>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let stream_sender = TuiStreamSender { sender };
    let (cancel_token, _interrupt_handle) = runtime_interrupt_pair();
    let result = executor
        .lock()
        .map_err(|_| io::Error::other("TUI executor lock poisoned"))
        .and_then(|mut locked| {
            (*locked)(prompt, stream_sender, cancel_token).map_err(io::Error::other)
        });

    let mut output = String::new();
    let mut streamed_delta_received = false;
    for event in receiver.try_iter() {
        match event {
            TuiTurnEvent::Delta(delta) => {
                if !delta.is_empty() {
                    streamed_delta_received = true;
                }
                output.push_str(&delta);
            }
            TuiTurnEvent::ToolCallStarted { tool_name, call_id } => {
                output.push_str(&format!(
                    "\n  ▸ {}\n",
                    tool_event_label(&tool_name, &call_id)
                ));
            }
            TuiTurnEvent::ToolResultReady {
                tool_name,
                call_id,
                status,
            } => {
                output.push_str(&format!(
                    "  ✓ {}: {status}\n",
                    tool_event_label(&tool_name, &call_id)
                ));
            }
            TuiTurnEvent::Complete(_) => {}
        }
    }

    match result {
        Ok(execution) => {
            let completion_body =
                inline_completion_body_after_streaming(&execution, streamed_delta_received);
            append_plain_repl_output(&mut output, &completion_body);
        }
        Err(message) => append_plain_repl_output(&mut output, &format!("提示失败：{message}")),
    }
    Ok(output.trim_end().to_string())
}

fn push_prompt_history_entry(state: &mut TuiInteractionState, prompt: &str) {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0);
    state.prompt_history.push(PromptHistoryEntry {
        timestamp_ms,
        text: prompt.to_string(),
    });
}

fn push_input_history_entry(state: &mut TuiInteractionState, input: &str) {
    let input = input.trim();
    if input.is_empty() {
        return;
    }
    if state.input_history.last().is_none_or(|last| last != input) {
        state.input_history.push(input.to_string());
    }
}

fn append_plain_repl_output(output: &mut String, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(body.trim_end());
}

fn repl_completion_candidates(
    result: &TuiLaunchResult,
    state: &TuiInteractionState,
) -> Vec<String> {
    let mut dynamic = vec![
        "/model gpt-5.5".to_string(),
        "/model gpt-5.4".to_string(),
        "/model gpt-5.4-mini".to_string(),
        "/reasoning auto".to_string(),
        "/reasoning low".to_string(),
        "/reasoning medium".to_string(),
        "/reasoning high".to_string(),
        "/language zh".to_string(),
        "/language en".to_string(),
        "/theme light".to_string(),
        "/theme dark".to_string(),
        "/resume latest".to_string(),
    ];
    if !state.model_label.trim().is_empty() && state.model_label != "auto" {
        dynamic.push(format!("/model {}", state.model_label));
    }
    ReplCompletionCatalog::new(
        &result.inline_view.command_model,
        &result.inline_view.skill_model,
        dynamic,
    )
    .candidates()
}

fn claw_line_editor_prompt(_language: TuiLanguage) -> String {
    "\x1b[2m╰─\x1b[0m \x1b[38;5;208m>\x1b[0m ".to_string()
}

fn render_claw_line_editor_context(state: &TuiInteractionState) -> String {
    let research_hint = if state.research_line.trim().is_empty() {
        state
            .language
            .text("research: 未绑定", "research: unbound")
            .to_string()
    } else {
        format!(
            "research: {}",
            truncate_plain(&strip_ansi(&state.research_line), 64)
        )
    };
    format!(
        "\x1b[2m╭─\x1b[0m \x1b[38;5;208mAstra\x1b[0m \x1b[2m{} · {} · {}\x1b[0m\n\x1b[2m│\x1b[0m {}\n\x1b[2m│\x1b[0m \x1b[2m{}\x1b[0m",
        state.language.text("输入", "input"),
        compact_permission_mode(&state.permission_mode, state.language),
        truncate_plain(&state.model_label, 24),
        state.language.text(
            "直接输入；Tab 补全；/research board 看板；Esc 清空；Ctrl-C 中断；/exit 退出",
            "Type directly; Tab completes; /research board opens board; Esc clears; Ctrl-C interrupts; /exit exits",
        ),
        research_hint
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InlineReplBannerContext {
    model: String,
    permission: String,
    branch: String,
    workspace: String,
    directory: String,
    research: String,
}

fn render_inline_repl_banner(result: &TuiLaunchResult, state: &TuiInteractionState) -> String {
    render_inline_repl_banner_from_context(&inline_repl_banner_context(result, state))
}

fn inline_repl_banner_context(
    _result: &TuiLaunchResult,
    state: &TuiInteractionState,
) -> InlineReplBannerContext {
    let research_line = research_info_segments_from_line(&state.research_line)
        .into_iter()
        .take(4)
        .collect::<Vec<_>>()
        .join(" · ");
    InlineReplBannerContext {
        model: state.model_label.clone(),
        permission: state.permission_mode.clone(),
        branch: inline_repl_git_branch(),
        workspace: inline_repl_workspace_summary(),
        directory: inline_repl_directory_label(),
        research: if research_line.is_empty() {
            "未绑定研究线程".to_string()
        } else {
            research_line
        },
    }
}

fn render_inline_repl_banner_from_context(context: &InlineReplBannerContext) -> String {
    let info = format!(
        "{} via {} · {} · {} · {}",
        context.model,
        inline_repl_provider_label(&context.model),
        context.permission,
        context.branch,
        context.workspace
    );
    let amber = "\x1b[38;2;212;136;10m";
    let reset = "\x1b[0m";
    let dim = "\x1b[2m";
    let bold = "\x1b[1m";
    let content = [
        format!("{amber}◉{reset} {bold}Astra Code{reset}"),
        format!("{dim}{info}{reset}"),
        format!("{dim}cwd {}{reset}", truncate_plain(&context.directory, 56)),
        format!(
            "{dim}research {}{reset}",
            truncate_plain(&context.research, 64)
        ),
        format!("{dim}/help 命令 · /research board 看板 · $list 技能 · /exit 退出{reset}"),
    ];
    let inner = content
        .iter()
        .map(|line| visible_width(line))
        .max()
        .unwrap_or(42)
        .clamp(42, 88);
    let horizontal = "─".repeat(inner + 2);
    let rows = content
        .iter()
        .map(|line| {
            format!(
                "{amber}│{reset} {} {amber}│{reset}",
                pad(&fit(line, inner), inner)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{amber}╭{horizontal}╮{reset}\n{rows}\n{amber}╰{horizontal}╯{reset}")
}

fn inline_repl_directory_label() -> String {
    env::current_dir().map_or_else(
        |_| "<unknown>".to_string(),
        |path| path.display().to_string(),
    )
}

fn inline_repl_git_branch() -> String {
    process_output(["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn inline_repl_workspace_summary() -> String {
    let Some(status) = process_output(["status", "--short"]) else {
        return "unknown".to_string();
    };
    let changed = status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if changed == 0 {
        "clean".to_string()
    } else if changed == 1 {
        "1 change".to_string()
    } else {
        format!("{changed} changes")
    }
}

fn process_output<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = ProcessCommand::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn inline_repl_provider_label(model: &str) -> &'static str {
    let lower = model.to_ascii_lowercase();
    if lower.contains("claude") {
        "anthropic"
    } else if lower.contains("grok") {
        "xai"
    } else if lower.contains("gpt") || lower.contains("openai") {
        "openai"
    } else if lower.contains("local") {
        "local"
    } else {
        "configured provider"
    }
}

fn research_info_segments_from_line(line: &str) -> Vec<String> {
    line.split('|')
        .filter_map(|segment| {
            let compact = segment.trim();
            if compact.is_empty() || compact.ends_with(" none") {
                None
            } else {
                Some(compact.to_string())
            }
        })
        .collect()
}

fn run_inline_repl_submission<F, C, P>(
    state: &mut TuiInteractionState,
    input: &str,
    executor: &Arc<Mutex<F>>,
    config_executor: &Arc<Mutex<C>>,
    permission_executor: &Arc<Mutex<P>>,
    renderer: &TerminalMarkdownRenderer,
) -> io::Result<()>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    if let Some(response) =
        apply_language_command(state, input).or_else(|| apply_theme_command(state, input))
    {
        print_rendered_repl_response(renderer, &response, state.language, state.output_folded)?;
        return Ok(());
    }

    match prompt_payload_or_plain_text(input) {
        Some(Ok(prompt)) => {
            push_prompt_history_entry(state, &prompt);
            run_inline_repl_prompt_turn(state, &prompt, executor, renderer)?;
        }
        Some(Err(message)) => {
            print_rendered_repl_response(renderer, &message, state.language, state.output_folded)?;
        }
        None => {
            let response = route_typed_command_mut_with_executors(
                state,
                input,
                &mut |action| {
                    config_executor
                        .lock()
                        .map_err(|_| "TUI configuration executor lock poisoned".to_string())
                        .and_then(|mut locked| (*locked)(action))
                },
                &mut |action| {
                    permission_executor
                        .lock()
                        .map_err(|_| "TUI permission executor lock poisoned".to_string())
                        .and_then(|mut locked| (*locked)(action))
                },
            );
            print_rendered_repl_response(renderer, &response, state.language, state.output_folded)?;
        }
    }
    Ok(())
}

fn run_inline_repl_prompt_turn<F>(
    state: &mut TuiInteractionState,
    prompt: &str,
    executor: &Arc<Mutex<F>>,
    renderer: &TerminalMarkdownRenderer,
) -> io::Result<()>
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let stream_sender = TuiStreamSender {
        sender: sender.clone(),
    };
    let (cancel_token, interrupt_handle) = runtime_interrupt_pair();
    let _interrupt_guard = InlineReplInterruptGuard::install()?;
    let _raw_mode = InlineRawModeGuard::enter()?;
    let mut spinner_frame = 0;
    let mut last_spinner_tick = Instant::now();
    let turn_start = Instant::now();
    let mut status_line_visible = true;
    redraw_inline_turn_status_line(state, prompt, spinner_frame, turn_start)?;
    let prompt_for_thread = prompt.to_string();
    let stream_sender_for_thread = stream_sender.clone();
    let executor = Arc::clone(executor);
    thread::spawn(move || {
        let result = executor
            .lock()
            .map_err(|_| "TUI executor lock poisoned".to_string())
            .and_then(|mut locked| {
                (*locked)(&prompt_for_thread, stream_sender_for_thread, cancel_token)
            });
        let _ = sender.send(TuiTurnEvent::Complete(result));
    });
    let mut stream = MarkdownStreamState::default();
    let mut streamed_delta_received = false;
    let mut typed_interrupt_buffer = String::new();
    loop {
        if poll_inline_turn_interrupt(&interrupt_handle, &mut typed_interrupt_buffer)?
            || interrupt_if_requested(&interrupt_handle, &INLINE_REPL_SIGINT_REQUESTED)
        {
            if status_line_visible {
                clear_inline_turn_status_line()?;
            }
            if let Some(rendered) = stream.finish(renderer) {
                print_terminal_text(&rendered)?;
            }
            print_rendered_repl_response_raw(
                renderer,
                state
                    .language
                    .text("已请求中断当前回合。", "Prompt cancelled."),
                state.language,
                state.output_folded,
            )?;
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(TuiTurnEvent::Delta(delta)) => {
                if status_line_visible {
                    clear_inline_turn_status_line()?;
                    status_line_visible = false;
                }
                if !delta.is_empty() {
                    streamed_delta_received = true;
                }
                if let Some(rendered) = stream.push(renderer, &delta) {
                    print_terminal_text(&rendered)?;
                }
            }
            Ok(TuiTurnEvent::ToolCallStarted { tool_name, call_id }) => {
                if status_line_visible {
                    clear_inline_turn_status_line()?;
                    status_line_visible = false;
                }
                print_terminal_text(&format!(
                    "\n  ▸ {}\n",
                    tool_event_label(&tool_name, &call_id)
                ))?;
            }
            Ok(TuiTurnEvent::ToolResultReady {
                tool_name,
                call_id,
                status,
            }) => {
                print_terminal_text(&format!(
                    "  ✓ {}: {status}\n",
                    tool_event_label(&tool_name, &call_id)
                ))?;
            }
            Ok(TuiTurnEvent::Complete(result)) => {
                if status_line_visible {
                    clear_inline_turn_status_line()?;
                }
                if let Some(rendered) = stream.finish(renderer) {
                    print_terminal_text(&rendered)?;
                }
                match result {
                    Ok(execution) => {
                        let completion_body = inline_completion_body_after_streaming(
                            &execution,
                            streamed_delta_received,
                        );
                        if !completion_body.trim().is_empty() {
                            print_rendered_repl_response_raw(
                                renderer,
                                &completion_body,
                                state.language,
                                state.output_folded,
                            )?;
                        }
                    }
                    Err(message) => {
                        print_terminal_text(&format!("提示失败：{message}\n"))?;
                    }
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if status_line_visible {
                    clear_inline_turn_status_line()?;
                }
                if let Some(rendered) = stream.finish(renderer) {
                    print_terminal_text(&rendered)?;
                }
                print_terminal_text("提示失败：执行线程已断开\n")?;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if status_line_visible && last_spinner_tick.elapsed() >= INLINE_SPINNER_TICK {
                    spinner_frame = spinner_frame.wrapping_add(1);
                    redraw_inline_turn_status_line(state, prompt, spinner_frame, turn_start)?;
                    last_spinner_tick = Instant::now();
                }
            }
        }
    }
    Ok(())
}

struct InlineReplInterruptGuard {
    previous_handler: libc::sighandler_t,
}

impl InlineReplInterruptGuard {
    fn install() -> io::Result<Self> {
        INLINE_REPL_SIGINT_REQUESTED.store(false, Ordering::SeqCst);
        let previous_handler = unsafe {
            libc::signal(
                libc::SIGINT,
                inline_repl_sigint_handler as *const () as libc::sighandler_t,
            )
        };
        if previous_handler == libc::SIG_ERR {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self { previous_handler })
        }
    }
}

impl Drop for InlineReplInterruptGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.previous_handler);
        }
        INLINE_REPL_SIGINT_REQUESTED.store(false, Ordering::SeqCst);
    }
}

fn interrupt_if_requested(
    interrupt_handle: &RuntimeInterruptHandle,
    requested: &AtomicBool,
) -> bool {
    if requested.swap(false, Ordering::SeqCst) {
        interrupt_handle.interrupt();
        true
    } else {
        false
    }
}

fn poll_inline_turn_interrupt(
    interrupt_handle: &RuntimeInterruptHandle,
    typed_buffer: &mut String,
) -> io::Result<bool> {
    while event::poll(Duration::from_millis(0))? {
        match event::read()? {
            Event::Paste(text)
                if apply_inline_turn_interrupt_text(interrupt_handle, typed_buffer, &text) =>
            {
                return Ok(true);
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => {
                    interrupt_handle.interrupt();
                    return Ok(true);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    interrupt_handle.interrupt();
                    return Ok(true);
                }
                KeyCode::Enter => {
                    if typed_interrupt_alias(typed_buffer) {
                        interrupt_handle.interrupt();
                        typed_buffer.clear();
                        return Ok(true);
                    }
                    typed_buffer.clear();
                }
                KeyCode::Backspace => {
                    typed_buffer.pop();
                }
                KeyCode::Char(ch)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                        && apply_inline_turn_interrupt_text(
                            interrupt_handle,
                            typed_buffer,
                            &ch.to_string(),
                        ) =>
                {
                    return Ok(true);
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(false)
}

fn apply_inline_turn_interrupt_text(
    interrupt_handle: &RuntimeInterruptHandle,
    typed_buffer: &mut String,
    text: &str,
) -> bool {
    typed_buffer.push_str(text);
    let trimmed = typed_buffer.trim();
    if typed_interrupt_alias(trimmed) {
        interrupt_handle.interrupt();
        typed_buffer.clear();
        return true;
    }
    let still_possible = ["/exc", "/interrupt"]
        .iter()
        .any(|command| command.starts_with(trimmed));
    if !still_possible || typed_buffer.len() > "/interrupt".len() {
        typed_buffer.clear();
    }
    false
}

fn typed_interrupt_alias(input: &str) -> bool {
    matches!(input.trim(), "/exc" | "/interrupt")
}

fn inline_completion_body_after_streaming(
    execution: &TuiCommandExecution,
    streamed_delta_received: bool,
) -> String {
    if streamed_delta_received {
        return String::new();
    }

    strip_inline_runtime_metadata(&execution.body)
}

fn final_transcript_body_after_streaming(
    execution: &TuiCommandExecution,
    streamed_body: &str,
) -> String {
    let final_body = strip_inline_runtime_metadata(&execution.body);
    if final_body.trim().is_empty() {
        streamed_body.to_string()
    } else {
        final_body
    }
}

fn strip_inline_runtime_metadata(body: &str) -> String {
    body.lines()
        .filter(|line| !is_inline_runtime_metadata_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_inline_runtime_metadata_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("session ")
        || trimmed.starts_with("research ")
        || trimmed == "Prompt completed."
        || trimmed.starts_with("Prompt completed ")
}

fn print_rendered_repl_response(
    renderer: &TerminalMarkdownRenderer,
    response: &str,
    language: TuiLanguage,
    folded: bool,
) -> io::Result<()> {
    let response = strip_inline_runtime_metadata(response);
    let rendered = render_inline_repl_response_text(renderer, &response, language, folded);
    if rendered.trim().is_empty() {
        return Ok(());
    }
    println!("\x1b[2mAstra\x1b[0m");
    println!("{rendered}");
    Ok(())
}

fn print_rendered_repl_response_raw(
    renderer: &TerminalMarkdownRenderer,
    response: &str,
    language: TuiLanguage,
    folded: bool,
) -> io::Result<()> {
    let response = strip_inline_runtime_metadata(response);
    let rendered = render_inline_repl_response_text(renderer, &response, language, folded);
    if rendered.trim().is_empty() {
        return Ok(());
    }
    print_terminal_text(&format!("\x1b[2mAstra\x1b[0m\n{rendered}\n"))
}

struct ActiveTuiTerminal {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    raw_mode_enabled: bool,
    alternate_screen_enabled: bool,
}

impl ActiveTuiTerminal {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(
            stdout,
            SetTitle("Astra Code full-screen TUI"),
            EnterAlternateScreen
        ) {
            let _ = disable_raw_mode();
            return Err(err);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self {
                terminal,
                raw_mode_enabled: true,
                alternate_screen_enabled: true,
            }),
            Err(err) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                Err(err)
            }
        }
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    fn restore(&mut self) -> io::Result<()> {
        let mut first_error = None;
        if let Err(err) = self.terminal.show_cursor() {
            first_error.get_or_insert(err);
        }
        if self.alternate_screen_enabled {
            if let Err(err) = execute!(self.terminal.backend_mut(), LeaveAlternateScreen) {
                first_error.get_or_insert(err);
            }
            self.alternate_screen_enabled = false;
        }
        if self.raw_mode_enabled {
            if let Err(err) = disable_raw_mode() {
                first_error.get_or_insert(err);
            }
            self.raw_mode_enabled = false;
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(())
        }
    }
}

impl Drop for ActiveTuiTerminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TuiCommandExecution {
    pub body: String,
    pub iterations: Option<usize>,
    pub tool_calls_made: Option<usize>,
}

impl TuiCommandExecution {
    pub fn new(body: String) -> Self {
        Self {
            body,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct TuiStreamSender {
    sender: mpsc::Sender<TuiTurnEvent>,
}

impl TuiStreamSender {
    pub fn send_delta(&self, delta: &str) {
        let _ = self.sender.send(TuiTurnEvent::Delta(delta.to_string()));
    }

    pub fn send_tool_call_started(&self, tool_name: &str, call_id: &str) {
        let _ = self.sender.send(TuiTurnEvent::ToolCallStarted {
            tool_name: tool_name.to_string(),
            call_id: call_id.to_string(),
        });
    }

    pub fn send_tool_result(&self, tool_name: &str, call_id: &str, status: &str) {
        let _ = self.sender.send(TuiTurnEvent::ToolResultReady {
            tool_name: tool_name.to_string(),
            call_id: call_id.to_string(),
            status: status.to_string(),
        });
    }
}

fn read_crossterm_input() -> io::Result<Vec<u8>> {
    loop {
        match event::read()? {
            Event::Paste(text) => return Ok(text.into_bytes()),
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') | KeyCode::Char('C') => return Ok(vec![3]),
                        KeyCode::Char('d') | KeyCode::Char('D') => return Ok(vec![4]),
                        _ => return Ok(Vec::new()),
                    }
                }
                return Ok(match key.code {
                    KeyCode::Enter => vec![b'\n'],
                    KeyCode::Backspace => vec![127],
                    KeyCode::Esc => vec![27],
                    KeyCode::Up => vec![27, b'[', b'A'],
                    KeyCode::Down => vec![27, b'[', b'B'],
                    KeyCode::Char(ch) => ch.to_string().into_bytes(),
                    _ => Vec::new(),
                });
            }
            _ => {}
        }
    }
}

fn render_inline_snapshot(result: &TuiLaunchResult, raw_mode: &str) -> String {
    let model = terminal_frame_model(result);
    let mut lines = Vec::new();
    lines.push("Astra Code 代码智能体".to_string());
    lines.push(format!("raw_mode: {raw_mode}"));
    lines.push("对话优先的代码智能体".to_string());
    lines.push(format!(
        "状态：项目 {} | 远程 {} | 权限 {} | 主题 {}",
        model.project_id,
        model.remote_state,
        model.permission_mode,
        model.theme_mode.label(model.language)
    ));
    lines.push(format!(
        "研究：{}",
        combined_research_line(&result.projection.research)
    ));
    lines.push(
        "输入：直接输入对话；输入：直接描述要改的代码、错误或实验目标；/ 打开命令，$ 打开技能。"
            .to_string(),
    );
    lines.push("命令：/help /prompt /model /language /theme /sessions /permissions /approve <request-id> /deny <request-id> /terminal /research /exit".to_string());
    lines.push("技能：$list 浏览，$skill-name 查看，$skill-name <input> 暂存运行。".to_string());
    lines.push(format!("按键：{}", render_keys(result)));
    lines.join("\n")
}

fn tui_title(result: &TuiLaunchResult) -> &'static str {
    if result.launch_mode == "fullscreen_split_pane_projection_renderer" {
        "Astra Code full-screen TUI"
    } else {
        "Astra Code inline REPL"
    }
}

fn render_keys(result: &TuiLaunchResult) -> String {
    let bindings = result
        .inline_view
        .keymap
        .iter()
        .map(|binding| format!("{} {}", binding.key, binding.action_id))
        .collect::<Vec<_>>()
        .join(" | ");
    bindings
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct TerminalSize {
    cols: u16,
    rows: u16,
}

#[derive(Debug)]
struct TerminalFrameModel {
    #[cfg(test)]
    title: String,
    project_id: String,
    remote_state: String,
    session_count: String,
    permission_count: String,
    permission_mode: String,
    model_label: String,
    reasoning_effort: String,
    working_dir: String,
    git_branch: String,
    research_line: String,
    transcript: Vec<TranscriptTurn>,
    composer: String,
    output_folded: bool,
    key_hints: Vec<String>,
    skill_entries: Vec<String>,
    command_entries: Vec<TuiCommandCard>,
    language: TuiLanguage,
    theme_mode: TuiThemeMode,
    overlay_selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptTurn {
    role: &'static str,
    body: String,
}

#[allow(dead_code)]
struct TuiInteractionState {
    composer: String,
    transcript: Vec<TranscriptTurn>,
    skill_entries: Vec<String>,
    skill_descriptions: Vec<(String, String)>,
    skill_sources: Vec<(String, String)>,
    command_entries: Vec<TuiCommandCard>,
    active_session_id: Option<String>,
    recent_sessions: Vec<TuiSessionEntry>,
    prompt_history: Vec<PromptHistoryEntry>,
    input_history: Vec<String>,
    research_line: String,
    session_count: String,
    permission_count: String,
    permission_mode: String,
    model_label: String,
    reasoning_effort: String,
    working_dir: String,
    git_branch: String,
    remote_state: String,
    running_turn: Option<TuiRunningTurn>,
    language: TuiLanguage,
    theme_mode: TuiThemeMode,
    overlay_selected: usize,
    output_folded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiConfigAction {
    pub kind: TuiConfigActionKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiConfigActionKind {
    Model,
    Reasoning,
    Session(TuiSessionActionKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiSessionActionKind {
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiConfigActionResult {
    pub applied: bool,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub session_id: Option<String>,
    pub scope: String,
    pub message: String,
}

impl TuiConfigActionResult {
    pub fn session_resumed(session_id: String, scope: String, message: String) -> Self {
        Self {
            applied: true,
            provider_id: None,
            model: None,
            reasoning_effort: None,
            session_id: Some(session_id),
            scope,
            message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiPermissionAction {
    pub decision: TuiPermissionDecision,
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiPermissionDecision {
    Approve,
    Deny,
}

impl TuiPermissionDecision {
    pub fn as_command(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiPermissionActionResult {
    pub request_id: String,
    pub decision: String,
    pub pending_count: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiLanguage {
    Zh,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiThemeMode {
    Day,
    Night,
}

#[derive(Debug, Clone, Copy)]
struct TuiThemePalette {
    surface: Color,
    panel: Color,
    panel_alt: Color,
    text: Color,
    muted: Color,
    faint: Color,
    border: Color,
    accent: Color,
    success: Color,
    danger: Color,
}

impl TuiThemeMode {
    fn label(self, language: TuiLanguage) -> &'static str {
        match (self, language) {
            (Self::Day, TuiLanguage::Zh) => "白天",
            (Self::Day, TuiLanguage::En) => "light",
            (Self::Night, TuiLanguage::Zh) => "夜间",
            (Self::Night, TuiLanguage::En) => "dark",
        }
    }

    fn palette(self) -> TuiThemePalette {
        match self {
            Self::Day => TuiThemePalette {
                surface: Color::Rgb(244, 244, 248),
                panel: Color::Rgb(255, 255, 255),
                panel_alt: Color::Rgb(234, 234, 238),
                text: Color::Rgb(24, 24, 27),
                muted: Color::Rgb(113, 113, 122),
                faint: Color::Rgb(161, 161, 170),
                border: Color::Rgb(212, 212, 216),
                accent: Color::Rgb(180, 83, 9),
                success: Color::Rgb(56, 135, 74),
                danger: Color::Rgb(177, 67, 64),
            },
            Self::Night => TuiThemePalette {
                surface: Color::Rgb(14, 14, 18),
                panel: Color::Rgb(20, 20, 26),
                panel_alt: Color::Rgb(28, 28, 36),
                text: Color::Rgb(200, 196, 188),
                muted: Color::Rgb(107, 107, 120),
                faint: Color::Rgb(74, 74, 86),
                border: Color::Rgb(34, 34, 48),
                accent: Color::Rgb(212, 136, 10),
                success: Color::Rgb(108, 184, 103),
                danger: Color::Rgb(224, 92, 83),
            },
        }
    }
}

impl TuiLanguage {
    fn text(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TuiCommandCard {
    typed: String,
    action_id: String,
    label: String,
    gate: String,
    category: String,
    summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TuiSessionEntry {
    session_id: String,
    title: Option<String>,
    updated_at: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptHistoryEntry {
    timestamp_ms: u64,
    text: String,
}

struct TuiRunningTurn {
    response_index: usize,
    prompt: String,
    frame: usize,
    streamed_body: String,
    interrupt_handle: RuntimeInterruptHandle,
    #[cfg(test)]
    stream_sender: TuiStreamSender,
    receiver: mpsc::Receiver<TuiTurnEvent>,
}

impl TuiRunningTurn {
    #[cfg(test)]
    fn delta_sender(&self) -> TuiStreamSender {
        self.stream_sender.clone()
    }
}

enum TuiTurnEvent {
    Delta(String),
    ToolCallStarted {
        tool_name: String,
        call_id: String,
    },
    ToolResultReady {
        tool_name: String,
        call_id: String,
        status: String,
    },
    Complete(Result<TuiCommandExecution, String>),
}

fn tool_event_label(tool_name: &str, call_id: &str) -> String {
    if call_id.trim().is_empty() {
        return tool_name.to_string();
    }
    let short_id: String = call_id.chars().take(12).collect();
    format!("{tool_name} #{short_id}")
}

#[derive(Debug, PartialEq, Eq)]
enum TuiInputOutcome {
    Continue,
    Exit(&'static str),
}

fn terminal_frame_model(result: &TuiLaunchResult) -> TerminalFrameModel {
    let view = &result.inline_view;
    TerminalFrameModel {
        #[cfg(test)]
        title: tui_title(result).to_string(),
        project_id: result.projection.project_id.clone(),
        remote_state: status_badge(view, "Remote").unwrap_or_else(|| "unknown".to_string()),
        session_count: status_badge(view, "Sessions").unwrap_or_else(|| "0".to_string()),
        permission_count: result.projection.permissions.pending_count.to_string(),
        permission_mode: status_badge(view, "Permission Mode")
            .unwrap_or_else(|| "read-only".to_string()),
        model_label: status_badge(view, "Model").unwrap_or_else(|| "auto".to_string()),
        reasoning_effort: status_badge(view, "Reasoning").unwrap_or_else(|| "auto".to_string()),
        working_dir: inline_repl_directory_label(),
        git_branch: inline_repl_git_branch(),
        research_line: combined_research_line(&result.projection.research),
        transcript: Vec::new(),
        composer: String::new(),
        key_hints: vec![
            "Enter 发送".to_string(),
            "/ 命令".to_string(),
            "$ 技能".to_string(),
            "/language en".to_string(),
            "/terminal 终端".to_string(),
            "Ctrl-D 关闭".to_string(),
        ],
        command_entries: view
            .command_model
            .groups
            .iter()
            .flat_map(|group| &group.commands)
            .map(|entry| TuiCommandCard {
                typed: entry.typed.clone(),
                action_id: entry.action_id.clone(),
                label: entry.label.clone(),
                gate: entry.gate.clone(),
                category: command_category_for_action(&entry.action_id),
                summary: entry.summary.clone(),
            })
            .collect(),
        skill_entries: view
            .skill_model
            .entries
            .iter()
            .map(|entry| entry.typed.clone())
            .collect(),
        language: TuiLanguage::Zh,
        theme_mode: TuiThemeMode::Day,
        overlay_selected: 0,
        output_folded: false,
    }
}

fn status_badge(view: &TuiInlineView, label: &str) -> Option<String> {
    view.status_hud
        .iter()
        .find(|badge| badge.label == label)
        .map(|badge| badge.state.clone())
}

fn command_spec_for_action(action_id: &str) -> Option<SurfaceCommandSpec> {
    product_command_specs()
        .into_iter()
        .find(|spec| spec.action_id == action_id)
}

fn command_category_for_action(action_id: &str) -> String {
    command_spec_for_action(action_id)
        .map(|spec| spec.category)
        .unwrap_or_else(|| "work".to_string())
}

fn research_summary_line(research: &HostResearchSummary) -> String {
    if !research.compact_research_line.trim().is_empty() {
        return research.compact_research_line.clone();
    }
    let list_segment = |items: &[String]| {
        if items.is_empty() {
            "none".to_string()
        } else {
            items
                .iter()
                .map(|item| compact_segment_value(item))
                .collect::<Vec<_>>()
                .join(",")
        }
    };
    format!(
        "status {} | thread_id {} | thread_title {} | stage {} | class {} | mode {} | confidence {} | pending {} | open {} | decisions {} | evidence {} | next {}",
        research.status,
        research.active_thread_id.as_deref().unwrap_or("none"),
        research
            .active_thread_title
            .as_deref()
            .map(compact_segment_value)
            .unwrap_or_else(|| "none".to_string()),
        research.active_stage_id.as_deref().unwrap_or("none"),
        research.active_stage_class.as_deref().unwrap_or("none"),
        research.active_deliberation_mode.as_deref().unwrap_or("none"),
        research.confidence.as_deref().unwrap_or("none"),
        list_segment(&research.pending_operations),
        list_segment(&research.open_questions),
        list_segment(&research.agreed_decisions),
        list_segment(&research.evidence_refs),
        compact_segment_value(&research.next_recommended_action)
    )
}

fn combined_research_line(research: &HostResearchSummary) -> String {
    let mut line = research_summary_line(research);
    let board_segment = research_board_segment(&research.board);
    if !board_segment.is_empty() {
        if !line.trim().is_empty() {
            line.push_str(" || ");
        }
        line.push_str(&board_segment);
    }
    if let Some(run) = &research.active_run_progress {
        let run_segment = format!(
            "运行 {} | status {} | progress {}/{} · {}% | current {}",
            run.run_id,
            compact_segment_value(&run.status),
            run.done_steps,
            run.total_steps,
            run.percent,
            compact_segment_value(
                run.current_step_title
                    .as_deref()
                    .unwrap_or("no active step")
            )
        );
        if !line.trim().is_empty() {
            line.push_str(" || ");
        }
        line.push_str(&run_segment);
    }
    if let Some(loop_closure) = &research.loop_closure {
        let loop_segment = format!(
            "闭环 {} | continue {} | next {}",
            compact_segment_value(&loop_closure.status),
            loop_closure.should_continue,
            compact_segment_value(&loop_closure.next_recommended_action)
        );
        if !line.trim().is_empty() {
            line.push_str(" || ");
        }
        line.push_str(&loop_segment);
    }
    if let Some(maintenance) = &research.task_pool_maintenance {
        let maintenance_segment = format!(
            "任务池 {} | ready {} | running {} | blocked {} | action {}",
            compact_segment_value(&maintenance.status),
            maintenance.ready_to_run_count,
            maintenance.running_count,
            maintenance.blocked_count,
            compact_segment_value(&maintenance.recommended_action_id)
        );
        if !line.trim().is_empty() {
            line.push_str(" || ");
        }
        line.push_str(&maintenance_segment);
    }
    if let Some(goal_watch) = &research.goal_watch {
        let watch_segment = format!(
            "watch {} | ticks {} | stop {}",
            compact_segment_value(&goal_watch.status),
            goal_watch.ticks_completed,
            compact_segment_value(&goal_watch.stop_reason)
        );
        if !line.trim().is_empty() {
            line.push_str(" || ");
        }
        line.push_str(&watch_segment);
    }
    line
}

fn research_board_segment(board: &crate::research::ResearchBoardProjection) -> String {
    let total_entries = board.entries.len();
    if total_entries == 0 {
        return String::new();
    }
    let bucket_counts =
        board
            .entries
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut acc, entry| {
                *acc.entry(entry.bucket_id.clone()).or_insert(0usize) += 1;
                acc
            });
    let summary = bucket_counts
        .iter()
        .take(3)
        .map(|(bucket, count)| format!("{bucket}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("看板 {} 条 | {}", total_entries, summary)
}

fn compact_segment_value(value: &str) -> String {
    value
        .replace(['|', '\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl TuiInteractionState {
    fn from_result(result: &TuiLaunchResult) -> Self {
        Self {
            composer: String::new(),
            transcript: vec![TranscriptTurn {
                role: "Astra",
                body: "就绪。直接输入需求，或输入 / 打开命令，$ 打开技能。".to_string(),
            }],
            command_entries: result
                .inline_view
                .command_model
                .groups
                .iter()
                .flat_map(|group| &group.commands)
                .map(|entry| TuiCommandCard {
                    typed: entry.typed.clone(),
                    action_id: entry.action_id.clone(),
                    label: entry.label.clone(),
                    gate: entry.gate.clone(),
                    category: command_category_for_action(&entry.action_id),
                    summary: entry.summary.clone(),
                })
                .collect(),
            skill_entries: result
                .inline_view
                .skill_model
                .entries
                .iter()
                .map(|entry| entry.typed.clone())
                .collect(),
            skill_descriptions: result
                .inline_view
                .skill_model
                .entries
                .iter()
                .map(|entry| (entry.skill_id.clone(), entry.description.clone()))
                .collect(),
            skill_sources: result
                .inline_view
                .skill_model
                .entries
                .iter()
                .map(|entry| (entry.skill_id.clone(), entry.source.clone()))
                .collect(),
            active_session_id: result.projection.sessions.active_session_id.clone(),
            recent_sessions: result
                .projection
                .sessions
                .recent
                .iter()
                .map(|session| TuiSessionEntry {
                    session_id: session.session_id.clone(),
                    title: session.title.clone(),
                    updated_at: session.updated_at.clone(),
                    status: session.status.clone(),
                })
                .collect(),
            prompt_history: Vec::new(),
            input_history: Vec::new(),
            research_line: combined_research_line(&result.projection.research),
            session_count: result.projection.sessions.total_count.to_string(),
            permission_count: result.projection.permissions.pending_count.to_string(),
            permission_mode: status_badge(&result.inline_view, "Permission Mode")
                .unwrap_or_else(|| "read-only".to_string()),
            model_label: status_badge(&result.inline_view, "Model")
                .unwrap_or_else(|| "auto".to_string()),
            reasoning_effort: status_badge(&result.inline_view, "Reasoning")
                .unwrap_or_else(|| "auto".to_string()),
            working_dir: inline_repl_directory_label(),
            git_branch: inline_repl_git_branch(),
            remote_state: result.projection.surfaces.mobile.readiness.clone(),
            running_turn: None,
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        }
    }
}

fn apply_interaction_to_model(model: &mut TerminalFrameModel, interaction: &TuiInteractionState) {
    model.transcript.clone_from(&interaction.transcript);
    model.composer.clone_from(&interaction.composer);
    model.skill_entries.clone_from(&interaction.skill_entries);
    model
        .command_entries
        .clone_from(&interaction.command_entries);
    model
        .permission_count
        .clone_from(&interaction.permission_count);
    model
        .permission_mode
        .clone_from(&interaction.permission_mode);
    model.model_label.clone_from(&interaction.model_label);
    model
        .reasoning_effort
        .clone_from(&interaction.reasoning_effort);
    model.remote_state.clone_from(&interaction.remote_state);
    model.research_line.clone_from(&interaction.research_line);
    model.session_count.clone_from(&interaction.session_count);
    model.language = interaction.language;
    model.theme_mode = interaction.theme_mode;
    model.overlay_selected = interaction.overlay_selected;
    model.output_folded = interaction.output_folded;
}

#[cfg(test)]
fn handle_tui_input(state: &mut TuiInteractionState, bytes: &[u8]) -> TuiInputOutcome {
    let language = state.language;
    handle_tui_input_with_executor(state, bytes, &mut |_prompt| {
        Ok(TuiCommandExecution::new(match language {
            TuiLanguage::Zh => "提示已接收。输入会进入 CLI 命令路由。".to_string(),
            TuiLanguage::En => {
                "Prompt captured. The turn will run through the CLI command router.".to_string()
            }
        }))
    })
}

#[cfg(test)]
fn handle_tui_input_with_executor<F>(
    state: &mut TuiInteractionState,
    bytes: &[u8],
    executor: &mut F,
) -> TuiInputOutcome
where
    F: FnMut(&str) -> Result<TuiCommandExecution, String>,
{
    if let Some(exit_key) = decode_exit_key(bytes) {
        return TuiInputOutcome::Exit(exit_key);
    }
    if let Some(direction) = decode_arrow_key(bytes) {
        move_overlay_selection(state, direction);
        return TuiInputOutcome::Continue;
    }
    match bytes {
        [b'\r'] | [b'\n'] => {
            if typed_exit_requested(&state.composer) {
                return TuiInputOutcome::Exit("slash-exit");
            }
            if !select_overlay_candidate(state) {
                submit_composer_with_executor(state, executor);
            } else if !overlay_candidate_needs_more_input(&state.composer) {
                if typed_exit_requested(&state.composer) {
                    return TuiInputOutcome::Exit("slash-exit");
                }
                submit_composer_with_executor(state, executor);
            }
        }
        [8] | [127] => {
            state.composer.pop();
            state.overlay_selected = 0;
        }
        [27] => {
            state.composer.clear();
            state.overlay_selected = 0;
        }
        _ => {
            append_text_input(&mut state.composer, bytes);
            state.overlay_selected = 0;
        }
    }
    TuiInputOutcome::Continue
}

#[cfg(test)]
fn handle_tui_input_streaming<F>(
    state: &mut TuiInteractionState,
    bytes: &[u8],
    executor: &Arc<Mutex<F>>,
) -> TuiInputOutcome
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
{
    let fallback_config_executor = Arc::new(Mutex::new(|_action: TuiConfigAction| {
        Err("TUI configuration executor is not bound".to_string())
    }));
    let fallback_permission_executor = Arc::new(Mutex::new(|_action: TuiPermissionAction| {
        Err("TUI permission executor is not bound".to_string())
    }));
    handle_tui_input_streaming_with_config(
        state,
        bytes,
        executor,
        &fallback_config_executor,
        &fallback_permission_executor,
    )
}

fn handle_tui_input_streaming_with_config<F, C, P>(
    state: &mut TuiInteractionState,
    bytes: &[u8],
    executor: &Arc<Mutex<F>>,
    config_executor: &Arc<Mutex<C>>,
    permission_executor: &Arc<Mutex<P>>,
) -> TuiInputOutcome
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    if bytes == [27] {
        if interrupt_running_turn(state) {
            state.composer.clear();
            state.overlay_selected = 0;
            return TuiInputOutcome::Continue;
        }
        state.composer.clear();
        state.overlay_selected = 0;
        return TuiInputOutcome::Continue;
    }
    if bytes == [3] {
        if interrupt_running_turn(state) {
            state.composer.clear();
            state.overlay_selected = 0;
            return TuiInputOutcome::Continue;
        }
        if !state.composer.is_empty() {
            state.composer.clear();
            state.overlay_selected = 0;
            return TuiInputOutcome::Continue;
        }
        return TuiInputOutcome::Exit("ctrl-c");
    }
    if let Some(exit_key) = decode_exit_key(bytes) {
        return TuiInputOutcome::Exit(exit_key);
    }
    if let Some(direction) = decode_arrow_key(bytes) {
        move_overlay_selection(state, direction);
        return TuiInputOutcome::Continue;
    }
    match bytes {
        [b'\r'] | [b'\n'] => {
            if typed_exit_requested(&state.composer) {
                return TuiInputOutcome::Exit("slash-exit");
            }
            if !select_overlay_candidate(state) {
                submit_composer_streaming_with_config(
                    state,
                    executor,
                    config_executor,
                    permission_executor,
                );
            } else if !overlay_candidate_needs_more_input(&state.composer) {
                if typed_exit_requested(&state.composer) {
                    return TuiInputOutcome::Exit("slash-exit");
                }
                submit_composer_streaming_with_config(
                    state,
                    executor,
                    config_executor,
                    permission_executor,
                );
            }
        }
        [8] | [127] => {
            state.composer.pop();
            state.overlay_selected = 0;
        }
        [27] => {
            state.composer.clear();
            state.overlay_selected = 0;
        }
        _ => {
            append_text_input(&mut state.composer, bytes);
            state.overlay_selected = 0;
        }
    }
    TuiInputOutcome::Continue
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayDirection {
    Up,
    Down,
}

fn decode_arrow_key(bytes: &[u8]) -> Option<OverlayDirection> {
    match bytes {
        [27, b'[', b'A'] => Some(OverlayDirection::Up),
        [27, b'[', b'B'] => Some(OverlayDirection::Down),
        _ => None,
    }
}

fn move_overlay_selection(state: &mut TuiInteractionState, direction: OverlayDirection) {
    let count = active_overlay_len(state);
    if count == 0 {
        return;
    }
    state.overlay_selected = state.overlay_selected.min(count.saturating_sub(1));
    match direction {
        OverlayDirection::Up => {
            state.overlay_selected = if state.overlay_selected == 0 {
                count - 1
            } else {
                state.overlay_selected - 1
            };
        }
        OverlayDirection::Down => {
            state.overlay_selected = (state.overlay_selected + 1) % count;
        }
    }
}

fn select_overlay_candidate(state: &mut TuiInteractionState) -> bool {
    if command_palette_active(&state.composer) {
        let entries = matching_command_entries_for_state(state);
        let Some(entry) = entries
            .get(state.overlay_selected.min(entries.len().saturating_sub(1)))
            .cloned()
        else {
            return false;
        };
        state.composer = command_typed_for_selection(&entry);
        state.overlay_selected = 0;
        return true;
    }
    if skill_palette_active(&state.composer) {
        let entries = matching_skill_entries_for_state(state);
        let Some(entry) = entries
            .get(state.overlay_selected.min(entries.len().saturating_sub(1)))
            .cloned()
        else {
            return false;
        };
        state.composer = entry;
        state.overlay_selected = 0;
        return true;
    }
    false
}

fn overlay_candidate_needs_more_input(composer: &str) -> bool {
    composer.ends_with(' ')
}

fn command_palette_active(composer: &str) -> bool {
    let trimmed = composer.trim_start();
    trimmed.starts_with('/')
        && !trimmed.chars().any(char::is_whitespace)
        && !is_exact_product_command(trimmed)
}

fn skill_palette_active(composer: &str) -> bool {
    let trimmed = composer.trim_start();
    trimmed.starts_with('$') && !trimmed.chars().any(char::is_whitespace)
}

fn is_exact_product_command(input: &str) -> bool {
    product_command_specs().into_iter().any(|spec| {
        let typed = spec.typed.split('<').next().unwrap_or(&spec.typed).trim();
        typed == input
    })
}

fn typed_exit_requested(composer: &str) -> bool {
    matches!(composer.trim(), "/exit" | "/quit")
}

fn typed_interrupt_requested(composer: &str) -> bool {
    if typed_interrupt_alias(composer) {
        return true;
    }
    let request = parse_surface_command(composer.trim());
    request.kind == SurfaceCommandKind::ProjectedAction
        && request.action_id.as_deref() == Some("interrupt_turn")
}

fn command_typed_for_selection(entry: &TuiCommandCard) -> String {
    if let Some(prefix) = entry.typed.split('<').next() {
        let prefix = prefix.trim_end();
        if prefix != entry.typed {
            return format!("{prefix} ");
        }
    }
    entry.typed.clone()
}

fn active_overlay_len(state: &TuiInteractionState) -> usize {
    if command_palette_active(&state.composer) {
        matching_command_entries_for_state(state).len()
    } else if skill_palette_active(&state.composer) {
        matching_skill_entries_for_state(state).len()
    } else {
        0
    }
}

fn append_text_input(buffer: &mut String, bytes: &[u8]) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    if text.chars().all(|ch| !ch.is_control() || ch == '\t') {
        buffer.push_str(text);
    }
}

fn slash_query(input: &str) -> String {
    input
        .trim_start()
        .trim_start_matches('/')
        .trim()
        .to_lowercase()
}

fn skill_query(input: &str) -> String {
    input
        .trim_start()
        .trim_start_matches('$')
        .trim()
        .to_lowercase()
}

fn matching_command_entries(entries: &[TuiCommandCard], composer: &str) -> Vec<TuiCommandCard> {
    let query = slash_query(composer);
    let mut entries = if entries.is_empty() {
        product_command_specs()
            .into_iter()
            .map(|spec| TuiCommandCard {
                typed: spec.typed,
                action_id: spec.action_id,
                label: spec.label,
                gate: spec.category.clone(),
                category: spec.category,
                summary: spec.summary,
            })
            .collect::<Vec<_>>()
    } else {
        entries.to_vec()
    };
    entries.retain(|entry| command_entry_search_score(entry, &query).is_some());
    if !query.is_empty() {
        entries.sort_by_key(|entry| {
            let score =
                command_entry_search_score(entry, &query).unwrap_or((usize::MAX, usize::MAX));
            (
                score.0,
                score.1,
                command_category_rank(&entry.category),
                entry.typed.len(),
            )
        });
    }
    entries
}

fn command_entry_search_score(entry: &TuiCommandCard, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return Some((0, 0));
    }

    let typed = entry.typed.to_lowercase();
    let typed_without_slash = typed.trim_start_matches('/').to_string();
    let typed_root = typed_without_slash
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if typed_without_slash.starts_with(query) {
        return Some((0, 0));
    }
    if !typed_root.is_empty() && query.starts_with(&format!("{typed_root} ")) {
        return Some((0, 1));
    }

    let haystacks = [
        (1usize, entry.action_id.to_lowercase()),
        (2, typed_without_slash),
        (3, entry.label.to_lowercase()),
        (
            4,
            localized_command_label(entry, TuiLanguage::Zh).to_lowercase(),
        ),
        (5, entry.category.to_lowercase()),
        (
            6,
            localized_command_category(&entry.category, TuiLanguage::Zh).to_lowercase(),
        ),
        (7, entry.summary.to_lowercase()),
        (
            8,
            localized_command_summary(entry, TuiLanguage::Zh).to_lowercase(),
        ),
        (
            9,
            localized_command_summary(entry, TuiLanguage::En).to_lowercase(),
        ),
    ];
    haystacks.iter().find_map(|(tier, haystack)| {
        if let Some(position) = haystack.find(query) {
            Some((*tier, position))
        } else if query.chars().count() >= 4 && fuzzy_subsequence(haystack, query) {
            Some((10, 0))
        } else {
            None
        }
    })
}

fn fuzzy_subsequence(haystack: &str, query: &str) -> bool {
    let mut chars = haystack.chars();
    query
        .chars()
        .all(|needle| chars.by_ref().any(|candidate| candidate == needle))
}

fn command_category_rank(category: &str) -> usize {
    match category {
        "conversation" => 0,
        "navigation" => 1,
        "permissions" => 2,
        "configuration" => 3,
        "session" => 4,
        "source_control" => 5,
        "output" => 6,
        "context" => 7,
        "work" => 8,
        "research" => 9,
        "tools" => 10,
        "usage" => 11,
        "diagnostics" => 12,
        _ => 13,
    }
}

fn command_display_group(category: &str) -> &'static str {
    match category {
        "conversation" | "navigation" | "permissions" | "configuration" => "Core",
        "session" | "source_control" | "output" | "context" | "work" => "Session",
        "research" | "tools" | "usage" | "diagnostics" => "Skills",
        _ => "Other",
    }
}

#[allow(dead_code)]
fn command_display_group_rank(group: &str) -> usize {
    match group {
        "Core" => 0,
        "Session" => 1,
        "Skills" => 2,
        _ => 3,
    }
}

fn localized_display_group(group: &str, language: TuiLanguage) -> &'static str {
    match group {
        "Core" => language.text("核心", "Core"),
        "Session" => language.text("会话", "Session"),
        "Skills" => language.text("技能", "Skills"),
        _ => language.text("其他", "Other"),
    }
}

fn matching_skill_entries(entries: &[String], composer: &str) -> Vec<String> {
    let query = skill_query(composer);
    entries
        .iter()
        .filter(|entry| query.is_empty() || entry.to_lowercase().contains(&query))
        .cloned()
        .collect()
}

fn matching_command_entries_for_state(state: &TuiInteractionState) -> Vec<TuiCommandCard> {
    matching_command_entries(&state.command_entries, &state.composer)
}

fn matching_skill_entries_for_state(state: &TuiInteractionState) -> Vec<String> {
    matching_skill_entries(&state.skill_entries, &state.composer)
}

fn apply_language_command(state: &mut TuiInteractionState, input: &str) -> Option<String> {
    let mut parts = input.split_whitespace();
    let command = parts.next()?;
    if command != "/language" && command != "/lang" {
        return None;
    }
    match parts.next() {
        Some("zh") | Some("cn") | Some("中文") => {
            state.language = TuiLanguage::Zh;
            Some("界面语言已切换为中文。".to_string())
        }
        Some("en") | Some("english") | Some("English") => {
            state.language = TuiLanguage::En;
            Some("Interface language switched to English.".to_string())
        }
        _ => Some(match state.language {
            TuiLanguage::Zh => "用法：/language zh 或 /language en。当前语言：中文。".to_string(),
            TuiLanguage::En => {
                "Usage: /language zh or /language en. Current language: English.".to_string()
            }
        }),
    }
}

fn apply_theme_command(state: &mut TuiInteractionState, input: &str) -> Option<String> {
    let mut parts = input.split_whitespace();
    let command = parts.next()?;
    if command != "/theme" {
        return None;
    }
    match parts.next() {
        Some("light") | Some("day") | Some("白天") | Some("日间") => {
            state.theme_mode = TuiThemeMode::Day;
            Some(
                state
                    .language
                    .text("主题已切换为白天模式。", "Theme switched to light mode.")
                    .to_string(),
            )
        }
        Some("dark") | Some("night") | Some("黑夜") | Some("夜间") => {
            state.theme_mode = TuiThemeMode::Night;
            Some(
                state
                    .language
                    .text("主题已切换为夜间模式。", "Theme switched to dark mode.")
                    .to_string(),
            )
        }
        _ => Some(match state.language {
            TuiLanguage::Zh => "用法：/theme light 或 /theme dark。当前默认白天模式。".to_string(),
            TuiLanguage::En => {
                "Usage: /theme light or /theme dark. The default theme is light.".to_string()
            }
        }),
    }
}

#[cfg(test)]
fn submit_composer_with_executor<F>(state: &mut TuiInteractionState, executor: &mut F)
where
    F: FnMut(&str) -> Result<TuiCommandExecution, String>,
{
    let input = state.composer.trim().to_string();
    if input.is_empty() {
        return;
    }
    state.transcript.push(TranscriptTurn {
        role: "You",
        body: input.clone(),
    });
    if let Some(response) = apply_language_command(state, &input) {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: response,
        });
        state.composer.clear();
        return;
    }
    if let Some(response) = apply_theme_command(state, &input) {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: response,
        });
        state.composer.clear();
        return;
    }
    let response = route_typed_command_with_executor(state, &input, executor);
    state.transcript.push(TranscriptTurn {
        role: "Astra",
        body: response,
    });
    state.composer.clear();
}

#[cfg(test)]
fn submit_composer_streaming<F>(state: &mut TuiInteractionState, executor: &Arc<Mutex<F>>)
where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
{
    let fallback_config_executor = Arc::new(Mutex::new(|_action: TuiConfigAction| {
        Err("TUI configuration executor is not bound".to_string())
    }));
    let fallback_permission_executor = Arc::new(Mutex::new(|_action: TuiPermissionAction| {
        Err("TUI permission executor is not bound".to_string())
    }));
    submit_composer_streaming_with_config(
        state,
        executor,
        &fallback_config_executor,
        &fallback_permission_executor,
    )
}

fn submit_composer_streaming_with_config<F, C, P>(
    state: &mut TuiInteractionState,
    executor: &Arc<Mutex<F>>,
    config_executor: &Arc<Mutex<C>>,
    permission_executor: &Arc<Mutex<P>>,
) where
    F: FnMut(&str, TuiStreamSender, RuntimeCancelToken) -> Result<TuiCommandExecution, String>
        + Send
        + 'static,
    C: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String> + Send + 'static,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String> + Send + 'static,
{
    let input = state.composer.trim().to_string();
    if input.is_empty() {
        return;
    }
    if state.running_turn.is_some() && typed_interrupt_requested(&input) {
        state.transcript.push(TranscriptTurn {
            role: "You",
            body: input,
        });
        interrupt_running_turn(state);
        state.composer.clear();
        state.overlay_selected = 0;
        return;
    }
    if state.running_turn.is_some() {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: state.language.text(
                "已有回合运行中。请等待完成后再提交下一个输入。",
                "A turn is already running. Wait for it to finish before submitting the next prompt.",
            ).to_string(),
        });
        state.composer.clear();
        return;
    }

    state.transcript.push(TranscriptTurn {
        role: "You",
        body: input.clone(),
    });

    if let Some(response) = apply_language_command(state, &input) {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: response,
        });
        state.composer.clear();
        return;
    }

    if let Some(response) = apply_theme_command(state, &input) {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: response,
        });
        state.composer.clear();
        return;
    }

    match prompt_payload_or_plain_text(&input) {
        Some(Ok(prompt)) => {
            push_prompt_history_entry(state, &prompt);
            let response_index = state.transcript.len();
            state.transcript.push(TranscriptTurn {
                role: "Astra",
                body: running_turn_body(&prompt, 0),
            });
            let (sender, receiver) = mpsc::channel();
            let stream_sender = TuiStreamSender {
                sender: sender.clone(),
            };
            let (cancel_token, interrupt_handle) = runtime_interrupt_pair();
            let prompt_for_thread = prompt.clone();
            let stream_sender_for_thread = stream_sender.clone();
            let executor = Arc::clone(executor);
            thread::spawn(move || {
                let result = executor
                    .lock()
                    .map_err(|_| "TUI executor lock poisoned".to_string())
                    .and_then(|mut locked| {
                        (*locked)(&prompt_for_thread, stream_sender_for_thread, cancel_token)
                    });
                let _ = sender.send(TuiTurnEvent::Complete(result));
            });
            state.running_turn = Some(TuiRunningTurn {
                response_index,
                prompt,
                frame: 0,
                streamed_body: String::new(),
                interrupt_handle,
                #[cfg(test)]
                stream_sender,
                receiver,
            });
        }
        Some(Err(message)) => {
            state.transcript.push(TranscriptTurn {
                role: "Astra",
                body: message,
            });
        }
        None => {
            let response = route_typed_command_mut_with_executors(
                state,
                &input,
                &mut |action| {
                    config_executor
                        .lock()
                        .map_err(|_| "TUI configuration executor lock poisoned".to_string())
                        .and_then(|mut locked| (*locked)(action))
                },
                &mut |action| {
                    permission_executor
                        .lock()
                        .map_err(|_| "TUI permission executor lock poisoned".to_string())
                        .and_then(|mut locked| (*locked)(action))
                },
            );
            state.transcript.push(TranscriptTurn {
                role: "Astra",
                body: response,
            });
        }
    }
    state.composer.clear();
}

fn interrupt_running_turn(state: &mut TuiInteractionState) -> bool {
    let Some(running) = state.running_turn.take() else {
        return false;
    };
    running.interrupt_handle.interrupt();
    let body = interrupted_turn_body(&running.prompt, state.language);
    if let Some(turn) = state.transcript.get_mut(running.response_index) {
        turn.body = body;
    } else {
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body,
        });
    }
    true
}

fn refresh_streaming_turn(state: &mut TuiInteractionState) {
    let mut finished = None;
    let language = state.language;
    if let Some(running) = state.running_turn.as_mut() {
        loop {
            match running.receiver.try_recv() {
                Ok(TuiTurnEvent::Delta(delta)) => {
                    running.streamed_body.push_str(&delta);
                    if let Some(turn) = state.transcript.get_mut(running.response_index) {
                        turn.body = running_stream_body(
                            &running.prompt,
                            running.frame,
                            &running.streamed_body,
                        );
                    }
                }
                Ok(TuiTurnEvent::ToolCallStarted { tool_name, call_id }) => {
                    running.streamed_body.push_str(&format!(
                        "\n  ▸ {}\n",
                        tool_event_label(&tool_name, &call_id)
                    ));
                }
                Ok(TuiTurnEvent::ToolResultReady {
                    tool_name,
                    call_id,
                    status,
                }) => {
                    running.streamed_body.push_str(&format!(
                        "  ✓ {}: {status}\n",
                        tool_event_label(&tool_name, &call_id)
                    ));
                }
                Ok(TuiTurnEvent::Complete(result)) => {
                    let body = match result {
                        Ok(execution) => final_transcript_body_after_streaming(
                            &execution,
                            &running.streamed_body,
                        ),
                        Err(message) => match language {
                            TuiLanguage::Zh => format!("提示失败：{message}"),
                            TuiLanguage::En => format!("Prompt failed: {message}"),
                        },
                    };
                    finished = Some((running.response_index, body));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    running.frame = running.frame.wrapping_add(1);
                    if let Some(turn) = state.transcript.get_mut(running.response_index) {
                        turn.body = running_stream_body(
                            &running.prompt,
                            running.frame,
                            &running.streamed_body,
                        );
                    }
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = Some((
                        running.response_index,
                        language
                            .text(
                                "提示失败：执行线程已断开",
                                "Prompt failed: execution worker disconnected",
                            )
                            .to_string(),
                    ));
                    break;
                }
            }
        }
    }
    if let Some((response_index, body)) = finished {
        if let Some(turn) = state.transcript.get_mut(response_index) {
            turn.body = body;
        }
        state.running_turn = None;
    }
}

fn prompt_payload(input: &str) -> Option<Result<String, String>> {
    let request = parse_surface_command(input);
    if request.kind == SurfaceCommandKind::PromptTurn {
        return Some(match request.prompt {
            Some(prompt) if !prompt.trim().is_empty() => Ok(prompt),
            _ => Err("用法：/prompt <text>。输入会进入 CLI 闭环路由。".to_string()),
        });
    }
    None
}

fn prompt_payload_or_plain_text(input: &str) -> Option<Result<String, String>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with('/') && !trimmed.starts_with('$') {
        return Some(Ok(trimmed.to_string()));
    }
    prompt_payload(trimmed)
}

fn running_turn_body(prompt: &str, frame: usize) -> String {
    let spinner = ["-", "\\", "|", "/"][frame % 4];
    format!(
        "正在运行 {spinner} {prompt}\n流式刷新：CLI 回合仍在运行，最终 provider/session/research 输出会替换此卡片。"
    )
}

fn running_stream_body(prompt: &str, frame: usize, streamed_body: &str) -> String {
    if streamed_body.is_empty() {
        return running_turn_body(prompt, frame);
    }
    let spinner = ["-", "\\", "|", "/"][frame % 4];
    format!("{streamed_body}\n\n正在运行 {spinner} {prompt}\nToken 级流式输出...")
}

fn interrupted_turn_body(prompt: &str, language: TuiLanguage) -> String {
    match language {
        TuiLanguage::Zh => format!(
            "已请求中断：当前回合已取消。\n原始提示：{prompt}\n说明：底层执行器可能仍在后台收尾，后续完成事件会被忽略。"
        ),
        TuiLanguage::En => format!(
            "Interrupt requested: the active turn was canceled.\nOriginal prompt: {prompt}\nNote: the underlying executor may still finish in the background; its later completion event will be ignored."
        ),
    }
}

#[cfg(test)]
fn route_typed_command_with_executor<F>(
    state: &mut TuiInteractionState,
    input: &str,
    executor: &mut F,
) -> String
where
    F: FnMut(&str) -> Result<TuiCommandExecution, String>,
{
    let request = parse_tui_surface_command(input);
    if request.kind == SurfaceCommandKind::PromptTurn {
        if let Some(prompt) = request.prompt.filter(|prompt| !prompt.trim().is_empty()) {
            return execute_prompt_turn(&prompt, executor);
        } else {
            return match state.language {
                TuiLanguage::Zh => "用法：/prompt <text>。输入会进入 CLI 闭环路由。".to_string(),
                TuiLanguage::En => {
                    "Usage: /prompt <text>. The prompt runs through the governed CLI turn router."
                        .to_string()
                }
            };
        }
    }
    route_surface_command_mut(state, request)
}

#[cfg(test)]
fn execute_prompt_turn<F>(prompt: &str, executor: &mut F) -> String
where
    F: FnMut(&str) -> Result<TuiCommandExecution, String>,
{
    match executor(prompt) {
        Ok(execution) => execution.body,
        Err(message) => format!("提示失败：{message}"),
    }
}

#[cfg(test)]
fn route_typed_command(state: &TuiInteractionState, input: &str) -> String {
    let mut scratch = state.clone_for_routing();
    route_surface_command_mut(&mut scratch, parse_tui_surface_command(input))
}

#[cfg(test)]
fn route_typed_command_mut(state: &mut TuiInteractionState, input: &str) -> String {
    route_surface_command_mut(state, parse_tui_surface_command(input))
}

#[cfg(test)]
fn route_typed_command_mut_with_config<F>(
    state: &mut TuiInteractionState,
    input: &str,
    mut config_executor: F,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
{
    let mut permission_executor = |_action| Err("TUI permission executor is not bound".to_string());
    route_surface_command_mut_with_executors(
        state,
        parse_tui_surface_command(input),
        &mut config_executor,
        &mut permission_executor,
    )
}

fn route_typed_command_mut_with_executors<F, P>(
    state: &mut TuiInteractionState,
    input: &str,
    config_executor: &mut F,
    permission_executor: &mut P,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String>,
{
    route_surface_command_mut_with_executors(
        state,
        parse_tui_surface_command(input),
        config_executor,
        permission_executor,
    )
}

#[cfg(test)]
#[cfg(test)]
fn route_surface_command_mut(
    state: &mut TuiInteractionState,
    request: SurfaceCommandRequest,
) -> String {
    let mut config_executor = |_action| Err("TUI configuration executor is not bound".to_string());
    let mut permission_executor = |_action| Err("TUI permission executor is not bound".to_string());
    route_surface_command_mut_with_executors(
        state,
        request,
        &mut config_executor,
        &mut permission_executor,
    )
}

fn parse_tui_surface_command(input: &str) -> SurfaceCommandRequest {
    tui_local_surface_request(input).unwrap_or_else(|| parse_surface_command(input))
}

fn tui_local_surface_request(input: &str) -> Option<SurfaceCommandRequest> {
    let raw = input.trim();
    let (action_id, command) = if raw == "/history" || raw.starts_with("/history ") {
        ("show_history", "/history")
    } else if raw == "/resume" || raw.starts_with("/resume ") {
        ("resume_session", "/resume")
    } else if raw == "/research board" || raw.starts_with("/research board ") {
        ("open_research_board", "/research board")
    } else if raw == "/approve" || raw.starts_with("/approve ") {
        ("approve_permission", "/approve")
    } else if raw == "/deny" || raw.starts_with("/deny ") {
        ("deny_permission", "/deny")
    } else {
        return None;
    };
    Some(SurfaceCommandRequest {
        raw: raw.to_string(),
        kind: SurfaceCommandKind::ProjectedAction,
        action_id: Some(action_id.to_string()),
        skill_id: None,
        args: raw
            .strip_prefix(command)
            .unwrap_or("")
            .split_whitespace()
            .map(ToString::to_string)
            .collect(),
        prompt: None,
        suggestions: Vec::new(),
    })
}

fn route_surface_command_mut_with_executors<F, P>(
    state: &mut TuiInteractionState,
    request: SurfaceCommandRequest,
    config_executor: &mut F,
    permission_executor: &mut P,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String>,
{
    match request.kind {
        SurfaceCommandKind::PromptTurn => state
            .language
            .text(
                "提示已接收。输入会进入 CLI 命令路由。",
                "Prompt captured. The turn will run through the CLI command router.",
            )
            .to_string(),
        SurfaceCommandKind::Help => render_command_help(state),
        SurfaceCommandKind::SkillList => render_skill_list(state),
        SurfaceCommandKind::SkillInspect => {
            let skill_id = request.skill_id.unwrap_or_default();
            render_skill_inspect(state, &skill_id)
        }
        SurfaceCommandKind::SkillRun => {
            let skill_id = request.skill_id.unwrap_or_default();
            if skill_known(state, &skill_id) {
                match state.language {
                    TuiLanguage::Zh => format!(
                        "技能运行已暂存：${skill_id} {}\n执行路径：skills run --skill {skill_id}",
                        request.args.join(" ")
                    ),
                    TuiLanguage::En => format!(
                        "Skill run staged: ${skill_id} {}\nExecution path: skills run --skill {skill_id}",
                        request.args.join(" ")
                    ),
                }
            } else {
                match state.language {
                    TuiLanguage::Zh => {
                        format!("未知技能 `{skill_id}`。输入 $list 浏览已安装技能。")
                    }
                    TuiLanguage::En => {
                        format!(
                            "Unknown skill `{skill_id}`. Type $list to browse installed skills."
                        )
                    }
                }
            }
        }
        SurfaceCommandKind::ProjectedAction => {
            render_projected_action(state, &request, config_executor, permission_executor)
        }
        SurfaceCommandKind::Unknown => {
            let suggestions = if request.suggestions.is_empty() {
                "/help or $list".to_string()
            } else {
                request.suggestions.join(" ")
            };
            match state.language {
                TuiLanguage::Zh => format!("未知命令 `{}`。可尝试 {suggestions}。", request.raw),
                TuiLanguage::En => format!("Unknown command `{}`. Try {suggestions}.", request.raw),
            }
        }
    }
}

impl TuiInteractionState {
    #[cfg(test)]
    fn clone_for_routing(&self) -> Self {
        Self {
            composer: self.composer.clone(),
            transcript: self.transcript.clone(),
            skill_entries: self.skill_entries.clone(),
            skill_descriptions: self.skill_descriptions.clone(),
            skill_sources: self.skill_sources.clone(),
            command_entries: self.command_entries.clone(),
            active_session_id: self.active_session_id.clone(),
            recent_sessions: self.recent_sessions.clone(),
            prompt_history: self.prompt_history.clone(),
            input_history: self.input_history.clone(),
            research_line: self.research_line.clone(),
            session_count: self.session_count.clone(),
            permission_count: self.permission_count.clone(),
            permission_mode: self.permission_mode.clone(),
            model_label: self.model_label.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            remote_state: self.remote_state.clone(),
            running_turn: None,
            working_dir: self.working_dir.clone(),
            git_branch: self.git_branch.clone(),
            language: self.language,
            theme_mode: self.theme_mode,
            overlay_selected: self.overlay_selected,
            output_folded: self.output_folded,
        }
    }
}

fn render_command_help(state: &TuiInteractionState) -> String {
    let mut lines = vec![
        state.language.text("命令", "Commands").to_string(),
        state
            .language
            .text(
                "对话保持一级入口；/ 打开产品命令，$ 打开技能。",
                "Conversation stays primary; / opens product commands and $ opens skills.",
            )
            .to_string(),
    ];

    // Build a unified list of (display_group, category_rank, typed, label).
    let items: Vec<(&str, usize, String, String)> = if !state.command_entries.is_empty() {
        state
            .command_entries
            .iter()
            .map(|card| {
                (
                    command_display_group(&card.category),
                    command_category_rank(&card.category),
                    card.typed.clone(),
                    localized_command_label(card, state.language),
                )
            })
            .collect()
    } else {
        product_command_specs()
            .iter()
            .map(|spec| {
                (
                    command_display_group(&spec.category),
                    command_category_rank(&spec.category),
                    spec.typed.clone(),
                    localized_label(&spec.action_id, &spec.label, state.language),
                )
            })
            .collect()
    };

    // Group by display group in order: Core, Session, Skills, Other.
    let group_order = ["Core", "Session", "Skills", "Other"];
    for group in &group_order {
        let mut group_items: Vec<_> = items.iter().filter(|(g, _, _, _)| *g == *group).collect();
        if group_items.is_empty() {
            continue;
        }
        // Sort within group by category rank, then by typed string.
        group_items.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)));

        // Render group header.
        lines.push(format!(
            "  {}",
            localized_display_group(group, state.language)
        ));

        // Render items.
        for (_, _, typed, label) in &group_items {
            lines.push(format!("    {typed}  \u{2192}  {label}"));
        }
    }

    lines.push(
        state
            .language
            .text(
                "技能：$list，$skill-name，或 $skill-name <input>",
                "Skills: $list, $skill-name, or $skill-name <input>",
            )
            .to_string(),
    );
    lines.join("\n")
}

fn render_skill_list(state: &TuiInteractionState) -> String {
    if state.skill_entries.is_empty() {
        return state
            .language
            .text(
                "技能：当前工作区未发现技能。可安装到 .codex/skills、.agents/skills 或配置的状态目录。",
                "Skills: none discovered for this workspace. Install skills under .codex/skills, .agents/skills, or the configured state home.",
            )
            .to_string();
    }
    let preview = state
        .skill_entries
        .iter()
        .take(12)
        .map(|entry| {
            format!(
                "{entry} - {}",
                skill_description_for_state(state, entry, state.language)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    match state.language {
        TuiLanguage::Zh => format!(
            "技能：共 {} 个\n{}\n使用 $skill-name 查看；使用 $skill-name <input> 暂存一次受控运行。",
            state.skill_entries.len(),
            preview
        ),
        TuiLanguage::En => format!(
            "Skills: {} total\n{}\nUse $skill-name to inspect, or $skill-name <input> to stage a governed run.",
            state.skill_entries.len(),
            preview
        ),
    }
}

fn render_skill_inspect(state: &TuiInteractionState, skill_id: &str) -> String {
    if skill_known(state, skill_id) {
        let description =
            skill_description_for_state(state, &format!("${skill_id}"), state.language);
        let source = skill_source_for_state(state, skill_id);
        return match state.language {
            TuiLanguage::Zh => format!(
                "${skill_id}\n描述：{description}\n来源：{source}\n用法：${skill_id} <input>\nCLI: skills inspect {skill_id} --json"
            ),
            TuiLanguage::En => format!(
                "${skill_id}\nDescription: {description}\nSource: {source}\nUse: ${skill_id} <input>\nCLI: skills inspect {skill_id} --json"
            ),
        };
    }
    match state.language {
        TuiLanguage::Zh => format!("未知技能 `{skill_id}`。输入 $list 浏览已安装技能。"),
        TuiLanguage::En => {
            format!("Unknown skill `{skill_id}`. Type $list to browse installed skills.")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiOutputKind {
    Command,
    Test,
    Diff,
    Code,
    Error,
    Warning,
    Status,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiBlockStatus {
    Running,
    Passed,
    Failed,
    Warn,
    Info,
}

#[derive(Debug, Clone)]
struct TuiOutputBlock {
    kind: TuiOutputKind,
    title: String,
    status: TuiBlockStatus,
    lines: Vec<String>,
    collapsed: bool,
    hidden_count: usize,
    payload: Option<crate::tui_output::TuiStructuredPayload>,
}

fn render_latest_structured_output(state: &TuiInteractionState, logs_only: bool) -> String {
    let Some(body) = state
        .transcript
        .iter()
        .rev()
        .find(|turn| turn.role == "Astra" && !turn.body.trim().is_empty())
        .map(|turn| turn.body.as_str())
    else {
        return state
            .language
            .text(
                "结构化输出：当前没有可显示的命令、测试、diff 或日志块。",
                "Structured output: no command, test, diff, or log blocks are available.",
            )
            .to_string();
    };
    let blocks = structured_output_blocks(body, state.output_folded);
    let filtered = blocks
        .into_iter()
        .filter(|block| {
            !logs_only
                || matches!(
                    block.kind,
                    TuiOutputKind::Warning | TuiOutputKind::Error | TuiOutputKind::Status
                )
        })
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        return state
            .language
            .text(
                "日志：当前没有日志块。",
                "Logs: no log blocks are available.",
            )
            .to_string();
    }
    let title = if logs_only {
        state.language.text("日志", "Logs")
    } else {
        state.language.text("结构化输出", "Structured Output")
    };
    let mut lines = vec![title.to_string()];
    for block in filtered {
        push_output_block_text(&mut lines, &block, state.language);
    }
    lines.join("\n")
}

#[cfg(test)]
fn render_structured_output_text(body: &str, language: TuiLanguage, folded: bool) -> String {
    let mut lines = vec![language.text("结构化输出", "Structured Output").to_string()];
    for block in structured_output_blocks(body, folded) {
        push_output_block_text(&mut lines, &block, language);
    }
    lines.join("\n")
}

fn render_inline_repl_response_text(
    renderer: &TerminalMarkdownRenderer,
    response: &str,
    language: TuiLanguage,
    folded: bool,
) -> String {
    if response_has_structured_blocks(response, folded) {
        render_inline_structured_response_text(response, language, folded)
    } else {
        renderer.render(response)
    }
}

fn response_has_structured_blocks(response: &str, folded: bool) -> bool {
    structured_output_blocks(response, folded)
        .iter()
        .any(|block| !matches!(block.kind, TuiOutputKind::Text) || block.collapsed)
}

fn render_inline_structured_response_text(
    response: &str,
    language: TuiLanguage,
    folded: bool,
) -> String {
    let mut lines = Vec::new();
    for block in structured_output_blocks(response, folded) {
        push_inline_structured_block(&mut lines, &block, language);
    }
    lines.join("\n")
}

fn push_inline_structured_block(
    lines: &mut Vec<String>,
    block: &TuiOutputBlock,
    language: TuiLanguage,
) {
    lines.push(format!(
        "\x1b[2m╭─\x1b[0m {} \x1b[2m· {}\x1b[0m",
        localized_output_kind(block.kind, language),
        localized_block_status(block.status, language)
    ));
    lines.push(format!("\x1b[2m│\x1b[0m \x1b[1m{}\x1b[0m", block.title));
    for detail in structured_payload_summary_lines(block.payload.as_ref(), language) {
        lines.push(format!("\x1b[2m│\x1b[0m \x1b[2m{detail}\x1b[0m"));
    }
    for line in &block.lines {
        lines.push(format!("\x1b[2m│\x1b[0m {line}"));
    }
    if block.hidden_count > 0 {
        lines.push(format!(
            "\x1b[2m│\x1b[0m ... {} {} · {}",
            block.hidden_count,
            language.text("行已折叠", "lines folded"),
            language.text("/expand 展开", "/expand to open")
        ));
    }
    lines.push("\x1b[2m╰────────────────────────────────\x1b[0m".to_string());
}

fn push_output_block_text(lines: &mut Vec<String>, block: &TuiOutputBlock, language: TuiLanguage) {
    let status = localized_block_status(block.status, language);
    let fold_hint = if block.collapsed {
        format!(
            " · {} {}",
            language.text("已折叠", "folded"),
            language.text("输入 /expand 展开", "type /expand")
        )
    } else {
        String::new()
    };
    lines.push(format!(
        "[{}] {} · {}{}",
        localized_output_kind(block.kind, language),
        block.title,
        status,
        fold_hint
    ));
    for detail in structured_payload_summary_lines(block.payload.as_ref(), language) {
        lines.push(format!("  {detail}"));
    }
    for line in &block.lines {
        lines.push(format!("  {line}"));
    }
    if block.hidden_count > 0 {
        lines.push(format!(
            "  ... {} {}",
            block.hidden_count,
            language.text("行已折叠", "lines folded")
        ));
    }
}

fn structured_output_blocks(body: &str, folded: bool) -> Vec<TuiOutputBlock> {
    let fold_policy = if folded {
        crate::tui_output::FoldPolicy::FoldLong { visible_lines: 4 }
    } else {
        crate::tui_output::FoldPolicy::Expanded
    };
    crate::tui_output::blocks_from_legacy_text(body, fold_policy)
        .into_iter()
        .map(output_block_from_shared)
        .collect()
}

fn output_block_from_shared(block: crate::tui_output::TuiOutputBlock) -> TuiOutputBlock {
    let collapsed = block.fold.is_folded();
    let hidden_count = block.fold.hidden_count();
    let lines = block.visible_lines().to_vec();
    TuiOutputBlock {
        kind: output_kind_from_shared(block.kind),
        title: block.title,
        status: block_status_from_shared(block.status),
        lines,
        collapsed,
        hidden_count,
        payload: block.payload,
    }
}

fn structured_payload_summary_lines(
    payload: Option<&crate::tui_output::TuiStructuredPayload>,
    language: TuiLanguage,
) -> Vec<String> {
    use crate::tui_output::TuiStructuredPayload;

    match payload {
        Some(TuiStructuredPayload::Tool {
            command,
            stdout,
            stderr,
            summary,
            duration,
        }) => {
            let mut details = vec![format!(
                "{} {}",
                language.text("命令:", "command:"),
                command
            )];
            if !summary.trim().is_empty() && summary.trim() != command.trim() {
                details.push(format!(
                    "{} {}",
                    language.text("摘要:", "summary:"),
                    summary
                ));
            }
            details.push(format!(
                "{} {} / {}",
                language.text("输出:", "output:"),
                stdout.len(),
                stderr.len()
            ));
            if let Some(duration) = duration.as_deref().filter(|value| !value.is_empty()) {
                details.push(format!(
                    "{} {duration}",
                    language.text("耗时:", "duration:")
                ));
            }
            details
        }
        Some(TuiStructuredPayload::Code {
            language: code_language,
            file_path,
            line_range,
            line_count,
        }) => {
            let mut details = vec![format!(
                "{} {} · {} {}",
                language.text("语言:", "language:"),
                code_language,
                line_count,
                language.text("行", "lines")
            )];
            if let Some(path) = file_path.as_deref().filter(|value| !value.is_empty()) {
                details.push(format!("{} {path}", language.text("文件:", "file:")));
            }
            if let Some(range) = line_range.as_deref().filter(|value| !value.is_empty()) {
                details.push(format!("{} {range}", language.text("范围:", "range:")));
            }
            details
        }
        Some(TuiStructuredPayload::Diff {
            file_path,
            additions,
            deletions,
            hunks,
        }) => vec![format!(
            "{} {} · +{} -{} · {} {}",
            language.text("文件:", "file:"),
            file_path,
            additions,
            deletions,
            hunks,
            language.text("段", "hunks")
        )],
        Some(TuiStructuredPayload::Error {
            error_type,
            message,
            location,
            fix,
        }) => payload_problem_summary_lines(
            language.text("错误:", "error:"),
            error_type,
            message,
            location.as_deref(),
            fix.as_deref(),
            language,
        ),
        Some(TuiStructuredPayload::Warning {
            warning_type,
            message,
            location,
            suggestion,
        }) => payload_problem_summary_lines(
            language.text("警告:", "warning:"),
            warning_type,
            message,
            location.as_deref(),
            suggestion.as_deref(),
            language,
        ),
        Some(TuiStructuredPayload::Test {
            pass_count,
            total,
            failures,
            duration,
        }) => {
            let mut details = vec![format!(
                "{} {pass_count}/{total}",
                language.text("测试:", "tests:")
            )];
            if !failures.is_empty() {
                details.push(format!(
                    "{} {}",
                    language.text("失败:", "failures:"),
                    failures.join(", ")
                ));
            }
            if let Some(duration) = duration.as_deref().filter(|value| !value.is_empty()) {
                details.push(format!(
                    "{} {duration}",
                    language.text("耗时:", "duration:")
                ));
            }
            details
        }
        Some(TuiStructuredPayload::Status { action, metrics }) => {
            let mut details = vec![format!("{} {action}", language.text("动作:", "action:"))];
            if !metrics.is_empty() {
                details.push(format!(
                    "{} {}",
                    language.text("指标:", "metrics:"),
                    metrics.join(" · ")
                ));
            }
            details
        }
        Some(TuiStructuredPayload::ResearchEvidence {
            topic,
            source,
            confidence,
            citations,
            ..
        }) => vec![format!(
            "{} {} · {} {} · {} {} · {} {}",
            language.text("主题:", "topic:"),
            topic,
            language.text("来源:", "source:"),
            source,
            language.text("置信度:", "confidence:"),
            confidence,
            language.text("引用:", "citations:"),
            citations
        )],
        None => Vec::new(),
    }
}

fn payload_problem_summary_lines(
    label: &str,
    problem_type: &str,
    message: &str,
    location: Option<&str>,
    suggestion: Option<&str>,
    language: TuiLanguage,
) -> Vec<String> {
    let mut details = vec![format!("{label} {problem_type} · {message}")];
    if let Some(location) = location.filter(|value| !value.is_empty()) {
        details.push(format!(
            "{} {location}",
            language.text("位置:", "location:")
        ));
    }
    if let Some(suggestion) = suggestion.filter(|value| !value.is_empty()) {
        details.push(format!(
            "{} {suggestion}",
            language.text("建议:", "suggestion:")
        ));
    }
    details
}

fn output_kind_from_shared(kind: crate::tui_output::TuiOutputKind) -> TuiOutputKind {
    match kind {
        crate::tui_output::TuiOutputKind::Text | crate::tui_output::TuiOutputKind::Markdown => {
            TuiOutputKind::Text
        }
        crate::tui_output::TuiOutputKind::Tool | crate::tui_output::TuiOutputKind::Terminal => {
            TuiOutputKind::Command
        }
        crate::tui_output::TuiOutputKind::Diff => TuiOutputKind::Diff,
        crate::tui_output::TuiOutputKind::Test => TuiOutputKind::Test,
        crate::tui_output::TuiOutputKind::Error => TuiOutputKind::Error,
        crate::tui_output::TuiOutputKind::Warning => TuiOutputKind::Warning,
        crate::tui_output::TuiOutputKind::Status
        | crate::tui_output::TuiOutputKind::Artifact
        | crate::tui_output::TuiOutputKind::ResearchEvidence => TuiOutputKind::Status,
        crate::tui_output::TuiOutputKind::Code => TuiOutputKind::Code,
    }
}

fn block_status_from_shared(status: crate::tui_output::TuiBlockStatus) -> TuiBlockStatus {
    match status {
        crate::tui_output::TuiBlockStatus::Running => TuiBlockStatus::Running,
        crate::tui_output::TuiBlockStatus::Passed => TuiBlockStatus::Passed,
        crate::tui_output::TuiBlockStatus::Failed => TuiBlockStatus::Failed,
        crate::tui_output::TuiBlockStatus::Warn => TuiBlockStatus::Warn,
        crate::tui_output::TuiBlockStatus::Info => TuiBlockStatus::Info,
    }
}

fn localized_output_kind(kind: TuiOutputKind, language: TuiLanguage) -> &'static str {
    match kind {
        TuiOutputKind::Command => language.text("命令", "Command"),
        TuiOutputKind::Test => language.text("测试", "Test"),
        TuiOutputKind::Diff => "Diff",
        TuiOutputKind::Code => "Code",
        TuiOutputKind::Error => language.text("错误", "Error"),
        TuiOutputKind::Warning => language.text("警告", "Warning"),
        TuiOutputKind::Status => language.text("状态", "Status"),
        TuiOutputKind::Text => language.text("文本", "Text"),
    }
}

fn localized_block_status(status: TuiBlockStatus, language: TuiLanguage) -> &'static str {
    match status {
        TuiBlockStatus::Running => language.text("运行中", "running"),
        TuiBlockStatus::Passed => language.text("通过", "passed"),
        TuiBlockStatus::Failed => language.text("失败", "failed"),
        TuiBlockStatus::Warn => language.text("警告", "warning"),
        TuiBlockStatus::Info => language.text("信息", "info"),
    }
}

fn render_projected_action<F, P>(
    state: &mut TuiInteractionState,
    request: &SurfaceCommandRequest,
    config_executor: &mut F,
    permission_executor: &mut P,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String>,
{
    let action_id = request.action_id.as_deref().unwrap_or_default();
    match action_id {
        "submit_prompt" => {
            state.language.text(
                "对话模式已激活。可直接输入，或用 /prompt <text> 标记明确回合边界。",
                "Chat mode is active. Type directly, or use /prompt <text> for an explicit turn boundary.",
            ).to_string()
        }
        "exit_tui" => state.language.text(
            "退出命令已识别。真实交互循环会关闭 TUI；测试路由保持状态不退出进程。",
            "Exit command recognized. The real interactive loop closes the TUI; the test route keeps the process alive.",
        ).to_string(),
        "interrupt_turn" => {
            state.language.text(
                "当前没有运行中的回合。运行中可直接输入 /exc，或按 Esc/Ctrl-C 请求中断。",
                "No turn is currently running. During a turn, type /exc or press Esc/Ctrl-C to request interruption.",
            ).to_string()
        }
        "switch_session" => render_sessions_response(state),
        "show_history" => render_history_response(state, &request.args),
        "resume_session" => render_resume_response(state, &request.args, config_executor),
        "select_model" => render_model_selection_response(state, &request.args, config_executor),
        "select_reasoning" => render_reasoning_selection_response(state, &request.args, config_executor),
        "select_language" => {
            state.language.text(
                "语言命令：/language zh 或 /language en。当前默认中文。",
                "Language command: /language zh or /language en. Current language is English.",
            ).to_string()
        }
        "select_theme" => match state.language {
            TuiLanguage::Zh => "主题命令：/theme light 或 /theme dark。默认白天模式。".to_string(),
            TuiLanguage::En => "Theme command: /theme light or /theme dark. Default is light.".to_string(),
        },
        "inspect_permissions" => format!(
            "{}\n{} {}\n{} {}\nCLI: permissions pending --json. /approve <request-id> | /deny <request-id>.",
            state.language.text("权限状态", "Permission status"),
            state.language.text("待审批请求：", "Pending approvals:"),
            state.permission_count,
            state.language.text("权限模式：", "Permission mode:"),
            state.permission_mode,
        ),
        "approve_permission" | "deny_permission" => render_permission_decision_response(
            state,
            action_id,
            &request.args,
            permission_executor,
        ),
        "terminal_attach" => format!(
            "{}\n{} {}\n{}\nCLI: remote terminal attach --json",
            state
                .language
                .text("受控终端投影", "Governed terminal projection"),
            state.language.text("远程状态：", "Remote state:"),
            state.remote_state,
            state.language.text(
                "需要 active control lease；websocket 连接前需一次性 websocket ticket。",
                "Requires an active control lease; websocket connections must use a one-time websocket ticket.",
            )
        ),
        "terminal_replay" => {
            state.language.text(
                "终端回放投影。\nCLI: remote terminal replay --json.\n按 cursor 读取当前会话的只读受控滚屏。",
                "Terminal replay projection.\nCLI: remote terminal replay --json.\nReads read-only governed scrollback for the active session by cursor.",
            ).to_string()
        }
        "inspect_status" => format!(
            "{}\n{} {}\n{} {}\n{} {}\nCLI: inspect --json",
            state.language.text("状态", "Status"),
            state.language.text("会话数：", "Sessions:"),
            state.session_count,
            state.language.text("待审批：", "Pending approvals:"),
            state.permission_count,
            state.language.text("远程：", "Remote:"),
            state.remote_state
        ),
        "continue_session" => {
            let latest = vec!["latest".to_string()];
            render_resume_response(state, &latest, config_executor)
        }
        "inspect_diff" => state
            .language
            .text(
                "命令已暂存：查看当前工作区 diff。\nCLI: git diff --stat；完整 diff 用 git diff",
                "Command staged: inspect workspace diff.\nCLI: git diff --stat; use git diff for full details",
            )
            .to_string(),
        "stage_commit" => state
            .language
            .text(
                "命令已暂存：准备提交。\nCLI: git status --short；提交仍走 git commit",
                "Command staged: prepare commit.\nCLI: git status --short; commit remains git commit",
            )
            .to_string(),
        "inspect_cost" => state
            .language
            .text(
                "命令已暂存：查看成本摘要。\nCLI: cost --json",
                "Command staged: inspect cost summary.\nCLI: cost --json",
            )
            .to_string(),
        "inspect_usage" => state
            .language
            .text(
                "命令已暂存：查看用量。\nCLI: usage --json",
                "Command staged: inspect usage.\nCLI: usage --json",
            )
            .to_string(),
        "run_doctor" => state
            .language
            .text(
                "命令已暂存：运行诊断。\nCLI: doctor --json",
                "Command staged: run diagnostics.\nCLI: doctor --json",
            )
            .to_string(),
        "inspect_providers" => state
            .language
            .text(
                "命令已暂存：查看 provider。\nCLI: providers list --json",
                "Command staged: inspect providers.\nCLI: providers list --json",
            )
            .to_string(),
        "inspect_config" => state
            .language
            .text(
                "命令已暂存：查看配置。\nCLI: config effective --json",
                "Command staged: inspect config.\nCLI: config effective --json",
            )
            .to_string(),
        "inspect_tools" => state
            .language
            .text(
                "命令已暂存：查看工具。\nCLI: tools run read_file --path README.md --json",
                "Command staged: inspect tools.\nCLI: tools run read_file --path README.md --json",
            )
            .to_string(),
        "inspect_mcp" => state
            .language
            .text(
                "命令已暂存：查看 MCP。\nCLI: mcp list --json",
                "Command staged: inspect MCP.\nCLI: mcp list --json",
            )
            .to_string(),
        "fold_output" => {
            state.output_folded = true;
            state.language.text(
                "输出已折叠。输入 /expand 展开，或 /output 查看结构化摘要。",
                "Output folded. Type /expand to expand, or /output for the structured summary.",
            ).to_string()
        }
        "expand_output" => {
            state.output_folded = false;
            state.language.text(
                "输出已展开。输入 /fold 可重新折叠长输出。",
                "Output expanded. Type /fold to collapse long output again.",
            ).to_string()
        }
        "show_output" => render_latest_structured_output(state, false),
        "show_logs" => render_latest_structured_output(state, true),
        "open_artifact" => {
            state.language.text(
                "命令已暂存：打开产物。\nCLI: artifacts list --json, then artifacts inspect <path> --json.\n打开受控 diff、报告和结果预览。",
                "Command staged: open artifacts.\nCLI: artifacts list --json, then artifacts inspect <path> --json.\nOpens governed diffs, reports, and result previews.",
            ).to_string()
        }
        "inspect_memory" => {
            state.language.text(
                "命令已暂存：查看记忆。\nCLI: memory status --json.\n持久项目上下文和研究/代码记忆在显式打开前保持投影状态。",
                "Command staged: inspect memory.\nCLI: memory status --json.\nDurable project context and research/code recall remain a projection until explicitly opened.",
            ).to_string()
        }
        "open_research" => format!(
            "{}\n{}\n{}",
            state.language.text("研究简报", "Research brief"),
            state.language.text("研究", "Research"),
            state.research_line
        ),
        "open_research_board" => format!(
            "{}\n{}\n{}\nCLI: research board --json",
            state
                .language
                .text("Hermes 看板 Inspector", "Hermes board inspector"),
            state.language.text("同一份 host projection 派生；不写第二套看板状态。", "Derived from the same host projection; no second board state is written."),
            state.research_line
        ),
        "open_routines" => {
            state.language.text(
                "后台例程投影。\nCLI: routines list --json；routines inspect <routine-id> --json；routines ingress <routine-id> --trigger-kind <kind> --source <source> --json；routines run-due --json；routines retry <trigger-id> --json。\n入口、触发、agent 运行记录和 ProjectOps 监督租约保持同一条可追踪链路。",
                "Background routines projection.\nCLI: routines list --json; routines inspect <routine-id> --json; routines ingress <routine-id> --trigger-kind <kind> --source <source> --json; routines run-due --json; routines retry <trigger-id> --json.\nIngress, triggers, agent runtime records, and ProjectOps supervision leases stay on one traceable lane.",
            ).to_string()
        }
        "run_status" => state.language.text(
            "运行状态投影。\nCLI: run status --json。\n显示 active orchestration run、进度、恢复上下文和推荐控制动作。",
            "Run status projection.\nCLI: run status --json.\nShows the active orchestration run, progress, recovery context, and recommended control actions.",
        ).to_string(),
        "run_pause" => render_run_control_projection(state, "pause", None),
        "run_resume" => render_run_control_projection(state, "resume", None),
        "run_retry" => render_run_control_projection(
            state,
            "retry",
            request.args.first().map(String::as_str),
        ),
        "run_skip" => render_run_control_projection(
            state,
            "skip",
            request.args.first().map(String::as_str),
        ),
        "run_replan" => render_run_control_projection(state, "replan", None),
        "run_accept" => render_run_control_projection(state, "accept", None),
        "run_abort" => render_run_control_projection(state, "abort", None),
        _ => match state.language {
            TuiLanguage::Zh => format!("动作 {action_id} 已投影，但还没有 TUI 渲染器。"),
            TuiLanguage::En => format!("Action {action_id} is projected but has no TUI renderer yet."),
        },
    }
}

fn render_run_control_projection(
    state: &TuiInteractionState,
    action: &str,
    step_id: Option<&str>,
) -> String {
    let step = step_id
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    match state.language {
        TuiLanguage::Zh => format!(
            "运行控制已暂存：{action}{step}\nCLI: run {action}{step} --json\n该动作会写入 active orchestration run 的控制命令，并保留恢复上下文。"
        ),
        TuiLanguage::En => format!(
            "Run control staged: {action}{step}\nCLI: run {action}{step} --json\nThis writes a control command to the active orchestration run and preserves recovery context."
        ),
    }
}

fn skill_known(state: &TuiInteractionState, skill_id: &str) -> bool {
    state
        .skill_entries
        .iter()
        .any(|entry| entry == &format!("${skill_id}"))
}

const DEFAULT_HISTORY_LIMIT: usize = 20;

fn parse_history_count(raw: Option<&str>) -> Result<usize, String> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_HISTORY_LIMIT);
    };
    let parsed: usize = raw
        .parse()
        .map_err(|_| format!("history: invalid count '{raw}'. Expected a positive integer."))?;
    if parsed == 0 {
        return Err("history: count must be greater than 0.".to_string());
    }
    Ok(parsed)
}

fn format_history_timestamp(timestamp_ms: u64) -> String {
    let secs = timestamp_ms / 1_000;
    let subsec_ms = timestamp_ms % 1_000;
    let days_since_epoch = secs / 86_400;
    let seconds_of_day = secs % 86_400;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day % 3_600) / 60;
    let seconds = seconds_of_day % 60;

    let (year, month, day) = civil_from_days(i64::try_from(days_since_epoch).unwrap_or(0));
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{subsec_ms:03}Z")
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation
)]
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

fn render_prompt_history_report(entries: &[PromptHistoryEntry], limit: usize) -> String {
    if entries.is_empty() {
        return "Prompt history\n  Result           no prompts recorded yet".to_string();
    }

    let total = entries.len();
    let start = total.saturating_sub(limit);
    let shown = &entries[start..];
    let mut lines = vec![
        "Prompt history".to_string(),
        format!("  Total            {total}"),
        format!("  Showing          {} most recent", shown.len()),
        "  Reverse search   Ctrl-R in the REPL".to_string(),
        String::new(),
    ];
    for (offset, entry) in shown.iter().enumerate() {
        let absolute_index = start + offset + 1;
        let timestamp = format_history_timestamp(entry.timestamp_ms);
        let first_line = entry.text.lines().next().unwrap_or("").trim();
        let display = if first_line.chars().count() > 80 {
            let truncated: String = first_line.chars().take(77).collect();
            format!("{truncated}...")
        } else {
            first_line.to_string()
        };
        lines.push(format!(
            "{absolute_index:>4}. {timestamp}  {}",
            if display.is_empty() {
                "<empty prompt>"
            } else {
                &display
            }
        ));
    }
    lines.join("\n")
}

fn render_history_response(state: &TuiInteractionState, args: &[String]) -> String {
    let count = match parse_history_count(args.first().map(String::as_str)) {
        Ok(count) => count,
        Err(message) => return message,
    };
    render_prompt_history_report(&state.prompt_history, count)
}

fn render_sessions_response(state: &TuiInteractionState) -> String {
    if state.recent_sessions.is_empty() {
        return "Sessions\n  Result           no saved sessions for this project\n  Resume           /resume latest".to_string();
    }

    let mut lines = vec![
        "Sessions".to_string(),
        format!("  {} total", state.session_count),
        format!(
            "  Active           {}",
            state.active_session_id.as_deref().unwrap_or("none")
        ),
        "  Resume           /resume latest or /resume <session-id>".to_string(),
        String::new(),
    ];
    for session in &state.recent_sessions {
        let active_marker =
            if state.active_session_id.as_deref() == Some(session.session_id.as_str()) {
                "*"
            } else {
                " "
            };
        let title = session.title.as_deref().unwrap_or("(untitled)");
        lines.push(format!(
            "{active_marker} {}  {}  {}  {}",
            session.session_id, title, session.updated_at, session.status
        ));
    }
    lines.join("\n")
}

fn render_resume_response<F>(
    state: &mut TuiInteractionState,
    args: &[String],
    config_executor: &mut F,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
{
    let selector = args.first().map(String::as_str).unwrap_or("latest").trim();
    if selector.is_empty() {
        return state
            .language
            .text(
                "用法：/resume latest 或 /resume <session-id>。",
                "Usage: /resume latest or /resume <session-id>.",
            )
            .to_string();
    }

    match config_executor(TuiConfigAction {
        kind: TuiConfigActionKind::Session(TuiSessionActionKind::Resume),
        value: selector.to_string(),
    }) {
        Ok(result) if result.applied => {
            let session_id = result.session_id.unwrap_or_else(|| selector.to_string());
            state.active_session_id = Some(session_id.clone());
            match state.language {
                TuiLanguage::Zh => format!(
                    "会话已恢复：{session_id}\n范围：{}\n{}\n后续对话会进入该 session。",
                    result.scope, result.message
                ),
                TuiLanguage::En => format!(
                    "Session resumed: {session_id}\nScope: {}\n{}\nFollowing turns will enter that session.",
                    result.scope, result.message
                ),
            }
        }
        Ok(result) => result.message,
        Err(message) => match state.language {
            TuiLanguage::Zh => format!("恢复会话失败：{message}"),
            TuiLanguage::En => format!("Resume failed: {message}"),
        },
    }
}

fn render_model_selection_response<F>(
    state: &mut TuiInteractionState,
    args: &[String],
    config_executor: &mut F,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
{
    let language = state.language;
    let requested = normalize_model_args(args);
    let Some(model) = requested else {
        return match language {
            TuiLanguage::Zh => [
            "模型选择",
            &format!("当前：{}", state.model_label),
            "推荐：/model gpt-5.5（最高质量） · /model gpt-5.4（稳健） · /model gpt-5.4-mini（快速）",
            "Provider：会根据模型目录自动路由，例如 gpt-* → openai，claude-* → anthropic。",
            "应用：输入 /model <model>，TUI 会写入项目配置并刷新后续对话模型。",
            "思考强度：/reasoning auto|low|medium|high。",
        ]
        .join("\n"),
            TuiLanguage::En => [
            "Model selection",
            &format!("Current: {}", state.model_label),
            "Recommended: /model gpt-5.5 (best quality) · /model gpt-5.4 (balanced) · /model gpt-5.4-mini (fast)",
            "Provider: routed automatically from the catalog, for example gpt-* -> openai and claude-* -> anthropic.",
            "Apply: type /model <model>; the TUI writes project config and refreshes following turns.",
            "Reasoning effort: /reasoning auto|low|medium|high.",
        ]
        .join("\n"),
        };
    };

    match config_executor(TuiConfigAction {
        kind: TuiConfigActionKind::Model,
        value: model.clone(),
    }) {
        Ok(result) if result.applied => {
            let applied_model = result.model.unwrap_or(model);
            let label = match result.provider_id {
                Some(provider_id) if !provider_id.is_empty() => {
                    format!("{provider_id}/{applied_model}")
                }
                _ => applied_model,
            };
            state.model_label = label.clone();
            match language {
                TuiLanguage::Zh => format!(
                    "模型已应用：{label}\n范围：{}\n{}\n后续对话会使用该模型；provider 会由模型目录自动路由。",
                    result.scope, result.message
                ),
                TuiLanguage::En => format!(
                    "Model applied: {label}\nScope: {}\n{}\nFollowing turns will use this model; the provider is routed from the model catalog.",
                    result.scope, result.message
                ),
            }
        }
        Ok(result) => result.message,
        Err(message) => match language {
            TuiLanguage::Zh => format!("模型设置失败：{message}"),
            TuiLanguage::En => format!("Model selection failed: {message}"),
        },
    }
}

fn normalize_model_args(args: &[String]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let joined = args.join(" ");
    let normalized = joined
        .trim()
        .replace("gpt 5.5", "gpt-5.5")
        .replace("gpt 5.4", "gpt-5.4")
        .replace("gpt 5", "gpt-5")
        .replace("claude sonnet", "claude-sonnet")
        .replace(' ', "-");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn render_reasoning_selection_response<F>(
    state: &mut TuiInteractionState,
    args: &[String],
    config_executor: &mut F,
) -> String
where
    F: FnMut(TuiConfigAction) -> Result<TuiConfigActionResult, String>,
{
    let language = state.language;
    let effort = args.first().map(String::as_str);
    match (language, effort) {
        (TuiLanguage::Zh, None) => [
            "思考强度",
            &format!("当前：{}", state.reasoning_effort),
            "可选：auto、low、medium、high。",
            "建议：日常代码用 medium，复杂重构或 research 推理用 high，快速小改用 low。",
            "应用：输入 /reasoning <value>，TUI 会写入项目配置并刷新后续对话策略。",
        ]
        .join("\n"),
        (TuiLanguage::En, None) => [
            "Reasoning effort",
            &format!("Current: {}", state.reasoning_effort),
            "Options: auto, low, medium, high.",
            "Recommendation: medium for daily coding, high for complex refactors or research reasoning, low for quick edits.",
            "Apply: type /reasoning <value>; the TUI writes project config and refreshes following turns.",
        ]
        .join("\n"),
        (language, Some(value)) if is_valid_reasoning_effort(value) => {
            match config_executor(TuiConfigAction {
                kind: TuiConfigActionKind::Reasoning,
                value: value.to_string(),
            }) {
                Ok(result) if result.applied => {
                    let effort = result
                        .reasoning_effort
                        .unwrap_or_else(|| value.to_string());
                    state.reasoning_effort = effort.clone();
                    match language {
                        TuiLanguage::Zh => format!(
                            "思考强度已应用：{effort}\n范围：{}\n{}\n后续对话会使用该强度。",
                            result.scope, result.message
                        ),
                        TuiLanguage::En => format!(
                            "Reasoning effort applied: {effort}\nScope: {}\n{}\nFollowing turns will use this effort.",
                            result.scope, result.message
                        ),
                    }
                }
                Ok(result) => result.message,
                Err(message) => match language {
                    TuiLanguage::Zh => format!("思考强度设置失败：{message}"),
                    TuiLanguage::En => format!("Reasoning effort failed: {message}"),
                },
            }
        }
        (TuiLanguage::Zh, Some(value)) => {
            format!("未知思考强度 `{value}`。可选：auto、low、medium、high。")
        }
        (TuiLanguage::En, Some(value)) => {
            format!("Unknown reasoning effort `{value}`. Choose auto, low, medium, or high.")
        }
    }
}

fn is_valid_reasoning_effort(value: &str) -> bool {
    matches!(value, "auto" | "low" | "medium" | "high")
}

fn render_permission_decision_response<P>(
    state: &mut TuiInteractionState,
    action_id: &str,
    args: &[String],
    permission_executor: &mut P,
) -> String
where
    P: FnMut(TuiPermissionAction) -> Result<TuiPermissionActionResult, String>,
{
    let language = state.language;
    let decision = if action_id == "approve_permission" {
        TuiPermissionDecision::Approve
    } else {
        TuiPermissionDecision::Deny
    };
    let command = decision.as_command();
    let request_id = match args {
        [request_id] if !request_id.trim().is_empty() => request_id.trim().to_string(),
        [] => {
            return match language {
                TuiLanguage::Zh => {
                    format!("用法：/{command} <request-id>\n先用 /permissions 查看待审批请求。")
                }
                TuiLanguage::En => {
                    format!(
                        "Usage: /{command} <request-id>\nUse /permissions to inspect pending requests first."
                    )
                }
            };
        }
        _ => {
            return match language {
                TuiLanguage::Zh => {
                    format!("用法：/{command} <request-id>\n一次只能处理一个 request id。")
                }
                TuiLanguage::En => {
                    format!("Usage: /{command} <request-id>\nResolve one request id at a time.")
                }
            };
        }
    };

    match permission_executor(TuiPermissionAction {
        decision,
        request_id: request_id.clone(),
    }) {
        Ok(result) => {
            if let Some(pending_count) = result.pending_count {
                state.permission_count = pending_count.to_string();
            }
            match language {
                TuiLanguage::Zh => format!(
                    "权限请求已处理\nrequest id: {}\ndecision: {}\n待审批请求: {}\n{}\n下一步：重试触发该请求的操作；若已拒绝，请重新发起安全版本。",
                    result.request_id,
                    result.decision,
                    state.permission_count,
                    result.message
                ),
                TuiLanguage::En => format!(
                    "Permission request resolved\nrequest id: {}\ndecision: {}\npending approvals: {}\n{}\nNext: retry the operation that created this request; if denied, start a safer version.",
                    result.request_id,
                    result.decision,
                    state.permission_count,
                    result.message
                ),
            }
        }
        Err(message) => match language {
            TuiLanguage::Zh => {
                format!("权限请求处理失败：{message}\nrequest id: {request_id}\n下一步：输入 /permissions 刷新待审批请求。")
            }
            TuiLanguage::En => {
                format!("Permission request failed: {message}\nrequest id: {request_id}\nNext: type /permissions to refresh pending requests.")
            }
        },
    }
}

#[allow(dead_code)]
fn legacy_skill_preview(state: &TuiInteractionState) -> String {
    if state.skill_entries.is_empty() {
        String::new()
    } else {
        let preview = state
            .skill_entries
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "Skills: {preview}{}",
            if state.skill_entries.len() > 8 {
                " ..."
            } else {
                ""
            }
        )
    }
}

#[cfg(test)]
fn render_terminal_frame(model: &TerminalFrameModel, size: TerminalSize) -> String {
    let width = if size.cols == 0 {
        72
    } else {
        usize::from(size.cols).max(20)
    };
    let height = if size.rows == 0 {
        20
    } else {
        usize::from(size.rows).max(8)
    };
    let inner = width.saturating_sub(4).max(1);
    let info = research_info_segments(model);
    let mut lines = Vec::new();
    lines.push("\x1b[2J\x1b[H".to_string());
    lines.push(top_border(width));
    lines.push(row(
        width,
        &accent(&format!(
            "{}  ·  {}  ·  {}",
            model.language.text("Astra Code 代码智能体", "Astra Code"),
            model
                .language
                .text("对话编程", "Conversational coding agent"),
            tui_mode_label(model)
        )),
    ));
    lines.push(row(
        width,
        &fit(
            &format!(
                "{} {}  {} {}  {} {}  {} {}",
                model.language.text("项目", "Project"),
                model.project_id,
                model.language.text("远程", "Remote"),
                model.remote_state,
                model.language.text("权限", "Permission"),
                model.permission_mode,
                model.language.text("主题", "Theme"),
                theme_indicator(model)
            ),
            inner,
        ),
    ));
    lines.push(row(
        width,
        &fit(
            &format!(
                "{}  {} {}  ›  {} {}",
                model.language.text("研究", "Research"),
                info.thread_label.trim(),
                info.thread,
                info.stage_label.trim(),
                info.stage
            ),
            inner,
        ),
    ));
    if let Some((run_id, status, progress)) = orchestration_info_segments(model) {
        lines.push(row(
            width,
            &fit(
                &format!(
                    "{}  {}  ·  {}  ·  {}",
                    model.language.text("运行", "Run"),
                    run_id,
                    status,
                    progress
                ),
                inner,
            ),
        ));
    }
    lines.push(row(
        width,
        &fit(
            model.language.text(
                "Ask Astra  直接描述任务 | / 命令 | $ 技能 | 中文可粘贴提交",
                "Ask Astra  describe the task | / commands | $ skills | paste works for IME text",
            ),
            inner,
        ),
    ));
    lines.push(separator(width));
    let mut content_lines = Vec::new();
    if model.transcript.is_empty() {
        content_lines.push(format!(
            "Astra   {}",
            model.language.text(
                "就绪。直接输入需求，或输入 / 打开命令，$ 打开技能。",
                "Ready. Type a request, / for commands, or $ for skills.",
            )
        ));
    }
    for turn in model.transcript.iter().rev().take(8).rev() {
        push_transcript_turn_lines(&mut content_lines, turn);
    }
    if command_palette_active(&model.composer) {
        content_lines.push("".to_string());
        content_lines.extend(command_palette_lines(model));
    } else if skill_palette_active(&model.composer) {
        content_lines.push("".to_string());
        content_lines.extend(skill_palette_lines(model));
    }
    content_lines.push("".to_string());
    content_lines.extend(composer_lines(model));
    for content in content_lines {
        lines.push(row(width, &fit(&content, inner)));
    }

    while lines.len() < height.saturating_sub(3) {
        lines.push(row(width, ""));
    }
    lines.push(separator(width));
    lines.push(row(width, &fit(&localized_footer(model), inner)));
    lines.push(bottom_border(width));
    lines.join("\r\n")
}

fn render_ratatui_frame(frame: &mut Frame<'_>, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface)),
        frame.area(),
    );
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(
                if command_palette_active(&model.composer) || skill_palette_active(&model.composer)
                {
                    11
                } else {
                    6
                },
            ),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_ratatui_header(frame, root[0], model);
    render_ratatui_messages(frame, root[1], model);
    render_ratatui_composer(frame, root[2], model);
    render_ratatui_footer(frame, root[3], model);
}

fn render_ratatui_header(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let info = research_info_segments(model);
    let theme = model.theme_mode.palette();
    let divider = Style::default().fg(theme.border).bg(theme.surface);
    let label = Style::default().fg(theme.muted).bg(theme.surface);
    let primary = Style::default().fg(theme.text).bg(theme.surface);
    let accent = Style::default().fg(theme.accent).bg(theme.surface);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "Astra",
            Style::default()
                .fg(theme.accent)
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", divider),
        Span::styled(model.language.text("项目 ", "project "), label),
        Span::styled(truncate_plain(&model.project_id, 28), primary),
        Span::styled("  │  ", divider),
        Span::styled(model.language.text("远程 ", "remote "), label),
        Span::styled(model.remote_state.clone(), primary),
        Span::styled("  │  ", divider),
        Span::styled(model.language.text("权限 ", "permission "), label),
        Span::styled(
            compact_permission_mode(&model.permission_mode, model.language),
            primary,
        ),
        Span::styled("  │  ", divider),
        Span::styled("model ", label),
        Span::styled(truncate_plain(&model.model_label, 18), primary),
        Span::styled("  │  ", divider),
        Span::styled(model.language.text("主题 ", "theme "), label),
        Span::styled(theme_indicator(model), accent),
    ])];
    if info.state != "available" && info.state != "none" {
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default().bg(theme.surface)),
            Span::styled(info.thread_label, label),
            Span::styled(info.thread, primary),
            Span::styled("  ›  ", divider),
            Span::styled(info.stage_label, label),
            Span::styled(info.stage, accent),
            Span::styled("  ·  ", divider),
            Span::styled(info.state, label),
        ]));
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(usize::from(area.width.max(1))),
        divider,
    )));
    let paragraph = Paragraph::new(lines).style(Style::default().bg(theme.surface));
    frame.render_widget(paragraph, area);
}

fn render_ratatui_messages(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    let mut items = Vec::new();
    if model.transcript.is_empty() {
        items.push(ListItem::new(vec![
            Line::from(Span::styled(
                "Astra",
                Style::default()
                    .fg(theme.accent)
                    .bg(theme.surface)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                model
                    .language
                    .text(
                        "就绪。直接输入需求，或输入 / 打开命令，$ 打开技能。",
                        "Ready. Type a request, / for commands, or $ for skills.",
                    )
                    .to_string(),
                Style::default().fg(theme.muted).bg(theme.surface),
            )),
        ]));
    }
    let visible_turns = usize::from(area.height.saturating_sub(1)).max(1) / 4 + 2;
    for turn in model.transcript.iter().rev().take(visible_turns).rev() {
        let mut lines = vec![Line::from(Span::styled(
            role_label(turn.role),
            Style::default()
                .fg(role_color(turn.role, theme))
                .bg(theme.surface)
                .add_modifier(Modifier::BOLD),
        ))];
        lines.extend(render_message_body_lines(
            &turn.body,
            theme,
            model.language,
            model.output_folded,
        ));
        items.push(ListItem::new(lines).style(Style::default().fg(theme.text).bg(theme.surface)));
        items.push(ListItem::new(Line::from(Span::styled(
            "",
            Style::default().bg(theme.surface),
        ))));
    }
    let list = List::new(items).highlight_style(Style::default().bg(theme.panel_alt));
    frame.render_widget(list, area);
}

fn render_ratatui_composer(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    if command_palette_active(&model.composer) {
        render_ratatui_command_palette(frame, area, model);
        return;
    }
    if skill_palette_active(&model.composer) {
        render_ratatui_skill_palette(frame, area, model);
        return;
    }
    let theme = model.theme_mode.palette();
    let body = if model.composer.is_empty() {
        model.language.text(
            "让 Astra 修改、调试、解释代码，或推进研究任务",
            "Ask Astra to edit, debug, explain, or run a research task",
        )
    } else {
        &model.composer
    };
    let border_color = if model.composer.is_empty() {
        theme.border
    } else {
        theme.accent
    };
    let prompt_style = Style::default()
        .fg(theme.accent)
        .bg(theme.panel)
        .add_modifier(Modifier::BOLD);
    let body_style = if model.composer.is_empty() {
        Style::default().fg(theme.faint).bg(theme.panel)
    } else {
        Style::default().fg(theme.text).bg(theme.panel)
    };
    let mut composer_lines = vec![
        Line::from(vec![
            Span::styled("› ", prompt_style),
            Span::styled(body.to_string(), body_style),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default().bg(theme.panel)),
            Span::styled(
                model.language.text("/ 命令", "/ commands"),
                Style::default().fg(theme.accent).bg(theme.panel),
            ),
            Span::styled("   ", Style::default().bg(theme.panel)),
            Span::styled(
                model.language.text("$ 技能", "$ skills"),
                Style::default().fg(theme.accent).bg(theme.panel),
            ),
            Span::styled("   ", Style::default().bg(theme.panel)),
            Span::styled(
                model.language.text("Enter 发送", "Enter send"),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default().bg(theme.panel)),
            Span::styled(
                model.language.text(
                    "中文输入：在系统输入框确认后粘贴，显示最稳定",
                    "IME text: confirm in your OS field, then paste",
                ),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default().bg(theme.panel)),
            Span::styled(
                model.language.text("模型 ", "model "),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
            Span::styled(
                model.model_label.clone(),
                Style::default().fg(theme.muted).bg(theme.panel),
            ),
            Span::styled(" · ", Style::default().fg(theme.border).bg(theme.panel)),
            Span::styled(
                model.language.text("推理 ", "reasoning "),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
            Span::styled(
                model.reasoning_effort.clone(),
                Style::default().fg(theme.muted).bg(theme.panel),
            ),
            Span::styled(" · ", Style::default().fg(theme.border).bg(theme.panel)),
            Span::styled(
                model.language.text("目录 ", "dir "),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
            Span::styled(
                truncate_plain(&model.working_dir, 30),
                Style::default().fg(theme.muted).bg(theme.panel),
            ),
            Span::styled(" · ", Style::default().fg(theme.border).bg(theme.panel)),
            Span::styled(
                model.language.text("分支 ", "branch "),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
            Span::styled(
                model.git_branch.clone(),
                Style::default().fg(theme.muted).bg(theme.panel),
            ),
            Span::styled(" · ", Style::default().fg(theme.border).bg(theme.panel)),
            Span::styled(
                model.language.text("工作区 ", "workspace "),
                Style::default().fg(theme.faint).bg(theme.panel),
            ),
            Span::styled(
                truncate_plain(&model.project_id, 18),
                Style::default().fg(theme.muted).bg(theme.panel),
            ),
        ]),
    ];
    // Research progress bar (only when active research exists)
    let research = research_info_segments(model);
    if research.active {
        composer_lines.push(render_research_progress_bar(
            &research,
            model.language,
            theme,
        ));
    }
    if let Some((run_id, status, progress)) = orchestration_info_segments(model) {
        composer_lines.push(render_orchestration_progress_bar(
            &run_id,
            &status,
            &progress,
            model.language,
            theme,
        ));
    }
    let paragraph = Paragraph::new(composer_lines)
        .block(
            Block::default()
                .title(" Ask Astra ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color).bg(theme.panel))
                .style(Style::default().bg(theme.panel)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_ratatui_command_palette(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(3)])
        .split(area);
    let entries = matching_command_entries(&model.command_entries, &model.composer);
    let selected = model.overlay_selected.min(entries.len().saturating_sub(1));
    let visible_rows = palette_visible_rows(chunks[0].height)
        .saturating_sub(1)
        .max(1);
    let visible_entries = visible_entry_window(&entries, selected, visible_rows);
    let header = ListItem::new(Line::from(vec![
        Span::styled("◉ ", Style::default().fg(theme.accent)),
        Span::styled(
            model.language.text("命令", "Commands"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  ↑↓ {} · Enter {} · {}",
                model.language.text("选择", "select"),
                model.language.text("采用", "apply"),
                model.language.text("输入过滤", "type filters")
            ),
            Style::default().fg(theme.faint),
        ),
    ]));
    let items = entries
        .iter()
        .enumerate()
        .skip(visible_entries.start)
        .take(visible_entries.len)
        .map(|(index, entry)| {
            let selected_row = index == selected;
            let style = palette_row_style(theme, selected_row);
            let summary_style = palette_summary_style(theme, selected_row);
            let bar = if selected_row {
                Span::styled("┃ ", Style::default().fg(theme.accent))
            } else {
                Span::styled("  ", Style::default().fg(theme.faint))
            };
            ListItem::new(Line::from(vec![
                bar,
                Span::styled(format!("{:<18}", entry.typed), style),
                Span::styled(" ", style),
                Span::styled(localized_command_label(entry, model.language), style),
                Span::styled("  ", style),
                Span::styled(
                    localized_command_summary(entry, model.language),
                    summary_style,
                ),
            ]))
            .style(style)
        })
        .collect::<Vec<_>>();
    let mut all_items = vec![header];
    all_items.extend(items);
    let list = List::new(all_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent).bg(theme.panel))
            .style(Style::default().bg(theme.panel)),
    );
    frame.render_widget(list, chunks[0]);
    render_ratatui_inline_composer(frame, chunks[1], model);
}

fn render_ratatui_skill_palette(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(3)])
        .split(area);
    let entries = matching_skill_entries(&model.skill_entries, &model.composer);
    let selected = model.overlay_selected.min(entries.len().saturating_sub(1));
    let visible_rows = palette_visible_rows(chunks[0].height)
        .saturating_sub(1)
        .max(1);
    let visible_entries = visible_entry_window(&entries, selected, visible_rows);
    let header = ListItem::new(Line::from(vec![
        Span::styled("◉ ", Style::default().fg(theme.accent)),
        Span::styled(
            model.language.text("技能", "Skills"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  ↑↓ {} · Enter {} · {}",
                model.language.text("选择", "select"),
                model.language.text("采用", "apply"),
                model.language.text("输入过滤", "type filters")
            ),
            Style::default().fg(theme.faint),
        ),
    ]));
    let items = entries
        .iter()
        .enumerate()
        .skip(visible_entries.start)
        .take(visible_entries.len)
        .map(|(index, entry)| {
            let selected_row = index == selected;
            let style = palette_row_style(theme, selected_row);
            let summary_style = palette_summary_style(theme, selected_row);
            let bar = if selected_row {
                Span::styled("┃ ", Style::default().fg(theme.accent))
            } else {
                Span::styled("  ", Style::default().fg(theme.faint))
            };
            ListItem::new(Line::from(vec![
                bar,
                Span::styled(format!("{entry:<26}"), style),
                Span::styled(skill_summary(entry, model.language), summary_style),
            ]))
            .style(style)
        })
        .collect::<Vec<_>>();
    let mut all_items = vec![header];
    all_items.extend(items);
    let list = List::new(all_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent).bg(theme.panel))
            .style(Style::default().bg(theme.panel)),
    );
    frame.render_widget(list, chunks[0]);
    render_ratatui_inline_composer(frame, chunks[1], model);
}

fn palette_visible_rows(area_height: u16) -> usize {
    usize::from(area_height.saturating_sub(2)).max(1)
}

fn palette_row_style(theme: TuiThemePalette, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme.accent)
            .bg(theme.panel_alt)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text).bg(theme.panel)
    }
}

fn palette_summary_style(theme: TuiThemePalette, selected: bool) -> Style {
    if selected {
        Style::default().fg(theme.text).bg(theme.panel_alt)
    } else {
        Style::default().fg(theme.faint).bg(theme.panel)
    }
}

fn render_ratatui_inline_composer(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "› ",
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            model.composer.clone(),
            Style::default().fg(theme.text).bg(theme.panel),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border).bg(theme.panel))
            .style(Style::default().bg(theme.panel)),
    );
    frame.render_widget(paragraph, area);
}

fn render_ratatui_footer(frame: &mut Frame<'_>, area: Rect, model: &TerminalFrameModel) {
    let theme = model.theme_mode.palette();
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled("● ", Style::default().fg(theme.accent).bg(theme.surface)),
        Span::styled(
            format!(
                "{} {}",
                model.language.text("权限", "permission"),
                compact_permission_mode(&model.permission_mode, model.language)
            ),
            Style::default().fg(theme.muted).bg(theme.surface),
        ),
        Span::styled("  |  ", Style::default().fg(theme.border).bg(theme.surface)),
        Span::styled(
            format!(
                "{} {}",
                model.language.text("远程", "remote"),
                model.remote_state
            ),
            Style::default().fg(theme.muted).bg(theme.surface),
        ),
        Span::styled("  |  ", Style::default().fg(theme.border).bg(theme.surface)),
        Span::styled(
            theme_indicator(model),
            Style::default().fg(theme.muted).bg(theme.surface),
        ),
        Span::styled("  |  ", Style::default().fg(theme.border).bg(theme.surface)),
        Span::styled(
            format!("reasoning {}", model.reasoning_effort),
            Style::default().fg(theme.muted).bg(theme.surface),
        ),
        Span::styled("  |  ", Style::default().fg(theme.border).bg(theme.surface)),
        Span::styled(
            localized_footer(model),
            Style::default().fg(theme.muted).bg(theme.surface),
        ),
    ]))
    .alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

fn render_message_body_lines(
    body: &str,
    theme: TuiThemePalette,
    language: TuiLanguage,
    folded: bool,
) -> Vec<Line<'static>> {
    let blocks = structured_output_blocks(body, folded);
    if blocks.iter().any(|block| {
        !matches!(block.kind, TuiOutputKind::Text) || block.collapsed || block.lines.len() > 1
    }) {
        let mut rendered = Vec::new();
        for block in blocks {
            let icon = output_kind_icon(block.kind);
            let icon_color = output_kind_icon_color(block.kind);
            let border = output_kind_border_color(block.kind);
            let is_text = matches!(block.kind, TuiOutputKind::Text);
            let border_fg = if is_text { theme.border } else { border };
            let icon_span = if icon.is_empty() {
                vec![]
            } else {
                vec![Span::styled(
                    format!("{icon} "),
                    Style::default().fg(icon_color).bg(theme.surface),
                )]
            };
            rendered.push(Line::from({
                let mut spans = vec![Span::styled(
                    "  ╭─ ",
                    Style::default().fg(border_fg).bg(theme.surface),
                )];
                spans.extend(icon_span);
                spans.push(Span::styled(
                    format!(
                        "{} · {} · {}",
                        localized_output_kind(block.kind, language),
                        block.title,
                        localized_block_status(block.status, language)
                    ),
                    Style::default()
                        .fg(output_kind_color(block.kind, block.status, theme))
                        .bg(theme.surface)
                        .add_modifier(Modifier::BOLD),
                ));
                spans
            }));
            for line in &block.lines {
                rendered.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(border_fg).bg(theme.surface)),
                    Span::styled(
                        line.to_string(),
                        inline_box_line_style(line, theme).bg(theme.surface),
                    ),
                ]));
            }
            for detail in structured_payload_summary_lines(block.payload.as_ref(), language) {
                rendered.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(border_fg).bg(theme.surface)),
                    Span::styled(detail, Style::default().fg(theme.faint).bg(theme.surface)),
                ]));
            }
            if block.hidden_count > 0 {
                rendered.push(Line::from(vec![
                    Span::styled("  │ ", Style::default().fg(border_fg).bg(theme.surface)),
                    Span::styled(
                        format!(
                            "... {} {} · {}",
                            block.hidden_count,
                            language.text("行已折叠", "lines folded"),
                            language.text("/expand 展开", "/expand to open")
                        ),
                        Style::default().fg(theme.faint).bg(theme.surface),
                    ),
                ]));
            }
            rendered.push(Line::from(vec![Span::styled(
                "  ╰────────────────────────────────",
                Style::default().fg(border_fg).bg(theme.surface),
            )]));
        }
        return rendered;
    }
    let mut rendered = Vec::new();
    let mut box_open = false;
    for raw in body.lines() {
        let line = raw.trim_end();
        let boxed = is_inline_box_line(line);
        if boxed && !box_open {
            rendered.push(Line::from(vec![Span::styled(
                "  ╭─ output",
                Style::default().fg(theme.border).bg(theme.surface),
            )]));
            box_open = true;
        } else if !boxed && box_open {
            rendered.push(Line::from(vec![Span::styled(
                "  ╰────────────────────────────────",
                Style::default().fg(theme.border).bg(theme.surface),
            )]));
            box_open = false;
        }

        if boxed {
            rendered.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(theme.border).bg(theme.surface)),
                Span::styled(
                    line.to_string(),
                    inline_box_line_style(line, theme).bg(theme.surface),
                ),
            ]));
        } else if is_streaming_status_line(line) {
            rendered.push(Line::from(vec![
                Span::styled("  ", Style::default().bg(theme.surface)),
                Span::styled("● ", Style::default().fg(theme.accent).bg(theme.surface)),
                Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(theme.accent)
                        .bg(theme.surface)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            rendered.push(Line::from(vec![
                Span::styled("  ", Style::default().bg(theme.surface)),
                Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.text).bg(theme.surface),
                ),
            ]));
        }
    }
    if box_open {
        rendered.push(Line::from(vec![Span::styled(
            "  ╰────────────────────────────────",
            Style::default().fg(theme.border).bg(theme.surface),
        )]));
    }
    rendered
}

fn output_kind_icon(kind: TuiOutputKind) -> &'static str {
    match kind {
        TuiOutputKind::Command => "●",
        TuiOutputKind::Code => "◆",
        TuiOutputKind::Error => "✕",
        TuiOutputKind::Warning => "△",
        TuiOutputKind::Status => "○",
        TuiOutputKind::Diff => "◊",
        TuiOutputKind::Test => "✔",
        TuiOutputKind::Text => "",
    }
}

fn output_kind_icon_color(kind: TuiOutputKind) -> Color {
    match kind {
        TuiOutputKind::Command => Color::Rgb(212, 136, 10),
        TuiOutputKind::Code => Color::Rgb(129, 140, 248),
        TuiOutputKind::Error => Color::Rgb(224, 92, 83),
        TuiOutputKind::Warning => Color::Rgb(234, 179, 8),
        TuiOutputKind::Status => Color::Rgb(56, 189, 248),
        TuiOutputKind::Diff => Color::Rgb(108, 184, 103),
        TuiOutputKind::Test => Color::Rgb(108, 184, 103),
        TuiOutputKind::Text => Color::Rgb(200, 196, 188),
    }
}

fn output_kind_border_color(kind: TuiOutputKind) -> Color {
    match kind {
        TuiOutputKind::Command => Color::Rgb(212, 136, 10),
        TuiOutputKind::Code => Color::Rgb(129, 140, 248),
        TuiOutputKind::Error => Color::Rgb(224, 92, 83),
        TuiOutputKind::Warning => Color::Rgb(234, 179, 8),
        TuiOutputKind::Status => Color::Rgb(56, 189, 248),
        TuiOutputKind::Diff => Color::Rgb(108, 184, 103),
        TuiOutputKind::Test => Color::Rgb(56, 189, 248),
        TuiOutputKind::Text => Color::Rgb(34, 34, 48),
    }
}

fn output_kind_color(kind: TuiOutputKind, status: TuiBlockStatus, theme: TuiThemePalette) -> Color {
    match status {
        TuiBlockStatus::Passed => theme.success,
        TuiBlockStatus::Failed => theme.danger,
        TuiBlockStatus::Warn => theme.accent,
        TuiBlockStatus::Running => theme.accent,
        TuiBlockStatus::Info => match kind {
            TuiOutputKind::Error => theme.danger,
            TuiOutputKind::Warning => theme.accent,
            TuiOutputKind::Diff => theme.success,
            TuiOutputKind::Code => Color::Rgb(129, 140, 248),
            TuiOutputKind::Status => Color::Rgb(56, 189, 248),
            TuiOutputKind::Test => theme.success,
            _ => theme.text,
        },
    }
}

fn is_inline_box_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("$ ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("- ")
        || trimmed.starts_with("RUN ")
        || trimmed.starts_with("running ")
        || trimmed.starts_with("test ")
        || trimmed.starts_with("error:")
        || trimmed.starts_with("warning:")
        || trimmed.contains(" | ")
        || trimmed.contains(" passed")
        || trimmed.contains(" failed")
        || trimmed.contains("Compiling ")
        || trimmed.contains("Finished ")
}

fn is_streaming_status_line(line: &str) -> bool {
    line.contains("正在运行")
        || line.contains("Token 级流式输出")
        || line.contains("Streaming")
        || line.contains("running")
}

fn inline_box_line_style(line: &str, theme: TuiThemePalette) -> Style {
    let trimmed = line.trim_start();
    let color = if trimmed.starts_with("+")
        || trimmed.contains(" ok")
        || trimmed.contains("passed")
        || trimmed.contains("完成")
    {
        theme.success
    } else if trimmed.starts_with("-") || trimmed.contains("failed") || trimmed.contains("error") {
        theme.danger
    } else if trimmed.starts_with("$") || trimmed.starts_with(">") {
        theme.accent
    } else {
        theme.muted
    };
    Style::default().fg(color)
}

fn render_research_progress_bar(
    research: &ResearchInfoSegments,
    _language: TuiLanguage,
    theme: TuiThemePalette,
) -> Line<'static> {
    let stage_pct = estimate_stage_progress(&research.stage);
    let filled = (stage_pct / 10) as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
    let pct_str = format!("{stage_pct}%");
    Line::from(vec![
        Span::styled("  ", Style::default().bg(theme.panel)),
        Span::styled(
            format!("◎ {} ", research.thread),
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
        Span::styled(bar, Style::default().fg(theme.accent).bg(theme.panel)),
        Span::styled(
            format!(" {} ", pct_str),
            Style::default().fg(theme.accent).bg(theme.panel),
        ),
        Span::styled(
            format!("· {}", research.stage),
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
    ])
}

fn render_orchestration_progress_bar(
    run_id: &str,
    status: &str,
    progress: &str,
    language: TuiLanguage,
    theme: TuiThemePalette,
) -> Line<'static> {
    let pct = extract_percent(progress).unwrap_or(0).min(100);
    let filled = (pct / 10) as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled));
    Line::from(vec![
        Span::styled("  ", Style::default().bg(theme.panel)),
        Span::styled(
            format!("◇ {} {} ", language.text("运行", "run"), run_id),
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
        Span::styled(bar, Style::default().fg(theme.accent).bg(theme.panel)),
        Span::styled(
            format!(" {pct}% "),
            Style::default().fg(theme.accent).bg(theme.panel),
        ),
        Span::styled(
            format!("· {status} · {progress}"),
            Style::default().fg(theme.muted).bg(theme.panel),
        ),
    ])
}

fn extract_percent(value: &str) -> Option<u32> {
    for token in value.split_whitespace() {
        if let Some(raw) = token.strip_suffix('%') {
            if let Ok(percent) = raw.parse::<u32>() {
                return Some(percent);
            }
        }
    }
    None
}

fn estimate_stage_progress(stage: &str) -> u32 {
    let lower = stage.to_ascii_lowercase();

    if lower.contains("survey") || lower.contains("调研") {
        20
    } else if lower.contains("idea")
        || lower.contains("构思")
        || lower.contains("refine")
        || lower.contains("细化")
    {
        45
    } else if lower.contains("experiment") || lower.contains("实验") {
        60
    } else if lower.contains("implement") || lower.contains("实现") {
        75
    } else if lower.contains("paper")
        || lower.contains("论文")
        || lower.contains("document")
        || lower.contains("文档")
    {
        90
    } else {
        10
    }
}

fn role_color(role: &str, theme: TuiThemePalette) -> Color {
    match role {
        "You" | "User" => theme.text,
        "Astra" => theme.accent,
        _ => theme.text,
    }
}

struct ResearchInfoSegments {
    state: String,
    thread_label: &'static str,
    thread: String,
    stage_label: &'static str,
    stage: String,
    active: bool,
}

fn research_info_segments(model: &TerminalFrameModel) -> ResearchInfoSegments {
    let research_line = model
        .research_line
        .split("||")
        .next()
        .unwrap_or(model.research_line.as_str());
    let mut status = "available".to_string();
    let mut thread = "none".to_string();
    let mut stage = "none".to_string();
    let mut class = "none".to_string();
    let mut mode = "none".to_string();
    for part in research_line.split('|').map(str::trim) {
        if let Some(value) = part.strip_prefix("status ") {
            status = value.to_string();
        } else if let Some(value) = part.strip_prefix("thread ") {
            if thread == "none" {
                thread = value.to_string();
            }
        } else if let Some(value) = part.strip_prefix("thread_id ") {
            if thread == "none" {
                thread = value.to_string();
            }
        } else if let Some(value) = part.strip_prefix("thread_title ") {
            if value != "none" && !value.is_empty() {
                thread = value.to_string();
            }
        } else if let Some(value) = part.strip_prefix("stage ") {
            stage = value.to_string();
        } else if let Some(value) = part.strip_prefix("class ") {
            class = value.to_string();
        } else if let Some(value) = part.strip_prefix("mode ") {
            mode = value.to_string();
        }
    }
    let has_thread = status != "no_active_thread" && thread != "none";
    let state = if has_thread {
        model
            .language
            .text("研究上下文", "research context")
            .to_string()
    } else {
        model
            .language
            .text("等待研究线程", "research idle")
            .to_string()
    };
    let thread = if has_thread {
        truncate_plain(&thread, 30)
    } else {
        model
            .language
            .text(
                "输入 /research 绑定问题",
                "use /research to bind a question",
            )
            .to_string()
    };
    let stage = format_stage_summary(model.language, &stage, &class, &mode);
    ResearchInfoSegments {
        state,
        thread_label: model.language.text("主题 ", "topic "),
        thread,
        stage_label: model.language.text("阶段 ", "stage "),
        stage,
        active: has_thread,
    }
}

fn orchestration_info_segments(model: &TerminalFrameModel) -> Option<(String, String, String)> {
    let mut run_id = "none".to_string();
    let mut status = "none".to_string();
    let mut progress = "0/0".to_string();
    let mut current = "no active step".to_string();
    for part in model.research_line.split("||").map(str::trim) {
        if let Some(value) = part.strip_prefix("运行 ") {
            let mut fields = value.split('|').map(str::trim);
            if let Some(id) = fields.next() {
                run_id = id.to_string();
            }
            if let Some(field) = fields.next().and_then(|f| f.strip_prefix("status ")) {
                status = field.to_string();
            }
            if let Some(field) = fields.next() {
                progress = field
                    .strip_prefix("progress ")
                    .unwrap_or(field)
                    .replace(" · ", " ")
                    .trim()
                    .to_string();
            }
            if let Some(field) = fields.next().and_then(|f| f.strip_prefix("current ")) {
                current = field.to_string();
            }
            break;
        }
    }
    if run_id == "none" {
        None
    } else {
        Some((run_id, status, format!("{progress} · {current}")))
    }
}

fn format_stage_summary(language: TuiLanguage, stage: &str, class: &str, mode: &str) -> String {
    let primary = first_present(&[class, stage]).unwrap_or("none");
    if primary == "none" {
        return language.text("未选择", "not selected").to_string();
    }
    let primary = localize_stage(language, primary);
    if mode == "none" || mode.is_empty() {
        primary
    } else {
        format!("{} / {}", primary, localize_mode(language, mode))
    }
}

fn first_present<'a>(values: &[&'a str]) -> Option<&'a str> {
    values
        .iter()
        .copied()
        .find(|value| !value.is_empty() && *value != "none")
}

fn localize_stage(language: TuiLanguage, value: &str) -> String {
    match value {
        "survey" => language.text("文献调研", "survey"),
        "idea_form" => language.text("问题构思", "idea forming"),
        "idea_refine" => language.text("方案细化", "idea refinement"),
        "document" => language.text("文档沉淀", "documentation"),
        "experiment_design" => language.text("实验设计", "experiment design"),
        "implement" => language.text("实现", "implementation"),
        "experiment_run" => language.text("实验运行", "experiment run"),
        "result_to_claim" => language.text("结果到结论", "result to claim"),
        "publish" => language.text("发布/写作", "publish"),
        "repair" => language.text("修复", "repair"),
        "literature" | "lit_review" => language.text("文献", "literature"),
        "idea" | "ideation" => language.text("构思", "ideation"),
        "method" | "design" => language.text("方法设计", "method design"),
        "implementation" | "coding" | "code" => language.text("实现", "implementation"),
        "experiment" | "evaluation" => language.text("实验评估", "evaluation"),
        "paper" | "writing" => language.text("论文写作", "writing"),
        "review" | "rebuttal" => language.text("评审回应", "review"),
        other => return other.replace('_', " "),
    }
    .to_string()
}

fn localize_mode(language: TuiLanguage, value: &str) -> String {
    match value {
        "exploring" => language.text("探索", "exploring"),
        "comparing" => language.text("比较", "comparing"),
        "reviewing" => language.text("复查", "reviewing"),
        "debugging" => language.text("调试", "debugging"),
        "interpreting_results" => language.text("解释结果", "interpreting"),
        "drafting" => language.text("起草", "drafting"),
        "awaiting_human_gate" => language.text("等确认", "awaiting gate"),
        "ready_to_record" => language.text("待记录", "ready to record"),
        "ready_to_execute" => language.text("待执行", "ready to execute"),
        other => return other.replace('_', " "),
    }
    .to_string()
}

fn truncate_plain(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut chars = value.chars();
    let mut output = chars.by_ref().take(width).collect::<String>();
    if chars.next().is_some() {
        output = output.chars().take(width.saturating_sub(1)).collect();
        output.push('~');
    }
    output
}

fn single_line_plain(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compact_permission_mode(mode: &str, language: TuiLanguage) -> String {
    match mode {
        "read-only" => language.text("只读", "read-only"),
        "workspace-write" => language.text("工作区可写", "workspace-write"),
        "danger-full-access" => language.text("完全访问", "full-access"),
        other => other,
    }
    .to_string()
}

fn theme_indicator(model: &TerminalFrameModel) -> String {
    match (model.theme_mode, model.language) {
        (TuiThemeMode::Day, TuiLanguage::Zh) => "日间".to_string(),
        (TuiThemeMode::Night, TuiLanguage::Zh) => "夜间".to_string(),
        (TuiThemeMode::Day, TuiLanguage::En) => "light".to_string(),
        (TuiThemeMode::Night, TuiLanguage::En) => "dark".to_string(),
    }
}

#[cfg(test)]
fn composer_lines(model: &TerminalFrameModel) -> Vec<String> {
    composer_lines_with_cursor(model, model.composer.len())
}

fn composer_lines_with_cursor(model: &TerminalFrameModel, cursor: usize) -> Vec<String> {
    let prompt = model.language.text("Ask Astra", "Ask Astra");
    let title = model.language.text("输入", "Composer");
    let mut lines = vec![format!("╭─ {title} ─────────────────────")];
    if model.composer.is_empty() {
        let placeholder = model.language.text(
            "描述要改的代码、错误或实验目标",
            "Describe the change, bug, or research task",
        );
        lines.push(format!(
            "│ {prompt}  \x1b[2m{placeholder}\x1b[0m\x1b[7m \x1b[0m"
        ));
    } else {
        for (index, visual_line) in composer_visual_lines_with_cursor(&model.composer, cursor)
            .into_iter()
            .enumerate()
        {
            let prefix = if index == 0 {
                format!("{prompt}  ")
            } else {
                " ".repeat(visible_width(prompt) + 2)
            };
            lines.push(format!("│ {prefix}{visual_line}"));
        }
    }
    lines.push(format!(
        "│ {}",
        model.language.text(
            "/ 命令  $ 技能  Enter 发送  Ctrl-J 换行  Esc 清空",
            "/ commands  $ skills  Enter send  Ctrl-J newline  Esc clear",
        )
    ));
    lines.push("╰────────────────────────────────".to_string());
    lines
}

fn composer_visual_lines_with_cursor(text: &str, cursor: usize) -> Vec<String> {
    let cursor = clamp_to_char_boundary(text, cursor);
    let mut lines = Vec::new();
    let mut start = 0usize;
    for segment in text.split('\n') {
        let end = start + segment.len();
        let selected = cursor >= start && cursor <= end;
        if selected {
            lines.push(render_cursor_in_line(segment, cursor - start));
        } else {
            lines.push(segment.to_string());
        }
        start = end.saturating_add(1);
    }
    if text.ends_with('\n')
        && cursor == text.len()
        && lines.last().is_some_and(|line| !line.contains("\x1b[7m"))
    {
        lines.push(render_cursor_in_line("", 0));
    }
    if lines.is_empty() {
        lines.push(render_cursor_in_line("", 0));
    }
    lines
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn render_cursor_in_line(line: &str, cursor: usize) -> String {
    let cursor = clamp_to_char_boundary(line, cursor);
    let before = &line[..cursor];
    let after = &line[cursor..];
    if after.is_empty() {
        return format!("{before}\x1b[7m \x1b[0m");
    }
    let mut chars = after.chars();
    let current = chars.next().unwrap_or(' ');
    let rest = chars.collect::<String>();
    format!("{before}\x1b[7m{current}\x1b[0m{rest}")
}

#[cfg(test)]
fn tui_mode_label(model: &TerminalFrameModel) -> String {
    match model.language {
        TuiLanguage::Zh if model.title.contains("full-screen") => "全屏 TUI".to_string(),
        TuiLanguage::Zh => "内联 REPL".to_string(),
        TuiLanguage::En => model.title.clone(),
    }
}

fn localized_footer(model: &TerminalFrameModel) -> String {
    let _legacy_hints = &model.key_hints;
    model
        .language
        .text(
            "Enter 发送 | Esc 中断/清空 | / 命令 | $ 技能 | /exit 关闭",
            "Enter send | Esc interrupt/clear | / commands | $ skills | /exit close",
        )
        .to_string()
}

fn command_palette_lines(model: &TerminalFrameModel) -> Vec<String> {
    let language = model.language;
    let entries = matching_command_entries(&model.command_entries, &model.composer);
    let mut lines = vec![
        format!(
            "╭─ {} ─────────────────────────",
            language.text("命令", "Commands")
        ),
        language
            .text(
                "│ 方向键选择，Enter 执行，继续输入过滤",
                "│ Arrow keys select, Enter runs, type to filter",
            )
            .to_string(),
    ];
    let visible_entries = visible_entry_window(&entries, model.overlay_selected, 16);
    for (index, entry) in entries
        .iter()
        .enumerate()
        .skip(visible_entries.start)
        .take(visible_entries.len)
    {
        let selected = index == model.overlay_selected.min(entries.len().saturating_sub(1));
        let line = format!(
            "{} {:<18} {:<10} {}",
            if selected { "▌" } else { " " },
            entry.typed,
            localized_command_label(entry, language),
            localized_command_summary(entry, language)
        );
        lines.push(if selected { selected_line(&line) } else { line });
    }
    if entries.is_empty() {
        lines.push(
            language
                .text("  没有匹配命令", "  No matching commands")
                .to_string(),
        );
    }
    lines.push("╰────────────────────────────────".to_string());
    lines
}

fn skill_palette_lines(model: &TerminalFrameModel) -> Vec<String> {
    let language = model.language;
    let entries = matching_skill_entries(&model.skill_entries, &model.composer);
    let mut lines = vec![
        format!(
            "╭─ {} ─────────────────────────",
            language.text("技能", "Skills")
        ),
        language
            .text(
                "│ 方向键选择，Enter 查看/调用，输入 $list 查看全部",
                "│ Arrow keys select, Enter inspects/invokes, type $list to browse",
            )
            .to_string(),
    ];
    let visible_entries = visible_entry_window(&entries, model.overlay_selected, 8);
    for (index, entry) in entries
        .iter()
        .enumerate()
        .skip(visible_entries.start)
        .take(visible_entries.len)
    {
        let selected = index == model.overlay_selected.min(entries.len().saturating_sub(1));
        let line = format!(
            "{} {:<24} {}",
            if selected { "▌" } else { " " },
            entry,
            skill_summary(entry, language)
        );
        lines.push(if selected { selected_line(&line) } else { line });
    }
    if entries.is_empty() {
        lines.push(
            language
                .text("  没有匹配技能", "  No matching skills")
                .to_string(),
        );
    }
    lines.push("╰────────────────────────────────".to_string());
    lines
}

fn selected_line(line: &str) -> String {
    format!("{SELECTED_LINE_PREFIX}{line}{ANSI_RESET}")
}

struct VisibleEntryWindow {
    start: usize,
    len: usize,
}

fn visible_entry_window<T>(
    entries: &[T],
    selected: usize,
    max_visible: usize,
) -> VisibleEntryWindow {
    if entries.is_empty() || max_visible == 0 {
        return VisibleEntryWindow { start: 0, len: 0 };
    }
    let selected = selected.min(entries.len().saturating_sub(1));
    let half = max_visible / 2;
    let mut start = selected.saturating_sub(half);
    if start + max_visible > entries.len() {
        start = entries.len().saturating_sub(max_visible);
    }
    let len = entries.len().saturating_sub(start).min(max_visible);
    VisibleEntryWindow { start, len }
}

fn localized_command_label(card: &TuiCommandCard, language: TuiLanguage) -> String {
    localized_label(&card.action_id, &card.label, language)
}

fn localized_label(action_id: &str, fallback: &str, language: TuiLanguage) -> String {
    let label = match action_id {
        "open_help" => language.text("帮助", "Help"),
        "open_palette" => language.text("命令面板", "Command Palette"),
        "open_slash_help" => language.text("斜杠命令", "Slash Commands"),
        "submit_prompt" => language.text("对话", "Chat"),
        "interrupt_turn" => language.text("中断", "Interrupt"),
        "exit_tui" => language.text("退出", "Exit"),
        "switch_session" => language.text("会话", "Sessions"),
        "inspect_status" => language.text("状态", "Status"),
        "continue_session" => language.text("继续", "Continue"),
        "select_model" => language.text("模型", "Model"),
        "select_reasoning" => language.text("思考强度", "Reasoning"),
        "select_language" => language.text("语言", "Language"),
        "select_theme" => language.text("主题", "Theme"),
        "inspect_permissions" => language.text("审批", "Permissions"),
        "approve_permission" => language.text("批准", "Approve Permission"),
        "deny_permission" => language.text("拒绝", "Deny Permission"),
        "terminal_attach" => language.text("终端", "Terminal"),
        "terminal_replay" => language.text("终端回放", "Terminal Replay"),
        "inspect_diff" => language.text("Diff", "Diff"),
        "stage_commit" => language.text("提交", "Commit"),
        "fold_output" => language.text("折叠输出", "Fold Output"),
        "expand_output" => language.text("展开输出", "Expand Output"),
        "show_output" => language.text("结构化输出", "Output"),
        "show_logs" => language.text("日志", "Logs"),
        "open_artifact" => language.text("产物", "Artifacts"),
        "inspect_memory" => language.text("记忆", "Memory"),
        "inspect_cost" => language.text("成本", "Cost"),
        "inspect_usage" => language.text("用量", "Usage"),
        "run_doctor" => language.text("诊断", "Doctor"),
        "inspect_providers" => language.text("Provider", "Providers"),
        "inspect_config" => language.text("配置", "Config"),
        "inspect_tools" => language.text("工具", "Tools"),
        "inspect_mcp" => language.text("MCP", "MCP"),
        "open_research" => language.text("研究", "Research"),
        "open_research_board" => language.text("Hermes 看板", "Hermes Board"),
        "open_routines" => language.text("例程", "Routines"),
        _ => fallback,
    };
    label.to_string()
}

fn localized_command_category(category: &str, language: TuiLanguage) -> String {
    match category {
        "conversation" => language.text("对话", "Conversation"),
        "navigation" => language.text("导航", "Navigation"),
        "session" => language.text("会话", "Session"),
        "permissions" => language.text("审批", "Permissions"),
        "source_control" => language.text("源码", "Source Control"),
        "work" => language.text("工作", "Work"),
        "output" => language.text("输出", "Output"),
        "context" => language.text("上下文", "Context"),
        "usage" => language.text("用量", "Usage"),
        "diagnostics" => language.text("诊断", "Diagnostics"),
        "configuration" => language.text("配置", "Configuration"),
        "tools" => language.text("工具", "Tools"),
        "research" => language.text("研究", "Research"),
        other => other,
    }
    .to_string()
}

fn localized_command_summary(card: &TuiCommandCard, language: TuiLanguage) -> String {
    let text = match card.action_id.as_str() {
        "open_help" => language.text(
            "查看可用命令和技能入口",
            "Show available commands and skill entry points",
        ),
        "open_palette" => language.text("浏览可执行产品命令", "Browse executable product commands"),
        "open_slash_help" => language.text("查看斜杠命令表面", "Inspect slash commands"),
        "submit_prompt" => language.text(
            "继续代码智能体对话",
            "Continue the coding-agent conversation",
        ),
        "interrupt_turn" => language.text(
            "请求中断当前运行回合",
            "Request interruption of the active turn",
        ),
        "exit_tui" => language.text("关闭交互式 TUI", "Close the interactive TUI"),
        "switch_session" => language.text("浏览并切换历史会话", "Browse and switch saved sessions"),
        "inspect_status" => language.text(
            "查看当前会话和工作区状态",
            "Show session and workspace status",
        ),
        "continue_session" => {
            language.text("继续最近一次会话", "Continue the latest saved session")
        }
        "select_model" => language.text("查看或切换模型", "Inspect or change model selection"),
        "select_reasoning" => {
            language.text("查看或切换推理强度", "Inspect or change reasoning effort")
        }
        "select_language" => language.text("切换中文或英文界面", "Switch Chinese or English UI"),
        "select_theme" => language.text("切换白天或夜间主题", "Switch light or dark theme"),
        "inspect_permissions" => language.text("查看待审批请求", "Inspect pending approvals"),
        "approve_permission" => language.text("批准指定权限请求", "Approve a permission request"),
        "deny_permission" => language.text("拒绝指定权限请求", "Deny a permission request"),
        "terminal_attach" => language.text("连接受控终端流", "Attach the governed terminal stream"),
        "terminal_replay" => language.text("回放当前终端滚屏", "Replay terminal scrollback"),
        "inspect_diff" => language.text("查看当前工作区改动", "Show current workspace changes"),
        "stage_commit" => language.text("准备提交当前改动", "Prepare the commit workflow"),
        "fold_output" => language.text(
            "折叠长输出、日志、测试和 diff",
            "Collapse long output, logs, tests, and diffs",
        ),
        "expand_output" => language.text("展开已折叠输出块", "Expand folded output blocks"),
        "show_output" => language.text(
            "查看最近结构化输出块",
            "Show recent structured output blocks",
        ),
        "show_logs" => language.text(
            "查看最近日志和流式状态",
            "Show recent logs and streaming status",
        ),
        "open_artifact" => language.text(
            "打开受控产物、diff 和报告",
            "Open artifacts, diffs, and reports",
        ),
        "inspect_memory" => {
            language.text("查看项目和研究记忆", "Inspect project and research memory")
        }
        "inspect_cost" => language.text("查看 token 和成本摘要", "Show token and cost summary"),
        "inspect_usage" => language.text("查看详细 API 用量", "Show detailed API usage"),
        "run_doctor" => language.text(
            "诊断配置和环境健康",
            "Diagnose setup and environment health",
        ),
        "inspect_providers" => language.text("查看模型 provider 配置", "Inspect model providers"),
        "inspect_config" => language.text("查看当前 CLI 配置", "Inspect active CLI configuration"),
        "inspect_tools" => language.text("查看可用受控工具", "Inspect governed tools"),
        "inspect_mcp" => language.text("查看 MCP 服务器", "Inspect MCP servers"),
        "open_research" => language.text(
            "显示研究主题、阶段和模式",
            "Show research topic, stage, and mode",
        ),
        "open_research_board" => language.text(
            "打开 Hermes 看板 inspector",
            "Open the Hermes board inspector",
        ),
        "open_routines" => language.text(
            "查看后台例程、入口和受监督触发历史",
            "Inspect background routines, ingress, and supervised trigger history",
        ),
        _ => card.summary.as_str(),
    };
    text.to_string()
}

fn skill_summary(entry: &str, language: TuiLanguage) -> &'static str {
    let name = entry.trim_start_matches('$');
    if name.contains("review") {
        language.text(
            "审查方案、代码或论文风险",
            "Review research, code, or paper risks",
        )
    } else if name.contains("research") || name.contains("lit") {
        language.text("研究检索与分析", "Research retrieval and analysis")
    } else if name.contains("paper") {
        language.text("论文写作、图表或编译", "Paper writing, figures, or compile")
    } else if name.contains("web") || name.contains("design") {
        language.text(
            "视觉界面与前端交付",
            "Visual interface and frontend delivery",
        )
    } else if name.contains("experiment") || name.contains("run") {
        language.text(
            "实验计划、运行或监控",
            "Experiment planning, running, or monitoring",
        )
    } else {
        language.text(
            "查看说明或暂存一次受控运行",
            "Inspect instructions or stage a governed run",
        )
    }
}

fn skill_description_for_state(
    state: &TuiInteractionState,
    entry: &str,
    language: TuiLanguage,
) -> String {
    let skill_id = entry.trim_start_matches('$');
    state
        .skill_descriptions
        .iter()
        .find(|(candidate, _)| candidate == skill_id)
        .map(|(_, description)| description.clone())
        .filter(|description| !description.trim().is_empty())
        .unwrap_or_else(|| skill_summary(entry, language).to_string())
}

fn skill_source_for_state(state: &TuiInteractionState, skill_id: &str) -> String {
    state
        .skill_sources
        .iter()
        .find(|(candidate, _)| candidate == skill_id)
        .map(|(_, source)| source.clone())
        .filter(|source| !source.trim().is_empty())
        .unwrap_or_else(|| "registry".to_string())
}

#[cfg(test)]
fn push_transcript_turn_lines(content_lines: &mut Vec<String>, turn: &TranscriptTurn) {
    content_lines.push(format!(
        "╭─ {} ─────────────────────",
        role_label(turn.role)
    ));
    let mut lines = turn.body.lines();
    if let Some(first) = lines.next() {
        content_lines.push(format!("│ {first}"));
        for line in lines {
            content_lines.push(format!("│ {line}"));
        }
    } else {
        content_lines.push("│".to_string());
    }
    content_lines.push("╰────────────────────────────────".to_string());
}

fn top_border(width: usize) -> String {
    format!("╭{}╮", "─".repeat(width.saturating_sub(2)))
}

fn separator(width: usize) -> String {
    format!("├{}┤", "─".repeat(width.saturating_sub(2)))
}

fn bottom_border(width: usize) -> String {
    format!("╰{}╯", "─".repeat(width.saturating_sub(2)))
}

fn role_label(role: &str) -> &str {
    match role {
        "You" | "User" => "You",
        "Astra" => "Astra",
        other => other,
    }
}

fn accent(value: &str) -> String {
    format!("\x1b[1;36m{value}\x1b[0m")
}

fn row(width: usize, content: &str) -> String {
    let inner = width.saturating_sub(4).max(1);
    format!("| {} |", pad(&fit(content, inner), inner))
}

fn fit(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if let Some(selected) = value
        .strip_prefix(SELECTED_LINE_PREFIX)
        .and_then(|inner| inner.strip_suffix(ANSI_RESET))
    {
        return format!(
            "{SELECTED_LINE_PREFIX}{}{ANSI_RESET}",
            fit_display_width(selected, width)
        );
    }
    if value.contains("\x1b[7m") && value.contains(ANSI_RESET) {
        return fit_cursor_ansi_line(value, width);
    }
    if value.contains("\x1b[") {
        let visible = visible_width(value);
        if visible <= width {
            return value.to_string();
        }
        return fit(&strip_ansi(value), width);
    }
    fit_display_width(value, width)
}

fn pad(value: &str, width: usize) -> String {
    let len = visible_width(value);
    if len >= width {
        value.to_string()
    } else {
        format!("{}{}", value, " ".repeat(width - len))
    }
}

fn visible_width(value: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi(value).as_str())
}

fn fit_display_width(value: &str, width: usize) -> String {
    let mut output = String::new();
    let mut used = 0usize;
    let mut truncated = false;
    for ch in value.chars() {
        let ch_width = UnicodeWidthStr::width(ch.to_string().as_str()).max(1);
        if used + ch_width > width {
            truncated = true;
            break;
        }
        output.push(ch);
        used += ch_width;
    }
    if truncated {
        while !output.is_empty() && used + 1 > width {
            if let Some(ch) = output.pop() {
                used = used.saturating_sub(UnicodeWidthStr::width(ch.to_string().as_str()).max(1));
            }
        }
        if width > 0 {
            output.push('~');
        }
    }
    output
}

fn fit_cursor_ansi_line(value: &str, width: usize) -> String {
    let Some(cursor_start) = value.find("\x1b[7m") else {
        return fit_display_width(&strip_ansi(value), width);
    };
    let cursor_body_start = cursor_start + "\x1b[7m".len();
    let Some(reset_offset) = value[cursor_body_start..].find(ANSI_RESET) else {
        return fit_display_width(&strip_ansi(value), width);
    };
    let cursor_end = cursor_body_start + reset_offset;
    let reset_end = cursor_end + ANSI_RESET.len();
    let before = strip_ansi(&value[..cursor_start]);
    let cursor = strip_ansi(&value[cursor_body_start..cursor_end]);
    let cursor = if cursor.is_empty() {
        " ".to_string()
    } else {
        cursor
    };
    let after = strip_ansi(&value[reset_end..]);
    let cursor_width = visible_width(&cursor).max(1);
    if cursor_width >= width {
        return format!("\x1b[7m{}\x1b[0m", fit_display_width(&cursor, width));
    }
    let left_budget = (width - cursor_width) / 2;
    let left = fit_display_suffix(&before, left_budget);
    let remaining = width
        .saturating_sub(visible_width(&left))
        .saturating_sub(cursor_width);
    let right = fit_display_prefix(&after, remaining);
    format!("{left}\x1b[7m{cursor}\x1b[0m{right}")
}

fn fit_display_prefix(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let content_width = width.saturating_sub(1);
    let mut output = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = UnicodeWidthStr::width(ch.to_string().as_str()).max(1);
        if used + ch_width > content_width {
            break;
        }
        output.push(ch);
        used += ch_width;
    }
    output.push('~');
    output
}

fn fit_display_suffix(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let content_width = width.saturating_sub(1);
    let mut kept = Vec::new();
    let mut used = 0usize;
    for ch in value.chars().rev() {
        let ch_width = UnicodeWidthStr::width(ch.to_string().as_str()).max(1);
        if used + ch_width > content_width {
            break;
        }
        kept.push(ch);
        used += ch_width;
    }
    let suffix = kept.into_iter().rev().collect::<String>();
    format!("~{suffix}")
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
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

fn decode_exit_key(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [3] => Some("ctrl-c"),
        [4] => Some("ctrl-d"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratatui_frame_text(model: &TerminalFrameModel, cols: u16, rows: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(cols, rows);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        terminal
            .draw(|frame| render_ratatui_frame(frame, model))
            .expect("frame should draw");
        let buffer = terminal.backend().buffer();
        let width = usize::from(buffer.area.width);
        buffer
            .content
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn test_interaction_state() -> TuiInteractionState {
        TuiInteractionState {
            composer: String::new(),
            transcript: Vec::new(),
            skill_entries: vec![
                "$web-design-engineer".to_string(),
                "$research-review".to_string(),
            ],
            skill_descriptions: vec![
                (
                    "web-design-engineer".to_string(),
                    "Build visual interfaces and frontend deliverables".to_string(),
                ),
                (
                    "research-review".to_string(),
                    "Review research, code, or paper risks".to_string(),
                ),
            ],
            skill_sources: vec![
                ("web-design-engineer".to_string(), "fixture".to_string()),
                ("research-review".to_string(), "fixture".to_string()),
            ],
            command_entries: product_command_specs()
                .into_iter()
                .map(|spec| TuiCommandCard {
                    typed: spec.typed,
                    action_id: spec.action_id,
                    label: spec.label,
                    gate: "test_gate".to_string(),
                    category: spec.category,
                    summary: spec.summary,
                })
                .collect(),
            active_session_id: Some("sess_active".to_string()),
            recent_sessions: Vec::new(),
            prompt_history: Vec::new(),
            input_history: Vec::new(),
            research_line: "status active | thread thread_1 | stage implementation | next test"
                .to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            remote_state: "ready".to_string(),
            running_turn: None,
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        }
    }

    fn product_command_cards_for_test() -> Vec<TuiCommandCard> {
        product_command_specs()
            .into_iter()
            .map(|spec| TuiCommandCard {
                typed: spec.typed,
                action_id: spec.action_id,
                label: spec.label,
                gate: "test_gate".to_string(),
                category: spec.category,
                summary: spec.summary,
            })
            .collect()
    }

    #[test]
    fn tui_product_command_registry_has_no_duplicate_or_empty_surface_copy() {
        let specs = product_command_specs();
        let mut typed = BTreeSet::new();
        let mut typed_by_action = BTreeMap::<String, Vec<String>>::new();

        for spec in &specs {
            assert!(
                typed.insert(spec.typed.clone()),
                "duplicate typed command {}",
                spec.typed
            );
            assert_nonempty_text(&spec.action_id, &format!("{} action_id", spec.typed));
            assert_nonempty_text(&spec.label, &format!("{} label", spec.typed));
            assert_nonempty_text(&spec.category, &format!("{} category", spec.typed));
            assert_nonempty_text(&spec.summary, &format!("{} summary", spec.typed));
            typed_by_action
                .entry(spec.action_id.clone())
                .or_default()
                .push(spec.typed.clone());
        }

        for (action_id, typed_commands) in typed_by_action {
            if typed_commands.len() <= 1 {
                continue;
            }
            assert!(
                matches!(action_id.as_str(), "submit_prompt"),
                "unexpected duplicate action_id {action_id}: {typed_commands:?}"
            );
        }
    }

    #[test]
    fn every_product_command_round_trips_through_real_tui_route() {
        let mut state = test_interaction_state();
        let mut config_actions = Vec::new();
        let mut permission_actions = Vec::new();

        for spec in product_command_specs() {
            let input = command_input_for_test(&spec.typed);
            let parsed = parse_tui_surface_command(&input);
            if spec.typed == "/prompt <text>" {
                assert_eq!(parsed.kind, SurfaceCommandKind::PromptTurn, "{input}");
                assert_eq!(parsed.prompt.as_deref(), Some("test prompt"));
                continue;
            }
            assert_ne!(parsed.kind, SurfaceCommandKind::Unknown, "{input}");
            assert_eq!(
                parsed.action_id.as_deref(),
                Some(spec.action_id.as_str()),
                "{input}"
            );

            let response = route_surface_command_mut_with_executors(
                &mut state,
                parsed,
                &mut |action| {
                    config_actions.push(action.clone());
                    match action.kind {
                        TuiConfigActionKind::Model => Ok(TuiConfigActionResult {
                            applied: true,
                            provider_id: Some("openai".to_string()),
                            model: Some(action.value),
                            reasoning_effort: None,
                            session_id: None,
                            scope: "project".to_string(),
                            message: "model fixture applied".to_string(),
                        }),
                        TuiConfigActionKind::Reasoning => Ok(TuiConfigActionResult {
                            applied: true,
                            provider_id: None,
                            model: None,
                            reasoning_effort: Some(action.value),
                            session_id: None,
                            scope: "project".to_string(),
                            message: "reasoning fixture applied".to_string(),
                        }),
                        TuiConfigActionKind::Session(TuiSessionActionKind::Resume) => {
                            Ok(TuiConfigActionResult::session_resumed(
                                "sess_fixture".to_string(),
                                "project".to_string(),
                                "session fixture resumed".to_string(),
                            ))
                        }
                    }
                },
                &mut |action| {
                    permission_actions.push(action.clone());
                    Ok(TuiPermissionActionResult {
                        request_id: action.request_id,
                        decision: action.decision.as_command().to_string(),
                        pending_count: Some(0),
                        message: "permission fixture resolved".to_string(),
                    })
                },
            );
            assert_nonempty_text(&response, &format!("{} TUI response", spec.typed));
            assert!(
                !response.contains("还没有 TUI 渲染器")
                    && !response.contains("has no TUI renderer yet"),
                "{} must have a real TUI interaction response: {}",
                spec.typed,
                response
            );

            for extra_input in match spec.typed.as_str() {
                "/model" => vec!["/model gpt-5.5"],
                "/reasoning" => vec!["/reasoning high"],
                _ => Vec::new(),
            } {
                let parsed = parse_tui_surface_command(extra_input);
                let response = route_surface_command_mut_with_executors(
                    &mut state,
                    parsed,
                    &mut |action| {
                        config_actions.push(action.clone());
                        match action.kind {
                            TuiConfigActionKind::Model => Ok(TuiConfigActionResult {
                                applied: true,
                                provider_id: Some("openai".to_string()),
                                model: Some(action.value),
                                reasoning_effort: None,
                                session_id: None,
                                scope: "project".to_string(),
                                message: "model fixture applied".to_string(),
                            }),
                            TuiConfigActionKind::Reasoning => Ok(TuiConfigActionResult {
                                applied: true,
                                provider_id: None,
                                model: None,
                                reasoning_effort: Some(action.value),
                                session_id: None,
                                scope: "project".to_string(),
                                message: "reasoning fixture applied".to_string(),
                            }),
                            TuiConfigActionKind::Session(TuiSessionActionKind::Resume) => {
                                Ok(TuiConfigActionResult::session_resumed(
                                    "sess_fixture".to_string(),
                                    "project".to_string(),
                                    "session fixture resumed".to_string(),
                                ))
                            }
                        }
                    },
                    &mut |action| {
                        permission_actions.push(action.clone());
                        Ok(TuiPermissionActionResult {
                            request_id: action.request_id,
                            decision: action.decision.as_command().to_string(),
                            pending_count: Some(0),
                            message: "permission fixture resolved".to_string(),
                        })
                    },
                );
                assert_nonempty_text(&response, &format!("{extra_input} TUI response"));
            }
        }

        assert!(
            config_actions
                .iter()
                .any(|action| matches!(action.kind, TuiConfigActionKind::Model)),
            "/model <model> should exercise the real config interaction path"
        );
        assert!(
            config_actions
                .iter()
                .any(|action| matches!(action.kind, TuiConfigActionKind::Reasoning)),
            "/reasoning <value> should exercise the real config interaction path"
        );
        assert!(
            permission_actions
                .iter()
                .any(|action| action.decision == TuiPermissionDecision::Approve),
            "/approve should exercise the real permission path"
        );
        assert!(
            permission_actions
                .iter()
                .any(|action| action.decision == TuiPermissionDecision::Deny),
            "/deny should exercise the real permission path"
        );
    }

    #[test]
    fn every_product_command_round_trips_through_real_tui_key_input() {
        for spec in product_command_specs() {
            let input = command_input_for_test(&spec.typed);
            let mut state = test_interaction_state();
            let mut prompts = Vec::new();
            let mut executor = |prompt: &str| {
                prompts.push(prompt.to_string());
                Ok(TuiCommandExecution {
                    body: format!("prompt fixture completed: {prompt}"),
                    ..Default::default()
                })
            };

            for byte in input.as_bytes() {
                assert_eq!(
                    handle_tui_input_with_executor(&mut state, &[*byte], &mut executor),
                    TuiInputOutcome::Continue,
                    "typing {input}"
                );
            }
            let submit_outcome = handle_tui_input_with_executor(&mut state, b"\n", &mut executor);
            if spec.action_id == "exit_tui" {
                assert_eq!(
                    submit_outcome,
                    TuiInputOutcome::Exit("slash-exit"),
                    "submitting {input}"
                );
                continue;
            }
            assert_eq!(
                submit_outcome,
                TuiInputOutcome::Continue,
                "submitting {input}"
            );

            assert_eq!(state.composer, "", "{input} should clear composer");
            assert!(
                state.transcript.len() >= 2,
                "{input} should append user and response turns"
            );
            assert_eq!(state.transcript[0].role, "You", "{input}");
            assert_eq!(state.transcript[0].body, input, "{input}");
            let response = state
                .transcript
                .last()
                .expect("response turn should exist")
                .body
                .as_str();
            assert_nonempty_text(response, &format!("{input} interactive response"));
            assert!(
                !response.contains("未知命令")
                    && !response.contains("Unknown command")
                    && !response.contains("还没有 TUI 渲染器")
                    && !response.contains("has no TUI renderer yet"),
                "{input} should be handled by a real interactive TUI route: {response}"
            );

            if spec.typed == "/prompt <text>" {
                assert_eq!(prompts, vec!["test prompt".to_string()]);
                assert!(response.contains("prompt fixture completed: test prompt"));
            }
        }
    }

    #[test]
    fn every_skill_round_trips_through_real_tui_route_with_canonical_description() {
        let mut state = test_interaction_state();
        let mut typed = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let skill_entries = state.skill_entries.clone();
        for entry in &skill_entries {
            let skill_id = entry.trim_start_matches('$');
            assert!(
                typed.insert(entry.clone()),
                "duplicate typed skill command {entry}"
            );
            assert!(
                ids.insert(skill_id.to_string()),
                "duplicate skill id {skill_id}"
            );
            let description = skill_description_for_state(&state, entry, TuiLanguage::En);
            assert_nonempty_text(&description, &format!("{skill_id} description"));
            assert_ne!(
                description, "Inspect instructions or stage a governed run",
                "{skill_id} should expose canonical skill description, not generic fallback"
            );

            let inspect = route_typed_command_mut(&mut state, entry);
            assert_nonempty_text(&inspect, &format!("{entry} inspect response"));
            assert!(
                inspect.contains(&description) || inspect.contains("描述："),
                "{entry} inspect should surface skill description: {inspect}"
            );

            let run = route_typed_command_mut(&mut state, &format!("{entry} fixture input"));
            assert_nonempty_text(&run, &format!("{entry} run response"));
            assert!(
                run.contains("skills run --skill") || run.contains("Skill run staged"),
                "{entry} should exercise the skill run route: {run}"
            );
        }

        let list = route_typed_command_mut(&mut state, "$list");
        assert_nonempty_text(&list, "$list response");
        for entry in &skill_entries {
            assert!(list.contains(entry), "$list should include {entry}");
        }
    }

    #[test]
    fn every_skill_round_trips_through_real_tui_key_input() {
        let skill_entries = test_interaction_state().skill_entries;
        for entry in skill_entries {
            let skill_id = entry.trim_start_matches('$').to_string();
            let description = {
                let state = test_interaction_state();
                skill_description_for_state(&state, &entry, TuiLanguage::En)
            };

            for input in [entry.clone(), format!("{entry} fixture input")] {
                let mut state = test_interaction_state();
                for byte in input.as_bytes() {
                    assert_eq!(
                        handle_tui_input(&mut state, &[*byte]),
                        TuiInputOutcome::Continue,
                        "typing {input}"
                    );
                }
                assert_eq!(
                    handle_tui_input(&mut state, b"\n"),
                    TuiInputOutcome::Continue,
                    "submitting {input}"
                );

                assert_eq!(state.composer, "", "{input} should clear composer");
                assert_eq!(state.transcript[0].body, input, "{input}");
                let response = state
                    .transcript
                    .last()
                    .expect("skill response should exist")
                    .body
                    .as_str();
                assert_nonempty_text(response, &format!("{input} interactive response"));
                assert!(
                    response.contains(&description)
                        || response.contains("skills run --skill")
                        || response.contains("Skill run staged"),
                    "{input} should surface skill description or run route: {response}"
                );
                assert!(
                    !response.contains("未知技能") && !response.contains("Unknown skill"),
                    "{skill_id} should be known in real TUI input: {response}"
                );
            }
        }
    }

    #[test]
    fn discovered_tui_model_has_unique_commands_skills_and_canonical_skill_copy() {
        let state_home = env::temp_dir().join(format!(
            "research_cli_tui_model_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos()
        ));
        let workspace = state_home.join("workspace");
        fs::create_dir_all(workspace.join(".git")).expect("workspace should be created");
        fs::create_dir_all(workspace.join(".codex/skills/alpha-skill"))
            .expect("skill dir should be created");
        fs::write(
            workspace.join(".codex/skills/alpha-skill/SKILL.md"),
            "---\ndescription: Alpha canonical description.\n---\n\n# Alpha\n",
        )
        .expect("skill manifest should write");
        fs::create_dir_all(workspace.join(".codex/skills/beta-skill"))
            .expect("second skill dir should be created");
        fs::write(
            workspace.join(".codex/skills/beta-skill/SKILL.md"),
            "# Beta\n\nBeta first paragraph description.\n",
        )
        .expect("second skill manifest should write");

        let resolved = crate::projects::current::ResolvedProject {
            project_id: "proj_tui_model".to_string(),
            workspace_root: workspace.clone(),
            workspace_hash: "hash_tui_model".to_string(),
            data_dir: workspace.join(".pmcli"),
            resolution_source: "test".to_string(),
        };
        let launch = crate::host_surface::tui_launch(&state_home, &resolved, None)
            .expect("tui launch should build model from discovered skills");
        let mut command_typed = BTreeSet::new();
        let mut command_action_pairs = BTreeSet::new();
        for group in &launch.inline_view.command_model.groups {
            assert_nonempty_text(&group.group_id, "command group id");
            assert_nonempty_text(&group.label, "command group label");
            for command in &group.commands {
                assert!(
                    command_typed.insert(command.typed.clone()),
                    "duplicate TUI command {}",
                    command.typed
                );
                assert!(
                    command_action_pairs.insert((command.typed.clone(), command.action_id.clone())),
                    "duplicate command/action pair {} -> {}",
                    command.typed,
                    command.action_id
                );
                assert_nonempty_text(&command.label, &format!("{} label", command.typed));
                assert_nonempty_text(&command.summary, &format!("{} summary", command.typed));
                assert_nonempty_text(&command.gate, &format!("{} gate", command.typed));
                let parsed = parse_tui_surface_command(&command_input_for_test(&command.typed));
                if command.typed != "/prompt <text>" {
                    assert_eq!(
                        parsed.action_id.as_deref(),
                        Some(command.action_id.as_str()),
                        "{} should parse back to its action id",
                        command.typed
                    );
                }
            }
        }

        let mut skill_typed = BTreeSet::new();
        let mut skill_ids = BTreeSet::new();
        let mut descriptions = BTreeMap::new();
        for skill in &launch.inline_view.skill_model.entries {
            assert!(
                skill_typed.insert(skill.typed.clone()),
                "duplicate skill command {}",
                skill.typed
            );
            assert!(
                skill_ids.insert(skill.skill_id.clone()),
                "duplicate skill id {}",
                skill.skill_id
            );
            assert_eq!(skill.typed, format!("${}", skill.skill_id));
            assert_nonempty_text(
                &skill.description,
                &format!("{} description", skill.skill_id),
            );
            assert_nonempty_text(&skill.source, &format!("{} source", skill.skill_id));
            descriptions.insert(skill.skill_id.clone(), skill.description.clone());
        }
        assert_eq!(
            descriptions.get("alpha-skill").map(String::as_str),
            Some("Alpha canonical description.")
        );
        assert_eq!(
            descriptions.get("beta-skill").map(String::as_str),
            Some("Beta first paragraph description.")
        );

        for command in launch
            .inline_view
            .command_model
            .groups
            .iter()
            .flat_map(|group| &group.commands)
        {
            let input = command_input_for_test(&command.typed);
            let mut interaction = TuiInteractionState::from_result(&launch);
            let mut executor = |prompt: &str| {
                Ok(TuiCommandExecution {
                    body: format!("prompt fixture completed: {prompt}"),
                    ..Default::default()
                })
            };
            for byte in input.as_bytes() {
                assert_eq!(
                    handle_tui_input_with_executor(&mut interaction, &[*byte], &mut executor),
                    TuiInputOutcome::Continue,
                    "typing discovered command {input}"
                );
            }
            let outcome = handle_tui_input_with_executor(&mut interaction, b"\n", &mut executor);
            if command.action_id == "exit_tui" {
                assert_eq!(
                    outcome,
                    TuiInputOutcome::Exit("slash-exit"),
                    "submitting discovered command {input}"
                );
                continue;
            }
            assert_eq!(
                outcome,
                TuiInputOutcome::Continue,
                "submitting discovered command {input}"
            );
            let response = interaction
                .transcript
                .last()
                .expect("discovered command should append response")
                .body
                .as_str();
            assert_nonempty_text(response, &format!("{input} discovered command response"));
            assert!(
                !response.contains("未知命令")
                    && !response.contains("Unknown command")
                    && !response.contains("has no TUI renderer yet"),
                "{input} should be handled by a real route: {response}"
            );
        }

        for skill in &launch.inline_view.skill_model.entries {
            for input in [
                skill.typed.clone(),
                format!("{} discovered fixture input", skill.typed),
            ] {
                let mut interaction = TuiInteractionState::from_result(&launch);
                for byte in input.as_bytes() {
                    assert_eq!(
                        handle_tui_input(&mut interaction, &[*byte]),
                        TuiInputOutcome::Continue,
                        "typing discovered skill {input}"
                    );
                }
                assert_eq!(
                    handle_tui_input(&mut interaction, b"\n"),
                    TuiInputOutcome::Continue,
                    "submitting discovered skill {input}"
                );
                let response = interaction
                    .transcript
                    .last()
                    .expect("discovered skill should append response")
                    .body
                    .as_str();
                assert_nonempty_text(response, &format!("{input} discovered skill response"));
                assert!(
                    response.contains(&skill.description)
                        || response.contains("skills run --skill")
                        || response.contains("Skill run staged"),
                    "{input} should surface canonical description or skill run route: {response}"
                );
                assert!(
                    !response.contains("未知技能") && !response.contains("Unknown skill"),
                    "{} should be known in discovered TUI input: {response}",
                    skill.skill_id
                );
            }
        }
    }

    fn command_input_for_test(typed: &str) -> String {
        typed
            .replace("<text>", "test prompt")
            .replace("<zh|en>", "en")
            .replace("<light|dark>", "dark")
            .replace("<request-id>", "req_001")
            .replace("<step-id>", "step_1")
            .replace("<model>", "gpt-5.5")
            .replace("<auto|low|medium|high>", "high")
    }

    fn assert_nonempty_text(value: &str, label: &str) {
        assert!(!value.trim().is_empty(), "{label} must not be empty");
        assert!(
            !value.contains("TODO") && !value.contains("todo"),
            "{label} must not contain placeholder copy: {value}"
        );
    }

    fn terminal_model_from_state_for_test(state: &TuiInteractionState) -> TerminalFrameModel {
        TerminalFrameModel {
            title: "Astra Code rich inline TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: state.remote_state.clone(),
            session_count: state.session_count.clone(),
            permission_count: state.permission_count.clone(),
            permission_mode: state.permission_mode.clone(),
            model_label: state.model_label.clone(),
            reasoning_effort: state.reasoning_effort.clone(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: state.composer.clone(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: state.skill_entries.clone(),
            command_entries: state.command_entries.clone(),
            language: state.language,
            theme_mode: state.theme_mode,
            overlay_selected: state.overlay_selected,
            output_folded: state.output_folded,
        }
    }

    fn routable_sample_for_command(typed: &str) -> String {
        typed
            .replace("<text>", "inspect this file")
            .replace("<zh|en>", "en")
            .replace("<light|dark>", "dark")
            .replace("<request-id>", "perm_1")
    }

    #[test]
    fn terminal_frame_defaults_to_chinese_without_bilingual_copy_dump() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "pairing_required".to_string(),
            session_count: "0".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status no_active_thread | thread none | stage none | next record_or_resume_research_thread".to_string(),
            transcript: vec![TranscriptTurn {
                role: "Astra",
                body: "就绪。直接输入需求，或输入 / 打开命令，$ 打开技能。".to_string(),
            }],
            composer: String::new(),
            key_hints: vec!["ctrl-d close".to_string(), "ctrl-c interrupt".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(&model, TerminalSize { cols: 96, rows: 28 });

        assert!(frame.contains("Astra Code"));
        assert!(frame.contains("代码智能体"));
        assert!(frame.contains("研究"));
        assert!(frame.contains("输入"));
        assert!(frame.contains("/ 命令"));
        assert!(frame.contains("$ 技能"));
        assert!(frame.contains("╭─ Astra"));
        assert!(frame.contains("│ 就绪"));
        assert!(!frame.contains(" / Research"));
        assert!(!frame.contains(" / Message"));
        assert!(!frame.contains(" / Commands"));
        assert!(!frame.contains(" / Skills"));
        assert!(!frame.contains("Tool stream"));
        assert!(!frame.contains("Composer"));
        assert!(!frame.contains("Shortcuts are optional"));
        assert!(!frame.contains("pane conversation"));
        assert!(!frame.contains("renderer:"));
        assert!(!frame.contains("empty="));
    }

    #[test]
    fn launch_mode_defaults_to_claw_style_inline_repl_except_fullscreen() {
        assert!(launch_mode_uses_inline_repl_backend("chat_first_repl"));
        assert!(!launch_mode_uses_inline_repl_backend(
            "fullscreen_split_pane_projection_renderer"
        ));
    }

    #[test]
    fn interactive_inline_launch_selects_claw_rich_inline_not_rich_composer() {
        assert_eq!(
            select_inline_repl_backend("chat_first_repl", true, true),
            InlineReplBackend::ClawRichInline
        );
        assert_eq!(
            select_inline_repl_backend("chat_first_repl", false, true),
            InlineReplBackend::Stdin
        );
        assert_eq!(
            select_inline_repl_backend("chat_first_repl", true, false),
            InlineReplBackend::Snapshot
        );
        assert_eq!(
            select_inline_repl_backend("fullscreen_split_pane_projection_renderer", true, true),
            InlineReplBackend::Fullscreen
        );
    }

    #[test]
    fn inline_repl_banner_matches_claw_code_home_layout_without_composer_box() {
        let context = InlineReplBannerContext {
            model: "gpt-5.5".to_string(),
            permission: "read-only".to_string(),
            branch: "main".to_string(),
            workspace: "clean".to_string(),
            directory: "/workspace/research_cli".to_string(),
            research: "thread thread_1 · stage implementation".to_string(),
        };

        let banner = render_inline_repl_banner_from_context(&context);

        assert!(banner.contains("◉"));
        assert!(banner.contains("Astra Code"));
        assert!(banner.contains("gpt-5.5"));
        assert!(banner.contains("read-only"));
        assert!(banner.contains("main"));
        assert!(banner.contains("clean"));
        assert!(banner.contains("/help"));
        assert!(banner.contains("/research board"));
        assert!(banner.contains("$list"));
        assert!(banner.contains("╭"));
        assert!(banner.contains("╰"));
        assert!(!banner.contains("█████"));
        assert!(!banner.contains("\x1b[2mModel\x1b[0m"));
        assert!(!banner.contains("Memory"));
        assert!(!banner.contains("Connected:"));
    }

    #[test]
    fn inline_running_status_uses_single_line_spinner_not_boxed_card() {
        let state = test_interaction_state();

        let line = render_inline_turn_status_line(&state, "修复 TUI\n输入框", 0, Instant::now());

        assert!(line.contains("⠋"));
        assert!(line.contains("Astra"));
        assert!(line.contains("思考中"));
        assert!(line.contains("/exc"));
        assert!(line.contains("░"));
        assert!(line.contains("█"));
        assert!(line.contains("0s"));
        assert!(!line.contains('\n'));
        assert!(line.contains("修复 TUI 输入框"));
        assert!(!line.contains("╭"));
        assert!(!line.contains("╰"));
    }

    #[test]
    fn inline_running_status_fits_narrow_terminal_without_wrapping() {
        let state = test_interaction_state();
        let long_prompt = "中文输入会让状态行很长".repeat(20);

        let line = render_inline_turn_status_line_with_width(&state, &long_prompt, 0, 0, 79);

        assert!(!line.contains('\n'));
        assert!(
            visible_width(&line) <= 79,
            "status line must stay inside an 80-column terminal: {}",
            visible_width(&line)
        );
    }

    #[test]
    fn claw_rich_line_editor_context_keeps_input_box_without_raw_mode_prompt() {
        let mut state = test_interaction_state();
        state.permission_mode = "ask-on-request".to_string();
        state.model_label = "gpt-5.5".to_string();

        let frame = render_claw_line_editor_context(&state);

        assert!(frame.contains("╭─"));
        assert!(frame.contains("Astra"));
        assert!(frame.contains("ask-on-request"));
        assert!(frame.contains("gpt-5.5"));
        assert!(frame.contains("Tab 补全"));
        assert!(frame.contains("/research board"));
        assert!(frame.contains("Ctrl-C 中断"));
        assert!(frame.contains("research:"));
        assert!(!frame.contains("╭─ 输入"));
        assert!(!frame.contains("Ask Astra"));
        assert!(!frame.contains("session "));
    }

    #[test]
    fn claw_rich_line_editor_prompt_delegates_text_cursor_to_rustyline() {
        let prompt = claw_line_editor_prompt(TuiLanguage::Zh);

        assert!(prompt.contains("╰─"));
        assert!(prompt.contains(">"));
        assert!(!prompt.contains("你好"));
        assert!(!prompt.contains("\x1b[7m"));
    }

    #[test]
    fn claw_rich_inline_prompt_renders_skill_candidates_from_dollar_prefix() {
        let mut state = test_interaction_state();
        state.composer = "$".to_string();
        let model = terminal_model_from_state_for_test(&state);

        let frame = render_claw_rich_inline_prompt_with_width_and_cursor(&model, 96, 1);

        assert!(frame.contains("> $"));
        assert!(frame.contains("╭─ 技能"));
        assert!(frame.contains("$research-review"));
        assert!(frame.contains(SELECTED_LINE_PREFIX));
        assert!(!frame.contains("╭─ 输入"));
    }

    #[test]
    fn inline_repl_response_renders_structured_blocks_without_runtime_status_noise() {
        let renderer = TerminalMarkdownRenderer::new();
        let body = "cargo test\nwarning: unused import\nerror: compile failed\n+ added\n- removed\nsession s | turn t | provider fixture";

        let body = strip_inline_runtime_metadata(body);
        let rendered = render_inline_repl_response_text(&renderer, &body, TuiLanguage::Zh, false);

        assert!(rendered.contains("╭─"));
        assert!(rendered.contains("命令"));
        assert!(rendered.contains("Diff"));
        assert!(!rendered.contains("状态"));
        assert!(rendered.contains("warning: unused import"));
    }

    #[test]
    fn typing_slash_renders_chinese_command_palette_overlay_by_default() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, &[b'/']),
            TuiInputOutcome::Continue
        );
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );

        assert!(frame.contains("命令"));
        assert!(!frame.contains("命令 / Commands"));
        assert!(frame.contains("╭─ 命令"));
        assert!(frame.contains(SELECTED_LINE_PREFIX));
        assert!(frame.contains("/prompt"));
        assert!(frame.contains("终端"));
        assert!(frame.contains("研究"));
    }

    #[test]
    fn rich_inline_prompt_frame_renders_slash_palette_with_selected_row() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"/"),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, &[27, b'[', b'B']),
            TuiInputOutcome::Continue
        );
        let model = terminal_model_from_state_for_test(&state);

        let frame = render_rich_inline_prompt_frame_with_width(&model, 104);

        assert!(frame.contains("Astra"));
        assert!(frame.contains("输入需求，/ 打开命令，$ 调用技能"));
        assert!(frame.contains("模型 auto"));
        assert!(frame.contains("权限 只读"));
        assert!(frame.contains("主题 日间"));
        assert!(frame.contains("╭─ 命令"));
        assert!(frame.contains(&format!("{SELECTED_LINE_PREFIX}▌ /palette")));
        assert!(frame.contains("方向键选择"));
        assert!(frame.contains("继续输入过滤"));
        assert!(frame.contains("╭─ 输入"));
        assert!(!frame.contains("action_id"));
        assert!(!frame.contains("test_gate"));
    }

    #[test]
    fn arrow_keys_move_command_palette_selection_and_render_selected_row() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"/"),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, &[27, b'[', b'B']),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.overlay_selected, 1);

        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );
        assert!(frame.contains(&format!("{SELECTED_LINE_PREFIX}▌ /palette")));
    }

    #[test]
    fn enter_executes_selected_command_palette_item() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"/"),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, &[27, b'[', b'B']),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.overlay_selected, 1);
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );

        assert_eq!(state.transcript[0].body, "/palette");
        assert!(state.transcript[1].body.contains("命令"));
    }

    #[test]
    fn arrow_keys_move_skill_palette_selection_and_enter_inspects_selected_skill() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"$"),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, &[27, b'[', b'B']),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );

        assert_eq!(state.transcript[0].body, "$research-review");
        assert!(state.transcript[1].body.contains("$research-review"));
        assert!(state.transcript[1].body.contains("来源"));
    }

    #[test]
    fn typing_dollar_renders_chinese_skill_palette_overlay_by_default() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, &[b'$']),
            TuiInputOutcome::Continue
        );
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );

        assert!(frame.contains("技能"));
        assert!(!frame.contains("技能 / Skills"));
        assert!(frame.contains("$web-design-engineer"));
        assert!(frame.contains("$research-review"));
        assert!(frame.contains("输入 $list 查看全部"));
    }

    #[test]
    fn rich_inline_prompt_frame_renders_skill_palette_with_selected_row() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"$"),
            TuiInputOutcome::Continue
        );
        assert_eq!(
            handle_tui_input(&mut state, &[27, b'[', b'B']),
            TuiInputOutcome::Continue
        );
        let model = terminal_model_from_state_for_test(&state);

        let frame = render_rich_inline_prompt_frame_with_width(&model, 104);

        assert!(frame.contains("╭─ 技能"));
        assert!(frame.contains("输入 $list 查看全部"));
        assert!(frame.contains(&format!("{SELECTED_LINE_PREFIX}▌ $research-review")));
        assert!(frame.contains("审查方案"));
        assert!(frame.contains("Ask Astra"));
        assert!(frame.contains("$ 技能"));
        assert!(!frame.contains(" / Skills"));
    }

    #[test]
    fn rich_inline_prompt_frame_keeps_narrow_chinese_rows_within_width() {
        let mut state = test_interaction_state();
        state.composer = "请阅读这个错误并修改实现：中文宽度必须正确".to_string();
        let model = terminal_model_from_state_for_test(&state);

        let frame =
            render_rich_inline_prompt_frame_with_width_and_cursor(&model, 40, "请阅读".len());

        assert_eq!(visible_width("中文"), 4);
        for line in frame.lines() {
            assert!(
                visible_width(line) <= 40,
                "line is too wide: {} > 40: {line}",
                visible_width(line)
            );
        }
    }

    #[test]
    fn rich_inline_running_card_keeps_ui_visible_during_streaming_turn() {
        let state = test_interaction_state();

        let card = render_rich_inline_running_turn_card(
            &state,
            "修复 TUI 输入框，并解释为什么需要 /exc",
            80,
        );

        assert!(card.contains("Astra"));
        assert!(card.contains("正在执行回合"));
        assert!(card.contains("/exc"));
        assert!(card.contains("流式输出"));
        assert!(card.contains("╭"));
    }

    #[test]
    fn raw_mode_line_endings_use_carriage_return_newline() {
        assert_eq!(raw_mode_line_endings("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn rich_inline_clipped_composer_preserves_visible_cursor() {
        let mut state = test_interaction_state();
        state.composer = "请阅读这个很长很长很长的错误并修改实现：中文宽度必须正确".to_string();
        let model = terminal_model_from_state_for_test(&state);

        let frame =
            render_rich_inline_prompt_frame_with_width_and_cursor(&model, 40, state.composer.len());

        assert!(frame.contains("\x1b[7m"));
        for line in frame.lines() {
            assert!(
                visible_width(line) <= 40,
                "line is too wide: {} > 40: {line}",
                visible_width(line)
            );
        }
    }

    #[test]
    fn rich_inline_composer_history_includes_slash_and_skill_inputs() {
        let mut state = test_interaction_state();
        push_input_history_entry(&mut state, "/model gpt-5.5");
        push_input_history_entry(&mut state, "$research-review");

        let mut composer = composer_state_from_interaction(&state);
        composer.apply(ComposerAction::HistoryPrevious);
        assert_eq!(composer.text(), "$research-review");
        composer.apply(ComposerAction::HistoryPrevious);
        assert_eq!(composer.text(), "/model gpt-5.5");
    }

    #[test]
    fn composer_visual_lines_show_cursor_inside_multiline_utf8_text() {
        let lines = composer_visual_lines_with_cursor("你好\nworld", "你".len());

        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("你\x1b[7m好\x1b[0m"));
        assert_eq!(lines[1], "world");

        let second_line = composer_visual_lines_with_cursor("你好\nworld", "你好\nwo".len());
        assert!(second_line[1].contains("wo\x1b[7mr\x1b[0mld"));
    }

    #[test]
    fn slash_palette_filters_as_user_types() {
        let mut state = test_interaction_state();
        for byte in b"/ter" {
            assert_eq!(
                handle_tui_input(&mut state, &[*byte]),
                TuiInputOutcome::Continue
            );
        }
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );

        assert!(frame.contains("/terminal"));
        assert!(frame.contains("/terminal replay"));
        assert!(!frame.contains("/permissions"));
    }

    #[test]
    fn language_command_switches_between_chinese_and_english() {
        let mut state = test_interaction_state();

        state.composer = "/language en".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "2".to_string(),
            permission_count: "1".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: state.working_dir.clone(),
            git_branch: state.git_branch.clone(),
            research_line: state.research_line.clone(),
            transcript: state.transcript.clone(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);
        let english_frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );
        assert!(english_frame.contains("Research"));
        assert!(english_frame.contains("Composer"));
        assert!(english_frame.contains("Ask Astra"));
        assert!(english_frame.contains("/ commands"));
        assert!(!english_frame.contains("/ 命令"));

        state.composer = "/language zh".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        apply_interaction_to_model(&mut model, &state);
        let chinese_frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );
        assert!(chinese_frame.contains("研究"));
        assert!(chinese_frame.contains("输入"));
        assert!(chinese_frame.contains("/ 命令"));
        assert!(!chinese_frame.contains(" / Research"));
    }

    #[test]
    fn theme_command_switches_between_day_and_night_modes() {
        let mut state = test_interaction_state();

        state.composer = "/theme dark".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        assert!(state.transcript.last().unwrap().body.contains("夜间"));

        state.composer = "/theme light".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        assert!(state.transcript.last().unwrap().body.contains("白天"));
    }

    #[test]
    fn theme_command_success_copy_follows_active_language() {
        let mut state = test_interaction_state();

        state.composer = "/language en".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        state.composer = "/theme dark".to_string();
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );

        let response = state.transcript.last().unwrap().body.as_str();
        assert!(response.contains("dark mode"));
        assert!(!response.contains("夜间"));
    }

    #[test]
    fn research_header_uses_thread_id_when_title_is_absent() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status available | thread_id thread_42 | thread_title none | stage implement | class coding | mode debugging | next test".to_string(),
            transcript: Vec::new(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let info = research_info_segments(&model);
        assert_eq!(info.thread, "thread_42");
        assert_eq!(info.state, "研究上下文");

        let frame = ratatui_frame_text(&model, 100, 22);

        assert!(frame.contains("thread_42"));
        assert!(!frame.contains("等待研究线程"));
    }

    #[test]
    fn terminal_frame_renders_active_orchestration_progress() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status available | thread_id thread_42 | thread_title Runtime | stage implement | class coding | mode debugging | next test || 运行 run_123 | status running | progress 2/4 · 50% | current Wire TUI checklist".to_string(),
            transcript: Vec::new(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 22,
            },
        );

        assert!(frame.contains("运行  run_123"));
        assert!(frame.contains("50%"));
        assert!(frame.contains("Wire TUI checklist"));
    }

    #[test]
    fn terminal_frame_defaults_to_day_theme_label_without_governance_noise() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status active | thread t1 | stage coding | next test".to_string(),
            transcript: Vec::new(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(&model, TerminalSize { cols: 96, rows: 28 });

        assert!(frame.contains("日间"));
        assert!(!frame.contains("待处理"));
        assert!(!frame.contains("置信"));
    }

    #[test]
    fn ratatui_frame_uses_projected_status_instead_of_hardcoded_model_copy() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "7".to_string(),
            permission_count: "3".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status active | thread t1 | stage coding | next test".to_string(),
            transcript: Vec::new(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::En,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = ratatui_frame_text(&model, 100, 22);

        assert!(frame.contains("ready"));
        assert!(frame.contains("permission read-only"));
        assert!(frame.contains("light"));
        assert!(!frame.contains("sessions 7"));
        assert!(!frame.contains("approvals 3"));
        assert!(!frame.contains("gpt-5.5"));
        assert!(!frame.contains("model gpt"));
    }

    #[test]
    fn ratatui_english_composer_localizes_shortcut_labels() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status active | thread t1 | stage coding | next test".to_string(),
            transcript: Vec::new(),
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::En,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = ratatui_frame_text(&model, 100, 22);

        assert!(frame.contains("/ commands"));
        assert!(frame.contains("$ skills"));
        assert!(frame.contains("Ask Astra"));
        assert!(frame.contains("Ask Astra to edit"));
        assert!(frame.contains("IME text: confirm in your OS field, then paste"));
        assert!(!frame.contains("raw"));
        assert!(!frame.contains("/doctor"));
        assert!(!frame.contains("warning"));
        assert!(!frame.contains("/ 命令"));
        assert!(!frame.contains("$ 技能"));
    }

    #[test]
    fn routed_command_responses_default_to_chinese_product_copy() {
        let state = test_interaction_state();

        let terminal = route_typed_command(&state, "/terminal");
        assert!(terminal.contains("受控终端投影"));
        assert!(terminal.contains("远程状态"));
        assert!(!terminal.contains("命令已暂存"));
        assert!(!terminal.contains(" / Governed terminal projection"));

        let skills = route_typed_command(&state, "$list");
        assert!(skills.contains("技能"));
        assert!(skills.contains("使用 $skill-name 查看"));
        assert!(!skills.contains(" / Skills"));

        let unknown = route_typed_command(&state, "/not-real");
        assert!(unknown.contains("未知命令"));
        assert!(!unknown.contains(" / Unknown command"));
    }

    #[test]
    fn terminal_frame_splits_multiline_agent_turns_into_stable_rows() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "status active | thread t1 | stage coding | next test".to_string(),
            transcript: vec![TranscriptTurn {
                role: "Astra",
                body: "hello\nworld\nsession s | turn t | provider openai".to_string(),
            }],
            composer: String::new(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(&model, TerminalSize { cols: 96, rows: 28 });

        assert!(frame.contains("╭─ Astra"));
        assert!(frame.contains("│ hello"));
        assert!(frame.contains("│ world"));
        assert!(frame.contains("│ session s | turn t | provider openai"));
    }

    #[test]
    fn terminal_frame_truncates_long_utf8_without_panicking() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "研究线程正在收集证据并准备实现".repeat(6),
            transcript: vec![TranscriptTurn {
                role: "User",
                body: "请继续分析这个代码库并修复移动端和终端体验".repeat(12),
            }],
            composer: "继续修复中文输入路径".repeat(8),
            key_hints: vec!["/ commands".to_string(), "$ skills".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(&model, TerminalSize { cols: 40, rows: 18 });

        assert!(frame.contains('~'));
        assert!(frame.contains("Astra"));
    }

    #[test]
    fn terminal_frame_respects_narrow_terminal_width() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "stage coding".to_string(),
            transcript: Vec::new(),
            composer: "/help".to_string(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let frame = render_terminal_frame(&model, TerminalSize { cols: 40, rows: 18 });

        for line in frame.split("\r\n").skip(1) {
            let visible = strip_ansi(line);
            assert!(
                visible.chars().count() <= 40,
                "line should fit 40 columns but was {} chars: {line:?}",
                visible.chars().count()
            );
        }
    }

    #[test]
    fn tui_input_accepts_text_and_submits_without_control_key_only_paths() {
        let mut state = test_interaction_state();

        for byte in b"/terminal" {
            assert_eq!(
                handle_tui_input(&mut state, &[*byte]),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(state.composer, "/terminal");
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.composer, "");
        assert_eq!(state.transcript[0].body, "/terminal");
        assert!(state.transcript[1].body.contains("remote terminal attach"));

        for byte in b"$list" {
            assert_eq!(
                handle_tui_input(&mut state, &[*byte]),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        assert!(state
            .transcript
            .last()
            .expect("skill response")
            .body
            .contains("$web-design-engineer"));
    }

    #[test]
    fn tui_input_accepts_utf8_and_paste_chunks() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, "研究 /research".as_bytes()),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.composer, "研究 /research");
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.transcript[0].body, "研究 /research");
    }

    #[test]
    fn tui_prompt_submission_invokes_cli_turn_executor() {
        let mut state = test_interaction_state();
        let mut prompts = Vec::new();
        let mut executor = |prompt: &str| {
            prompts.push(prompt.to_string());
            Ok(TuiCommandExecution {
                body: format!("closed-loop result for {prompt}"),
                ..Default::default()
            })
        };

        for byte in b"/prompt inspect runtime status" {
            assert_eq!(
                handle_tui_input_with_executor(&mut state, &[*byte], &mut executor),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input_with_executor(&mut state, b"\n", &mut executor),
            TuiInputOutcome::Continue
        );

        assert_eq!(prompts, vec!["inspect runtime status"]);
        assert_eq!(state.transcript[0].body, "/prompt inspect runtime status");
        assert!(state.transcript[1]
            .body
            .contains("closed-loop result for inspect runtime status"));
    }

    #[test]
    fn tui_plain_text_submission_invokes_cli_turn_executor() {
        let mut state = test_interaction_state();
        let mut prompts = Vec::new();
        let mut executor = |prompt: &str| {
            prompts.push(prompt.to_string());
            Ok(TuiCommandExecution {
                body: format!("agent completed {prompt}"),
                ..Default::default()
            })
        };

        for byte in b"review this change" {
            assert_eq!(
                handle_tui_input_with_executor(&mut state, &[*byte], &mut executor),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input_with_executor(&mut state, b"\n", &mut executor),
            TuiInputOutcome::Continue
        );

        assert_eq!(prompts, vec!["review this change"]);
        assert_eq!(state.transcript[0].body, "review this change");
        assert!(state.transcript[1].body.contains("agent completed"));
    }

    #[test]
    fn tui_streaming_prompt_refreshes_while_executor_is_running() {
        let mut state = test_interaction_state();
        state.composer = "/prompt slow research turn".to_string();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |prompt: &str, _stream, _cancel| {
                release_rx.recv().expect("test should release executor");
                Ok(TuiCommandExecution {
                    body: format!("finished {prompt}"),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);

        assert_eq!(state.transcript[0].body, "/prompt slow research turn");
        assert!(state.transcript[1].body.contains("正在运行"));
        assert!(state.running_turn.is_some());

        refresh_streaming_turn(&mut state);
        let running_body = state.transcript[1].body.clone();
        assert!(running_body.contains("流式刷新"));
        assert_ne!(running_body, "finished slow research turn");

        release_tx.send(()).expect("release executor");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            if state.running_turn.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(state.running_turn.is_none());
        assert_eq!(state.transcript[1].body, "finished slow research turn");
    }

    #[test]
    fn tui_streaming_prompt_appends_token_deltas_before_completion() {
        let mut state = test_interaction_state();
        state.composer = "/prompt stream answer".to_string();
        let (delta_tx, delta_rx) = std::sync::mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                delta_rx.recv().expect("test should release executor delta")();
                delta_rx
                    .recv()
                    .expect("test should release second executor delta")();
                Ok(TuiCommandExecution {
                    body: "hello world\nsession sess | turn turn | provider openai | model gpt-5.5"
                        .to_string(),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);
        let sender = state
            .running_turn
            .as_ref()
            .expect("running turn")
            .delta_sender();

        delta_tx
            .send(Box::new(move || {
                sender.send_delta("hello ");
            }))
            .expect("send first delta closure");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            if state.transcript[1].body.contains("hello ") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(state.transcript[1].body.contains("hello "));
        assert!(state.running_turn.is_some());

        let sender = state
            .running_turn
            .as_ref()
            .expect("running turn")
            .delta_sender();
        delta_tx
            .send(Box::new(move || {
                sender.send_delta("world");
            }))
            .expect("send second delta closure");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            if state.running_turn.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(state.running_turn.is_none());
        assert!(state.transcript[1].body.starts_with("hello world"));
        assert!(!state.transcript[1].body.contains("provider openai"));
    }

    #[test]
    fn inline_repl_completion_suppresses_duplicate_final_body_after_streaming_deltas() {
        let execution = TuiCommandExecution {
            body: "hello world\nsession sess | turn turn | provider openai | model gpt-5.5\nresearch task classification | confidence high | next cite evidence".to_string(),
                ..Default::default()
        };

        let rendered = inline_completion_body_after_streaming(&execution, true);

        assert!(!rendered.contains("hello world"));
        assert!(!rendered.contains("session sess"));
        assert!(!rendered.contains("research task classification"));
        assert!(rendered.trim().is_empty());
    }

    #[test]
    fn inline_repl_completion_keeps_final_body_when_no_streaming_delta_arrived() {
        let execution = TuiCommandExecution {
            body: "hello world\nsession sess | turn turn | provider openai | model gpt-5.5"
                .to_string(),
            ..Default::default()
        };

        let rendered = inline_completion_body_after_streaming(&execution, false);

        assert!(rendered.contains("hello world"));
        assert!(!rendered.contains("session sess"));
    }

    #[test]
    fn tui_prompt_payload_or_plain_text_classifies_plain_text_without_routing_prefix() {
        assert_eq!(
            prompt_payload_or_plain_text(" review this change "),
            Some(Ok("review this change".to_string()))
        );
        assert_eq!(
            prompt_payload_or_plain_text("继续分析这个问题"),
            Some(Ok("继续分析这个问题".to_string()))
        );
        assert_eq!(
            prompt_payload_or_plain_text("/prompt inspect runtime"),
            Some(Ok("inspect runtime".to_string()))
        );
        assert_eq!(prompt_payload_or_plain_text("/help"), None);
        assert_eq!(prompt_payload_or_plain_text("$research-review"), None);
    }

    #[test]
    fn non_tty_repl_submission_reads_stdin_text_for_prompt_executor_path() {
        let input = read_non_tty_repl_submission_from_reader("  review this change\n".as_bytes())
            .expect("stdin text should read")
            .expect("stdin text should submit");
        assert_eq!(input, "review this change");

        let mut state = test_interaction_state();
        let prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let prompts_for_executor = std::sync::Arc::clone(&prompts);
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |prompt: &str, stream: TuiStreamSender, _cancel| {
                prompts_for_executor
                    .lock()
                    .expect("prompt log lock")
                    .push(prompt.to_string());
                stream.send_delta("streamed answer\n");
                Ok(TuiCommandExecution {
                    body: format!("streamed answer\nsession sess | prompt {prompt}"),
                    ..Default::default()
                })
            },
        ));
        let config_executor =
            std::sync::Arc::new(std::sync::Mutex::new(|_action: TuiConfigAction| {
                Err("TUI configuration executor is not bound".to_string())
            }));
        let permission_executor =
            std::sync::Arc::new(std::sync::Mutex::new(|_action: TuiPermissionAction| {
                Err("TUI permission executor is not bound".to_string())
            }));

        let output = run_non_tty_inline_repl_submission(
            &mut state,
            &input,
            &executor,
            &config_executor,
            &permission_executor,
        )
        .expect("non-tty submission should execute");

        assert_eq!(
            prompts.lock().expect("prompt log lock").as_slice(),
            ["review this change"]
        );
        assert!(output.contains("streamed answer"));
        assert!(!output.contains("session sess"));
        assert_eq!(strip_ansi(&output), output);
    }

    #[test]
    fn non_tty_repl_submission_preserves_multiline_prompt_body() {
        let input = read_non_tty_repl_submission_from_reader(
            "summarize:\n- first point\n- second point\n\n".as_bytes(),
        )
        .expect("stdin text should read")
        .expect("stdin text should submit");

        assert_eq!(input, "summarize:\n- first point\n- second point");
    }

    #[test]
    fn inline_repl_interrupt_helper_cancels_running_turn_token() {
        let (token, interrupt_handle) = runtime_interrupt_pair();
        let requested = std::sync::atomic::AtomicBool::new(false);

        assert!(!interrupt_if_requested(&interrupt_handle, &requested));
        assert!(!token.is_cancelled());

        requested.store(true, std::sync::atomic::Ordering::SeqCst);

        assert!(interrupt_if_requested(&interrupt_handle, &requested));
        assert!(token.is_cancelled());
    }

    #[test]
    fn inline_turn_interrupt_accepts_exc_text_command_and_legacy_alias() {
        let (token, interrupt_handle) = runtime_interrupt_pair();
        let mut typed_buffer = String::new();

        assert!(!apply_inline_turn_interrupt_text(
            &interrupt_handle,
            &mut typed_buffer,
            "/e"
        ));
        assert_eq!(typed_buffer, "/e");
        assert!(apply_inline_turn_interrupt_text(
            &interrupt_handle,
            &mut typed_buffer,
            "xc"
        ));
        assert!(token.is_cancelled());
        assert_eq!(typed_buffer, "");

        let (legacy_token, legacy_handle) = runtime_interrupt_pair();
        let mut legacy_buffer = String::new();
        assert!(apply_inline_turn_interrupt_text(
            &legacy_handle,
            &mut legacy_buffer,
            "/interrupt"
        ));
        assert!(legacy_token.is_cancelled());
    }

    #[test]
    fn tui_streaming_plain_text_submission_invokes_cli_turn_executor() {
        let mut state = test_interaction_state();
        state.composer = "你好".to_string();
        let prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let prompts_for_executor = std::sync::Arc::clone(&prompts);
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |prompt: &str, stream: TuiStreamSender, _cancel| {
                prompts_for_executor
                    .lock()
                    .expect("prompt capture lock")
                    .push(prompt.to_string());
                stream.send_delta("模型");
                Ok(TuiCommandExecution {
                    body: format!("真实 runtime 回复: {prompt}"),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            if state.running_turn.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert_eq!(state.transcript[0].body, "你好");
        assert!(state.transcript[1].body.contains("真实 runtime 回复: 你好"));
        assert!(!state.transcript[1].body.contains("提示已接收"));

        state.composer = "review this change".to_string();
        submit_composer_streaming(&mut state, &executor);
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            if state.running_turn.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert_eq!(
            prompts.lock().expect("prompt capture lock").as_slice(),
            ["你好".to_string(), "review this change".to_string()]
        );
        assert_eq!(state.transcript[2].body, "review this change");
        assert!(state.transcript[3]
            .body
            .contains("真实 runtime 回复: review this change"));
    }

    #[test]
    fn tui_streaming_slash_and_skill_submissions_do_not_invoke_cli_turn_executor() {
        let mut state = test_interaction_state();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let calls_for_executor = std::sync::Arc::clone(&calls);
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream: TuiStreamSender, _cancel| {
                *calls_for_executor.lock().expect("call count lock") += 1;
                Ok(TuiCommandExecution {
                    body: "executor should not run".to_string(),
                    ..Default::default()
                })
            },
        ));

        state.composer = "/help".to_string();
        submit_composer_streaming(&mut state, &executor);
        state.composer = "$research-review".to_string();
        submit_composer_streaming(&mut state, &executor);

        assert_eq!(*calls.lock().expect("call count lock"), 0);
        assert!(state.running_turn.is_none());
        assert_eq!(state.transcript[0].body, "/help");
        assert_eq!(state.transcript[2].body, "$research-review");
        assert!(!state
            .transcript
            .iter()
            .any(|turn| turn.body.contains("executor should not run")));
    }

    #[test]
    fn tui_streaming_running_turn_accepts_typed_interrupt() {
        let mut state = test_interaction_state();
        state.composer = "/prompt slow research turn".to_string();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                release_rx.recv().expect("test should release executor");
                Ok(TuiCommandExecution {
                    body: "late completion must be ignored".to_string(),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);
        assert!(state.running_turn.is_some());

        state.composer = "/exc".to_string();
        assert_eq!(
            handle_tui_input_streaming(&mut state, b"\n", &executor),
            TuiInputOutcome::Continue
        );

        assert!(state.running_turn.is_none());
        assert_eq!(state.composer, "");
        assert!(state.transcript.iter().any(|turn| turn.body == "/exc"));
        let interrupted_body = state.transcript[1].body.clone();
        assert!(interrupted_body.contains("中断") || interrupted_body.contains("取消"));
        assert!(!interrupted_body.contains("已有回合运行中"));

        release_tx.send(()).expect("release executor");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(state.transcript[1].body, interrupted_body);
    }

    #[test]
    fn tui_streaming_ctrl_c_interrupts_running_turn_before_exit() {
        let mut state = test_interaction_state();
        state.composer = "/prompt cancellable turn".to_string();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                release_rx.recv().expect("test should release executor");
                Ok(TuiCommandExecution {
                    body: "late ctrl-c completion must be ignored".to_string(),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);
        assert!(state.running_turn.is_some());

        assert_eq!(
            handle_tui_input_streaming(&mut state, &[3], &executor),
            TuiInputOutcome::Continue
        );

        assert!(state.running_turn.is_none());
        let interrupted_body = state.transcript[1].body.clone();
        assert!(interrupted_body.contains("中断") || interrupted_body.contains("取消"));

        release_tx.send(()).expect("release executor");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(state.transcript[1].body, interrupted_body);
    }

    #[test]
    fn tui_streaming_esc_interrupts_running_turn_without_exiting() {
        let mut state = test_interaction_state();
        state.composer = "/prompt esc cancellable turn".to_string();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                release_rx.recv().expect("test should release executor");
                Ok(TuiCommandExecution {
                    body: "late esc completion must be ignored".to_string(),
                    ..Default::default()
                })
            },
        ));

        submit_composer_streaming(&mut state, &executor);
        assert!(state.running_turn.is_some());

        assert_eq!(
            handle_tui_input_streaming(&mut state, &[27], &executor),
            TuiInputOutcome::Continue
        );

        assert!(state.running_turn.is_none());
        assert_eq!(state.composer, "");
        let interrupted_body = state.transcript[1].body.clone();
        assert!(interrupted_body.contains("中断") || interrupted_body.contains("取消"));

        release_tx.send(()).expect("release executor");
        for _ in 0..20 {
            refresh_streaming_turn(&mut state);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(state.transcript[1].body, interrupted_body);
    }

    #[test]
    fn tui_streaming_ctrl_c_clears_composer_before_exit_when_idle() {
        let mut state = test_interaction_state();
        state.composer = "draft".to_string();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                Ok(TuiCommandExecution {
                    body: "unused".to_string(),
                    ..Default::default()
                })
            },
        ));

        assert_eq!(
            handle_tui_input_streaming(&mut state, &[3], &executor),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.composer, "");
        assert_eq!(
            handle_tui_input_streaming(&mut state, &[3], &executor),
            TuiInputOutcome::Exit("ctrl-c")
        );
    }

    #[test]
    fn tui_streaming_exit_command_still_exits_when_no_turn_is_running() {
        let mut state = test_interaction_state();
        let executor = std::sync::Arc::new(std::sync::Mutex::new(
            move |_prompt: &str, _stream, _cancel| {
                Ok(TuiCommandExecution {
                    body: "unused".to_string(),
                    ..Default::default()
                })
            },
        ));

        for byte in b"/exit" {
            assert_eq!(
                handle_tui_input_streaming(&mut state, &[*byte], &executor),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input_streaming(&mut state, b"\n", &executor),
            TuiInputOutcome::Exit("slash-exit")
        );
    }

    #[test]
    fn tui_router_handles_every_displayed_slash_command() {
        let state = test_interaction_state();
        let mut commands = vec!["/help", "/palette", "/slash"]
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        commands.extend(
            product_command_specs()
                .into_iter()
                .map(|spec| routable_sample_for_command(&spec.typed)),
        );
        commands.push("/prompt".to_string());

        for command in commands {
            let response = route_typed_command(&state, &command);
            assert!(
                !response.contains("Unknown / command"),
                "{command} should be routed, got {response}"
            );
        }
    }

    #[test]
    fn command_help_lists_every_product_command_with_localized_labels() {
        let state = test_interaction_state();
        let help = render_command_help(&state);

        for spec in product_command_specs() {
            assert!(
                help.contains(&spec.typed),
                "help should list {}, got:\n{help}",
                spec.typed
            );
        }
        assert!(help.contains("/status"));
        assert!(help.contains("/research"));
        assert!(help.contains("/logs"));
        assert!(help.contains("状态"));
        assert!(help.contains("思考强度"));
        // Group headers appear (Chinese localized).
        assert!(help.contains("核心"));
        assert!(help.contains("会话"));
        assert!(help.contains("技能"));
        assert!(!help.contains("//help"));
        // Old flat-list format with inline [category] tag is gone.
        assert!(!help.contains("Reasoning [配置]"));
        assert!(!help.contains("Inspect or change reasoning effort"));
        assert!(!help.contains("/mcp"));
    }

    #[test]
    fn command_palette_hides_internal_debug_commands_from_primary_surface() {
        let state = test_interaction_state();
        let typed = state
            .command_entries
            .iter()
            .map(|entry| entry.typed.as_str())
            .collect::<Vec<_>>();

        for internal in ["/doctor", "/config", "/tools", "/mcp", "/providers"] {
            assert!(
                !typed.contains(&internal),
                "{internal} should not be exposed in the primary TUI palette"
            );
        }
        assert!(typed.contains(&"/model"));
        assert!(typed.contains(&"/reasoning"));
    }

    #[test]
    fn model_command_accepts_catalog_model_and_explains_provider_routing() {
        let state = test_interaction_state();

        let overview = route_typed_command(&state, "/model");
        assert!(overview.contains("模型选择"));
        assert!(overview.contains("gpt-5.5"));
        assert!(overview.contains("Provider"));

        let selected = route_typed_command(&state, "/model gpt 5.5");
        assert!(selected.contains("模型设置失败"));
        assert!(selected.contains("configuration executor"));
    }

    #[test]
    fn model_command_applies_through_config_executor_and_updates_state() {
        let mut state = test_interaction_state();
        let mut calls = Vec::new();
        let response = route_typed_command_mut_with_config(
            &mut state,
            "/model gpt 5.5",
            &mut |action: TuiConfigAction| {
                calls.push(action.clone());
                Ok(TuiConfigActionResult {
                    applied: true,
                    provider_id: Some("openai".to_string()),
                    model: Some("gpt-5.5".to_string()),
                    reasoning_effort: None,
                    session_id: None,
                    scope: "project".to_string(),
                    message: "model set gpt-5.5 --scope project".to_string(),
                })
            },
        );

        assert_eq!(
            calls,
            vec![TuiConfigAction {
                kind: TuiConfigActionKind::Model,
                value: "gpt-5.5".to_string(),
            }]
        );
        assert_eq!(state.model_label, "openai/gpt-5.5");
        assert!(response.contains("模型已应用：openai/gpt-5.5"));
        assert!(response.contains("后续对话会使用该模型"));
    }

    #[test]
    fn reasoning_command_accepts_effort_without_debug_surface() {
        let state = test_interaction_state();

        let overview = route_typed_command(&state, "/reasoning");
        assert!(overview.contains("思考强度"));
        assert!(overview.contains("auto、low、medium、high"));

        let selected = route_typed_command(&state, "/reasoning high");
        assert!(selected.contains("思考强度设置失败"));
        assert!(selected.contains("configuration executor"));

        let invalid = route_typed_command(&state, "/reasoning huge");
        assert!(invalid.contains("未知思考强度"));
    }

    #[test]
    fn reasoning_command_applies_through_config_executor_and_updates_state() {
        let mut state = test_interaction_state();
        let mut calls = Vec::new();
        let response = route_typed_command_mut_with_config(
            &mut state,
            "/reasoning high",
            &mut |action: TuiConfigAction| {
                calls.push(action.clone());
                Ok(TuiConfigActionResult {
                    applied: true,
                    provider_id: None,
                    model: None,
                    reasoning_effort: Some("high".to_string()),
                    session_id: None,
                    scope: "project".to_string(),
                    message: "config set reasoning_effort high --scope project".to_string(),
                })
            },
        );

        assert_eq!(
            calls,
            vec![TuiConfigAction {
                kind: TuiConfigActionKind::Reasoning,
                value: "high".to_string(),
            }]
        );
        assert_eq!(state.reasoning_effort, "high");
        assert!(response.contains("思考强度已应用：high"));
        assert!(response.contains("后续对话会使用该强度"));
    }

    #[test]
    fn approve_permission_command_executes_permission_action_and_updates_state() {
        let mut state = test_interaction_state();
        state.permission_count = "1".to_string();
        let mut calls = Vec::new();

        let response = route_typed_command_mut_with_executors(
            &mut state,
            "/approve req_123",
            &mut |_action: TuiConfigAction| {
                Err("TUI configuration executor should not run".to_string())
            },
            &mut |action: TuiPermissionAction| {
                calls.push(action.clone());
                Ok(TuiPermissionActionResult {
                    request_id: "req_123".to_string(),
                    decision: "approved".to_string(),
                    pending_count: Some(0),
                    message: "permissions approve req_123 --json".to_string(),
                })
            },
        );

        assert_eq!(
            calls,
            vec![TuiPermissionAction {
                decision: TuiPermissionDecision::Approve,
                request_id: "req_123".to_string(),
            }]
        );
        assert_eq!(state.permission_count, "0");
        assert!(response.contains("req_123"));
        assert!(response.contains("approved"));
        assert!(response.contains("下一步"));
        assert!(!response.contains("未执行变更"));
    }

    #[test]
    fn deny_permission_command_executes_permission_action_and_updates_state() {
        let mut state = test_interaction_state();
        state.permission_count = "2".to_string();
        let mut calls = Vec::new();

        let response = route_typed_command_mut_with_executors(
            &mut state,
            "/deny req_456",
            &mut |_action: TuiConfigAction| {
                Err("TUI configuration executor should not run".to_string())
            },
            &mut |action: TuiPermissionAction| {
                calls.push(action.clone());
                Ok(TuiPermissionActionResult {
                    request_id: "req_456".to_string(),
                    decision: "denied".to_string(),
                    pending_count: Some(1),
                    message: "permissions deny req_456 --json".to_string(),
                })
            },
        );

        assert_eq!(
            calls,
            vec![TuiPermissionAction {
                decision: TuiPermissionDecision::Deny,
                request_id: "req_456".to_string(),
            }]
        );
        assert_eq!(state.permission_count, "1");
        assert!(response.contains("req_456"));
        assert!(response.contains("denied"));
        assert!(response.contains("下一步"));
        assert!(!response.contains("未执行变更"));
    }

    #[test]
    fn permission_commands_without_request_id_return_usage() {
        let mut state = test_interaction_state();

        let approve = route_typed_command_mut_with_executors(
            &mut state,
            "/approve",
            &mut |_action: TuiConfigAction| {
                Err("TUI configuration executor should not run".to_string())
            },
            &mut |_action: TuiPermissionAction| {
                Err("TUI permission executor should not run".to_string())
            },
        );
        let deny = route_typed_command_mut_with_executors(
            &mut state,
            "/deny",
            &mut |_action: TuiConfigAction| {
                Err("TUI configuration executor should not run".to_string())
            },
            &mut |_action: TuiPermissionAction| {
                Err("TUI permission executor should not run".to_string())
            },
        );

        assert!(approve.contains("用法：/approve <request-id>"));
        assert!(deny.contains("用法：/deny <request-id>"));
        assert!(!approve.contains("未知命令"));
        assert!(!deny.contains("未知命令"));
    }

    #[test]
    fn permissions_inspect_reports_live_state_without_staged_copy() {
        let mut state = test_interaction_state();
        state.permission_count = "3".to_string();
        state.permission_mode = "workspace-write".to_string();

        let response = route_typed_command_mut_with_executors(
            &mut state,
            "/permissions",
            &mut |_action: TuiConfigAction| {
                Err("TUI configuration executor should not run".to_string())
            },
            &mut |_action: TuiPermissionAction| {
                Err("TUI permission executor should not run".to_string())
            },
        );

        assert!(response.contains("权限状态"));
        assert!(response.contains("待审批请求： 3"));
        assert!(response.contains("权限模式： workspace-write"));
        assert!(!response.contains("命令已暂存"));
    }

    #[test]
    fn history_count_parser_accepts_positive_count_and_rejects_invalid_values() {
        assert_eq!(parse_history_count(None), Ok(DEFAULT_HISTORY_LIMIT));
        assert_eq!(parse_history_count(Some("3")), Ok(3));
        assert!(parse_history_count(Some("0"))
            .expect_err("zero should be rejected")
            .contains("greater than 0"));
        assert!(parse_history_count(Some("two"))
            .expect_err("non-numeric should be rejected")
            .contains("invalid count"));
    }

    #[test]
    fn history_command_reports_empty_prompt_history_without_projection_copy() {
        let mut state = test_interaction_state();

        let response = route_typed_command_mut(&mut state, "/history");

        assert!(response.contains("Prompt history"));
        assert!(response.contains("no prompts recorded yet"));
        assert!(!response.contains("CLI:"));
    }

    #[test]
    fn history_command_respects_count_and_uses_real_state_entries() {
        let mut state = test_interaction_state();
        state.prompt_history = vec![
            PromptHistoryEntry {
                timestamp_ms: 1_700_000_000_000,
                text: "first prompt".to_string(),
            },
            PromptHistoryEntry {
                timestamp_ms: 1_700_000_001_000,
                text: "second prompt".to_string(),
            },
            PromptHistoryEntry {
                timestamp_ms: 1_700_000_002_000,
                text: "third prompt".to_string(),
            },
        ];

        let response = route_typed_command_mut(&mut state, "/history 2");

        assert!(response.contains("Showing          2 most recent"));
        assert!(!response.contains("first prompt"));
        assert!(response.contains("second prompt"));
        assert!(response.contains("third prompt"));
    }

    #[test]
    fn sessions_command_lists_real_recent_sessions_and_marks_active() {
        let mut state = test_interaction_state();
        state.recent_sessions = vec![
            TuiSessionEntry {
                session_id: "sess_new".to_string(),
                title: Some("New work".to_string()),
                updated_at: "2026-04-30T10:00:00Z".to_string(),
                status: "active".to_string(),
            },
            TuiSessionEntry {
                session_id: "sess_active".to_string(),
                title: None,
                updated_at: "2026-04-29T10:00:00Z".to_string(),
                status: "active".to_string(),
            },
        ];
        state.session_count = "2".to_string();

        let response = route_typed_command_mut(&mut state, "/sessions");

        assert!(response.contains("Sessions"));
        assert!(response.contains("2 total"));
        assert!(response.contains("sess_new"));
        assert!(response.contains("New work"));
        assert!(response.contains("* sess_active"));
        assert!(response.contains("/resume latest"));
        assert!(!response.contains("命令已暂存"));
    }

    #[test]
    fn resume_latest_invokes_session_executor_and_updates_active_state() {
        let mut state = test_interaction_state();
        state.active_session_id = Some("sess_old".to_string());
        let mut calls = Vec::new();

        let response = route_typed_command_mut_with_config(
            &mut state,
            "/resume latest",
            &mut |action: TuiConfigAction| match action.kind {
                TuiConfigActionKind::Session(TuiSessionActionKind::Resume) => {
                    calls.push(action.clone());
                    Ok(TuiConfigActionResult::session_resumed(
                        "sess_new".to_string(),
                        "project".to_string(),
                        "sessions resume latest --json".to_string(),
                    ))
                }
                _ => Err("unexpected action".to_string()),
            },
        );

        assert_eq!(
            calls,
            vec![TuiConfigAction {
                kind: TuiConfigActionKind::Session(TuiSessionActionKind::Resume),
                value: "latest".to_string(),
            }]
        );
        assert_eq!(state.active_session_id.as_deref(), Some("sess_new"));
        assert!(response.contains("会话已恢复：sess_new"));
        assert!(response.contains("后续对话会进入该 session"));
    }

    #[test]
    fn continue_invokes_session_executor_as_resume_latest_alias() {
        let mut state = test_interaction_state();
        state.active_session_id = Some("sess_old".to_string());
        let mut calls = Vec::new();

        let response = route_typed_command_mut_with_config(
            &mut state,
            "/continue",
            &mut |action: TuiConfigAction| match action.kind {
                TuiConfigActionKind::Session(TuiSessionActionKind::Resume) => {
                    calls.push(action.clone());
                    Ok(TuiConfigActionResult::session_resumed(
                        "sess_new".to_string(),
                        "project".to_string(),
                        "continue --json".to_string(),
                    ))
                }
                _ => Err("unexpected action".to_string()),
            },
        );

        assert_eq!(
            calls,
            vec![TuiConfigAction {
                kind: TuiConfigActionKind::Session(TuiSessionActionKind::Resume),
                value: "latest".to_string(),
            }]
        );
        assert_eq!(state.active_session_id.as_deref(), Some("sess_new"));
        assert!(response.contains("会话已恢复：sess_new"));
        assert!(response.contains("后续对话会进入该 session"));
        assert!(!response.contains("命令已暂存"));
    }

    #[test]
    fn terminal_commands_explain_governed_remote_projection_contract() {
        let mut state = test_interaction_state();
        state.remote_state = "paired".to_string();

        let attach = route_typed_command(&state, "/terminal");
        assert!(attach.contains("受控终端投影"));
        assert!(attach.contains("远程状态： paired"));
        assert!(attach.contains("control lease"));
        assert!(attach.contains("websocket ticket"));
        assert!(attach.contains("CLI: remote terminal attach --json"));
        assert!(!attach.contains("命令已暂存"));

        let replay = route_typed_command(&state, "/terminal replay");
        assert!(replay.contains("终端回放投影"));
        assert!(replay.contains("cursor"));
        assert!(replay.contains("只读"));
        assert!(replay.contains("CLI: remote terminal replay --json"));
        assert!(!replay.contains("命令已暂存"));
    }

    #[test]
    fn research_command_renders_live_brief_without_staged_copy() {
        let mut state = test_interaction_state();
        state.research_line =
            "status active | thread thread_42 | stage implementation | next run_tests".to_string();

        let response = route_typed_command(&state, "/research");

        assert!(response.contains("研究简报"));
        assert!(response.contains("status active"));
        assert!(response.contains("thread thread_42"));
        assert!(response.contains("stage implementation"));
        assert!(response.contains("next run_tests"));
        assert!(!response.contains("命令已暂存"));
    }

    #[test]
    fn research_board_command_renders_explicit_inspector_copy() {
        let mut state = test_interaction_state();
        state.research_line =
            "board 7 entries | questions:2 | needs_approval:1 | blocked:1".to_string();

        let response = route_typed_command(&state, "/research board");

        assert!(response.contains("Hermes"));
        assert!(response.contains("host projection"));
        assert!(response.contains("CLI: research board --json"));
        assert!(response.contains("board 7 entries"));
        assert!(!response.contains("命令已暂存"));
    }

    #[test]
    fn tui_projected_actions_are_explicitly_staged_not_fake_executed() {
        let state = test_interaction_state();

        for (command, cli_route) in [
            ("/diff", "git diff --stat"),
            ("/commit", "git status --short"),
            ("/cost", "cost --json"),
            ("/usage", "usage --json"),
            ("/doctor", "doctor --json"),
            ("/providers", "providers list --json"),
            ("/config", "config effective --json"),
            ("/tools", "tools run read_file --path README.md --json"),
            ("/mcp", "mcp list --json"),
            ("/artifacts", "artifacts list --json"),
            ("/memory", "memory status --json"),
        ] {
            let response = route_typed_command(&state, command);
            assert!(
                response.contains("命令已暂存"),
                "{command} should make staged semantics explicit: {response}"
            );
            assert!(
                response.contains(cli_route),
                "{command} should point to a real CLI route `{cli_route}`: {response}"
            );
        }

        let approve = route_typed_command(&state, "/approve perm_1");
        assert!(approve.contains("权限请求处理失败"));
        assert!(approve.contains("TUI permission executor is not bound"));
        assert!(approve.contains("request id: perm_1"));
        assert!(!approve.contains("命令已暂存"));
        assert!(!approve.contains("未执行变更"));

        let deny = route_typed_command(&state, "/deny perm_1");
        assert!(deny.contains("权限请求处理失败"));
        assert!(deny.contains("TUI permission executor is not bound"));
        assert!(deny.contains("request id: perm_1"));
        assert!(!deny.contains("命令已暂存"));
        assert!(!deny.contains("未执行变更"));

        let terminal = route_typed_command(&state, "/terminal");
        assert!(terminal.contains("CLI: remote terminal attach --json"));
    }

    #[test]
    fn output_commands_are_typed_paths_not_shortcut_only() {
        let mut state = test_interaction_state();
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: "running cargo test\nwarning: unused variable\nerror: failed assertion\n+ added line\n- removed line\nsession s | turn t | provider fixture".to_string(),
        });

        let output = route_typed_command(&state, "/output");
        assert!(output.contains("结构化输出"));
        assert!(output.contains("错误"));
        assert!(output.contains("Diff"));
        assert!(!output.contains("未知命令"));

        let logs = route_typed_command(&state, "/logs");
        assert!(logs.contains("日志"));
        assert!(logs.contains("warning: unused variable"));
    }

    #[test]
    fn fold_and_expand_commands_change_long_output_projection() {
        let mut state = test_interaction_state();
        state.transcript.push(TranscriptTurn {
            role: "Astra",
            body: (0..16)
                .map(|index| format!("test case_{index} ... ok"))
                .collect::<Vec<_>>()
                .join("\n"),
        });

        let expanded = route_typed_command(&state, "/output");
        assert!(expanded.contains("case_15"));

        let folded = route_typed_command_mut(&mut state, "/fold");
        assert!(folded.contains("/expand"));
        assert!(state.output_folded);

        let folded_output = route_typed_command(&state, "/output");
        assert!(folded_output.contains("已折叠"));
        assert!(!folded_output.contains("case_15"));

        let expanded_again = route_typed_command_mut(&mut state, "/expand");
        assert!(expanded_again.contains("/fold"));
        assert!(!state.output_folded);
    }

    #[test]
    fn structured_output_renderer_groups_by_block_type_without_simple_line_matching_only() {
        let body = "cargo test\nwarning: unused import\nerror: compile failed\n+ new path\n- old path\nrunning 9 tests\ntest alpha ... ok\ntest beta ... ok\ntest gamma ... ok\ntest delta ... ok\ntest epsilon ... ok\ntest zeta ... ok\ntest eta ... FAILED\nsession s | turn t | provider fixture";
        let blocks = structured_output_blocks(body, false);

        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Command));
        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Warning));
        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Error));
        assert!(blocks.iter().any(|block| block.kind == TuiOutputKind::Diff));
        assert!(blocks.iter().any(|block| block.kind == TuiOutputKind::Test));
        assert!(blocks
            .iter()
            .any(|block| block.kind == TuiOutputKind::Status));

        let rendered = render_structured_output_text(body, TuiLanguage::Zh, true);
        assert!(rendered.contains("错误"));
        assert!(rendered.contains("警告"));
        assert!(rendered.contains("测试"));
        assert!(rendered.contains("测试: 6/7"));
        assert!(rendered.contains("动作: Status"));
        assert!(rendered.contains("已折叠"));
    }

    #[test]
    fn structured_output_preserves_fenced_code_as_one_code_block() {
        let body = "Here is the patch:\n```rust\nfn main() {\n    println!(\"astra\");\n}\n```\nThen run cargo test.";
        let blocks = structured_output_blocks(body, false);
        let code_blocks = blocks
            .iter()
            .filter(|block| block.title == "Code rust")
            .collect::<Vec<_>>();

        assert_eq!(code_blocks.len(), 1);
        assert_eq!(code_blocks[0].lines.len(), 5);

        let rendered = render_structured_output_text(body, TuiLanguage::En, false);
        assert!(rendered.contains("[Code] Code rust"));
        assert!(rendered.contains("language: rust"));
        assert!(rendered.contains("3 lines"));
        assert!(rendered.contains("fn main()"));
    }

    #[test]
    fn structured_output_keeps_unified_diff_headers_in_one_diff_block() {
        let body = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-old line\n+new line";
        let blocks = structured_output_blocks(body, false);
        let diff_blocks = blocks
            .iter()
            .filter(|block| block.kind == TuiOutputKind::Diff)
            .collect::<Vec<_>>();

        assert_eq!(diff_blocks.len(), 1);
        assert_eq!(diff_blocks[0].lines.len(), 6);
        assert!(diff_blocks[0]
            .lines
            .iter()
            .any(|line| line.starts_with("@@")));
        let rendered = render_structured_output_text(body, TuiLanguage::En, false);
        assert!(rendered.contains("file: src/lib.rs"));
        assert!(rendered.contains("+1 -1"));
    }

    #[test]
    fn markdown_lists_and_quotes_render_as_markdown_not_tool_output() {
        let body =
            "- keep chat first\n  - expose research only as context\n> avoid dashboard chrome";
        let blocks = structured_output_blocks(body, false);

        assert!(blocks.iter().all(|block| block.kind == TuiOutputKind::Text));
        assert!(blocks.iter().all(|block| block.title == "Markdown"));

        let rendered = render_structured_output_text(body, TuiLanguage::En, false);
        assert!(rendered.contains("[Text] Markdown"));
        assert!(rendered.contains("> avoid dashboard chrome"));
    }

    #[test]
    fn command_palette_search_matches_summaries_and_chinese_aliases() {
        let state = test_interaction_state();
        let by_chinese = matching_command_entries(&state.command_entries, "/日志");
        assert!(by_chinese.iter().any(|entry| entry.typed == "/logs"));

        let by_summary = matching_command_entries(&state.command_entries, "/collapse");
        assert!(by_summary.iter().any(|entry| entry.typed == "/fold"));
    }

    #[test]
    fn command_palette_prioritizes_typed_prefix_matches_over_secondary_text() {
        let state = test_interaction_state();

        let exit_matches = matching_command_entries(&state.command_entries, "/ex");
        let first_two = exit_matches
            .iter()
            .take(2)
            .map(|entry| entry.typed.as_str())
            .collect::<Vec<_>>();
        assert_eq!(first_two, vec!["/exc", "/exit"]);

        let interrupt_matches = matching_command_entries(&state.command_entries, "/exc");
        assert_eq!(
            interrupt_matches.first().map(|entry| entry.typed.as_str()),
            Some("/exc")
        );
    }

    #[test]
    fn command_palette_lines_show_groups_and_useful_descriptions() {
        let model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "stage coding".to_string(),
            transcript: Vec::new(),
            composer: "/fold".to_string(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 0,
            output_folded: false,
        };

        let lines = command_palette_lines(&model).join("\n");
        assert!(lines.contains("输出"));
        assert!(lines.contains("折叠"));
        assert!(lines.contains("长输出"));
        assert!(!lines.contains("action_id"));
        assert!(!lines.contains("gate"));
    }

    #[test]
    fn tui_input_keeps_control_keys_as_optional_exit_accelerators() {
        let mut state = test_interaction_state();

        assert_eq!(
            handle_tui_input(&mut state, &[4]),
            TuiInputOutcome::Exit("ctrl-d")
        );
        assert_eq!(
            handle_tui_input(&mut state, &[b'/']),
            TuiInputOutcome::Continue
        );
        assert_eq!(state.composer, "/");
    }

    #[test]
    fn tui_exit_is_available_as_typed_slash_command() {
        let mut state = test_interaction_state();

        for byte in b"/exit" {
            assert_eq!(
                handle_tui_input(&mut state, &[*byte]),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Exit("slash-exit")
        );

        let mut state = test_interaction_state();
        for byte in b"/quit" {
            assert_eq!(
                handle_tui_input(&mut state, &[*byte]),
                TuiInputOutcome::Continue
            );
        }
        assert_eq!(
            handle_tui_input(&mut state, b"\n"),
            TuiInputOutcome::Exit("slash-exit")
        );
    }

    #[test]
    fn command_palette_keeps_selected_item_visible_when_selection_scrolls_past_page() {
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "stage coding".to_string(),
            transcript: Vec::new(),
            composer: "/".to_string(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 10,
            output_folded: false,
        };
        model
            .command_entries
            .extend((0..8).map(|index| TuiCommandCard {
                typed: format!("/extra-{index}"),
                action_id: format!("extra_{index}"),
                label: format!("Extra {index}"),
                gate: "extra".to_string(),
                category: "extra".to_string(),
                summary: "Extra command for palette scroll testing".to_string(),
            }));

        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );

        assert!(frame.contains(SELECTED_LINE_PREFIX));
        assert!(frame.contains("/deny"));
    }

    #[test]
    fn command_palette_wraps_to_top_instead_of_losing_selected_row_at_bottom() {
        let mut state = test_interaction_state();
        assert_eq!(
            handle_tui_input(&mut state, b"/"),
            TuiInputOutcome::Continue
        );
        let entry_count = matching_command_entries_for_state(&state).len();

        for _ in 0..entry_count {
            assert_eq!(
                handle_tui_input(&mut state, &[27, b'[', b'B']),
                TuiInputOutcome::Continue
            );
        }

        assert_eq!(state.overlay_selected, 0);
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "stage coding".to_string(),
            transcript: Vec::new(),
            composer: "/".to_string(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 999,
            output_folded: false,
        };
        apply_interaction_to_model(&mut model, &state);
        let frame = render_terminal_frame(
            &model,
            TerminalSize {
                cols: 100,
                rows: 30,
            },
        );
        assert!(frame.contains(&format!("{SELECTED_LINE_PREFIX}▌ /help")));
    }

    #[test]
    fn ratatui_command_palette_styles_entire_selected_row() {
        let mut model = TerminalFrameModel {
            title: "Astra Code full-screen TUI".to_string(),
            project_id: "proj_demo".to_string(),
            remote_state: "ready".to_string(),
            session_count: "1".to_string(),
            permission_count: "0".to_string(),
            permission_mode: "read-only".to_string(),
            model_label: "auto".to_string(),
            reasoning_effort: "auto".to_string(),
            working_dir: "/tmp/test".to_string(),
            git_branch: "main".to_string(),
            research_line: "stage coding".to_string(),
            transcript: Vec::new(),
            composer: "/".to_string(),
            key_hints: vec!["/ commands".to_string()],
            skill_entries: Vec::new(),
            command_entries: product_command_cards_for_test(),
            language: TuiLanguage::Zh,
            theme_mode: TuiThemeMode::Day,
            overlay_selected: 1,
            output_folded: false,
        };
        model.command_entries.insert(
            1,
            TuiCommandCard {
                typed: "/custom-selected".to_string(),
                action_id: "custom_selected".to_string(),
                label: "Custom".to_string(),
                gate: "test".to_string(),
                category: "conversation".to_string(),
                summary: "selected row summary follows highlight".to_string(),
            },
        );

        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        terminal
            .draw(|frame| render_ratatui_frame(frame, &model))
            .expect("frame should draw");
        let buffer = terminal.backend().buffer();
        let width = usize::from(buffer.area.width);
        let selected_row = buffer
            .content
            .chunks(width)
            .find(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .contains("selected row summary follows highlight")
            })
            .expect("selected summary should render");
        let selected_cell = selected_row
            .iter()
            .find(|cell| cell.symbol() == "s")
            .expect("selected summary cell should render");

        assert_eq!(selected_cell.bg, model.theme_mode.palette().panel_alt);
    }

    #[test]
    fn escape_sequences_do_not_decode_as_exit_keys() {
        assert_eq!(decode_exit_key(&[b'q']), None);
        assert_eq!(decode_exit_key(&[b'Q']), None);
        assert_eq!(decode_exit_key(&[3]), Some("ctrl-c"));
        assert_eq!(decode_exit_key(&[4]), Some("ctrl-d"));
        assert_eq!(decode_exit_key(&[27]), None);
        assert_eq!(decode_exit_key(&[27, b'[', b'A']), None);
        assert_eq!(decode_exit_key(&[27, b'[', b'M', 32, 40, 40]), None);
        assert_eq!(decode_exit_key(&[27, b'[', b'<', b'0', b';']), None);
    }

    #[test]
    fn exc_command_reports_idle_state_without_fake_staging() {
        let state = test_interaction_state();

        let response = route_typed_command(&state, "/exc");

        assert!(response.contains("当前没有运行中的回合"));
        assert!(response.contains("/exc"));
        assert!(!response.contains("命令已暂存"));
    }
}
