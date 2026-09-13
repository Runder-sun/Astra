# Claw-Style Inline REPL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `astra` default TUI use a Claw-Code-style inline REPL with mature input, dynamic slash/skill completion, stream-safe output rendering, and the existing Astra runtime.

**Architecture:** Add a focused inline REPL backend beside the existing ratatui fullscreen backend. The new backend owns terminal line editing and output rendering, while prompt execution, model/reasoning config, sessions, skills, and provider calls continue to use Astra's existing runtime callbacks.

**Tech Stack:** Rust, `rustyline` for line editing, `pulldown-cmark` for Markdown parsing, `syntect` for code highlighting, existing `crossterm`, existing Astra runtime/TUI callback types.

---

### Task 1: Inline REPL Input And Completion

**Files:**
- Create: `src/tui_repl.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `THIRD_PARTY_NOTICES.md`

- [ ] Write tests for slash and skill completion candidates, duplicate filtering, and command argument completions.
- [ ] Verify tests fail before adding the module.
- [ ] Implement a Claw-Code-derived `LineEditor` wrapper using `rustyline`.
- [ ] Keep runtime execution out of this module.

### Task 2: Stream-Safe Markdown Renderer

**Files:**
- Create: `src/tui_markdown.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `THIRD_PARTY_NOTICES.md`

- [ ] Write tests for Markdown styling, fenced code handling, tables, and stream-safe boundaries.
- [ ] Verify tests fail before adding the module.
- [ ] Implement a compact renderer derived from Claw-Code's MIT Rust renderer.
- [ ] Keep the renderer usable by both inline REPL and future ratatui rendering.

### Task 3: Runtime Bridge For Inline REPL

**Files:**
- Modify: `src/tui.rs`
- Modify: `src/runtime/mod.rs`

- [ ] Write tests that default launch invokes the inline REPL path and `--fullscreen` keeps ratatui.
- [ ] Add `run_claw_style_inline_repl_with_config_executor` using existing `TuiStreamSender`, `TuiConfigAction`, and prompt executor callbacks.
- [ ] Route `/model` and `/reasoning` through the existing config executor.
- [ ] Preserve non-TTY snapshot behavior.

### Task 4: Verification

**Files:**
- Modify tests as needed near `src/tui.rs`, `src/tui_repl.rs`, `src/tui_markdown.rs`.

- [ ] Run targeted unit tests for new modules.
- [ ] Run `cargo test --lib tui -- --nocapture`.
- [ ] Run conformance tests touched by TUI launch.
- [ ] Run `cargo fmt --check`, `git diff --check`, and release build.
