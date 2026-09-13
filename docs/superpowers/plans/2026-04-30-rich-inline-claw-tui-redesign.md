# Rich Inline Claw-Style TUI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild Astra's default `astra` TUI as a Claw-Code-grade rich inline terminal UI: scrollback-native, not fullscreen by default, with mature input, visible slash/skill palettes, structured streaming output, permission cards, and research-aware status.

**Architecture:** Keep fullscreen ratatui as an explicit dashboard mode, but make the default interactive product a rich inline REPL. The rich inline layer owns terminal rendering and input ergonomics; Astra runtime remains authoritative for sessions, model/reasoning config, permission decisions, provider streaming, research metadata, and skills. Claw-Code source under `reference_repos/requested/claw-code/rust/crates/rusty-claude-cli/src` is the behavioral and quality reference.

**Tech Stack:** Rust, `rustyline`, `crossterm` for inline terminal redraw when needed, existing `pulldown-cmark`/`syntect` markdown rendering, existing Astra runtime callback types, PTY/manual smoke checks.

---

## Non-Negotiable Product Bar

- Default `astra` must be rich inline, not fullscreen and not bare text.
- `astra tui launch --fullscreen` may keep the existing fullscreen ratatui dashboard, but it is not the primary coding UX.
- `/` command and `$` skill palettes must be visible in the inline experience, with selected row, descriptions, grouped/product commands, and scroll behavior.
- Plain text must execute a real Astra prompt turn through the runtime. `/` and `$` must not accidentally become model prompts.
- Token streaming must visibly update. Final output must render as readable markdown/structured blocks, not debug logs.
- Permission approval appears as a conversation card only when a permission request exists; status bars may show permission mode but must not fake approval controls.
- Default language is Chinese; `/language zh|en` changes UI language. Do not dump Chinese and English simultaneously.
- Claw-Code comparison is mandatory before acceptance. If Astra is weaker in an implemented area, fix or explicitly record a blocker.

## Reference Files To Read

- `reference_repos/requested/claw-code/rust/crates/rusty-claude-cli/src/input.rs`
- `reference_repos/requested/claw-code/rust/crates/rusty-claude-cli/src/render.rs`
- `reference_repos/requested/claw-code/rust/crates/rusty-claude-cli/src/main.rs`
- Existing Astra files:
  - `src/tui.rs`
  - `src/tui_repl.rs`
  - `src/tui_markdown.rs`
  - `src/tui_output.rs`
  - `src/runtime/mod.rs`
  - `src/host_surface.rs`
  - `src/surface_commands.rs`

## Proposed File Boundaries

- `src/tui_repl.rs`: line editor, completion catalog, rich inline read outcomes, Claw-Code-compatible key behavior. No runtime.
- `src/tui_markdown.rs`: stream-safe markdown renderer. No runtime.
- `src/tui_output.rs`: typed/legacy output block classification, folding labels, permission/research/tool/status blocks.
- `src/tui.rs`: rich inline REPL orchestration, inline palette rendering, composer/status strip rendering, existing fullscreen ratatui dashboard.
- `src/runtime/mod.rs`: binds TUI actions to real runtime/session/permission/model/reasoning execution.
- `tests/operator/cli_surfaces.rs`: CLI-level behavior for launch mode, non-TTY fallback, and smokeable text contracts.

---

### Task 1: Claw-Code Gap Matrix And Acceptance Checklist

**Files:**
- Create: `docs/superpowers/reviews/2026-04-30-rich-inline-claw-gap-matrix.md`

- [ ] Read Claw-Code `input.rs`, `render.rs`, and the REPL/run-turn portions of `main.rs`.
- [ ] Compare against current Astra `src/tui.rs`, `src/tui_repl.rs`, `src/tui_markdown.rs`, `src/tui_output.rs`, and `src/runtime/mod.rs`.
- [ ] Produce a gap matrix with columns: `Area`, `Claw behavior`, `Current Astra behavior`, `Required fix`, `Acceptance test`.
- [ ] Include at least these areas: line editing, slash completion, visible command palette, skill invocation, multiline input, Ctrl-C/Esc semantics, non-TTY fallback, markdown streaming, tool blocks, permission prompts, prompt history, model/reasoning commands, session resume, research status, default launch mode.
- [ ] This task is read-only except for the review document.

**Verification:**
- The document must be specific enough that another worker can implement from it without reading the chat.

### Task 2: Rich Inline Palette And Composer

**Files:**
- Modify: `src/tui_repl.rs`
- Modify: `src/tui.rs`
- Test: `src/tui.rs` inline tests and `src/tui_repl.rs` tests

- [ ] Write failing tests that the default inline model can render a visible `/` command palette with selected row, description, and scroll window.
- [ ] Write failing tests that the default inline model can render a visible `$` skill palette with selected row, summary, and scroll window.
- [ ] Write failing tests for Claw-Code-like input semantics: normal text submits prompt, `/` routes command, `$` routes skill, multiline input is preserved, Ctrl-C/Esc clears or interrupts without being the only operation path.
- [ ] Implement an inline render model separate from fullscreen ratatui so rich inline can print/update a palette/composer without alternate screen.
- [ ] Reuse existing command/skill matching, localized labels, and palette scroll logic from `src/tui.rs`; do not duplicate a separate command registry.
- [ ] Ensure default Chinese copy is polished and compact; no debug terms in the first-level interface.

**Verification:**
- `cargo test --lib tui_repl tui::tests::typing_slash_renders_chinese_command_palette_overlay_by_default tui::tests::typing_dollar_renders_chinese_skill_palette_overlay_by_default -- --nocapture`
- Add or update targeted tests for rich inline palette rendering.

### Task 3: Rich Inline Streaming And Structured Blocks

**Files:**
- Modify: `src/tui_markdown.rs`
- Modify: `src/tui_output.rs`
- Modify: `src/tui.rs`
- Test: `src/tui_markdown.rs`, `src/tui_output.rs`, `src/tui.rs`

- [ ] Write failing tests that token deltas render incrementally and do not duplicate the final body.
- [ ] Write failing tests for structured inline blocks: markdown text, code fences, diff, tool status, permission card, research evidence/context.
- [ ] Implement Claw-Code-style inline output rendering with compact cards, muted metadata, orange/gray accent palette, and readable markdown.
- [ ] Add permission card rendering for real pending permission output. Do not show approval UI in static status unless a request exists.
- [ ] Add research-aware status strip output at startup and after relevant changes: topic/thread, stage, mode/confidence when useful.
- [ ] Keep non-TTY output plain and script-friendly; rich styling only when stdout is a terminal.

**Verification:**
- `cargo test --lib tui_markdown tui_output tui -- --nocapture`
- Manual smoke through a PTY or recorded non-TTY fallback for `/help`, `/reasoning`, `/sessions`, prompt execution, and cancellation.

### Task 4: Runtime Entry And Launch Mode Semantics

**Files:**
- Modify: `src/runtime/mod.rs`
- Modify: `src/host_surface.rs`
- Modify: `tests/operator/cli_surfaces.rs`
- Test: TUI launch conformance tests

- [ ] Write failing tests that interactive default `astra`/`astra tui launch` advertises and runs rich inline mode, not bare snapshot and not fullscreen.
- [ ] Keep `--fullscreen` explicitly mapped to `fullscreen_split_pane_projection_renderer`.
- [ ] Keep non-TTY fallback scriptable and deterministic, but do not let non-TTY fallback define the interactive product UI.
- [ ] Ensure `/model`, `/reasoning`, `/resume`, `/approve`, `/deny`, `/history`, `/sessions` remain bound to real runtime executors.
- [ ] Ensure `astra` installed from release enters the same default TUI as `astra tui launch` in an interactive shell.

**Verification:**
- `cargo test --test conformance host_surface_status_and_tui_projections_share_remote_workbench_contract -- --nocapture`
- `cargo test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture`
- Targeted operator CLI surface tests touching TUI launch.

### Task 5: Controller Final Acceptance

**Files:**
- No implementation ownership unless fixing review failures.

- [ ] Compare implementation against `docs/superpowers/reviews/2026-04-30-rich-inline-claw-gap-matrix.md`.
- [ ] Run full verification:
  - `cargo fmt --check`
  - `git diff --check`
  - `cargo test --lib -- --nocapture`
  - two conformance commands from Task 4
  - `cargo build --release --bin astra --bin research-cli`
- [ ] Run actual binary smoke:
  - `printf '/help\n' | target/release/astra tui launch`
  - `printf '/sessions\n' | target/release/astra tui launch`
  - interactive/PTY smoke for visible palette if possible.
- [ ] Install to `~/.local/bin/astra` and verify hash matches `target/release/astra`.
- [ ] Do not pass if visible `/` and `$` palettes, structured output, and streaming UX are not demonstrably better than the current bare inline state.
