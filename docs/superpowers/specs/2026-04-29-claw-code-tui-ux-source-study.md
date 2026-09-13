# Claw Code TUI / CLI UX Source Study

This note records source-grounded UX lessons from
`reference_repos/requested/claw-code`, so the Astra Code / research-cli redesign
does not drift into a speculative dashboard.

## Source Files Read

- `rust/TUI-ENHANCEMENT-PLAN.md`
- `rust/crates/rusty-claude-cli/src/main.rs`
- `rust/crates/rusty-claude-cli/src/input.rs`
- `rust/crates/rusty-claude-cli/src/render.rs`
- `rust/crates/commands/src/lib.rs`
- `rust/crates/rusty-claude-cli/tests/resume_slash_commands.rs`
- `rust/crates/rusty-claude-cli/tests/output_format_contract.rs`
- embedded `main.rs` unit tests around startup banner, reports, status, diff,
  permissions, completion, and resume behavior.

## What Claw Actually Is

Claw's mature default surface is an inline REPL, not a full-screen dashboard.
Its enhancement plan keeps inline REPL as the default and treats full-screen TUI
as an optional stretch mode.

The default loop is:

1. Print a startup banner with model, permission mode, git branch, workspace,
   directory, session id, and auto-save path.
2. Show workflow hints such as `/help`, `/status`, `/resume latest`, `/diff`,
   `/commit`, Tab completion, and Shift+Enter newline.
3. Read a prompt from a simple `> ` composer.
4. Parse slash commands locally when the line starts with `/`.
5. Otherwise run an agent turn, stream output, gate tools through permission
   prompts, persist the session, and return to the prompt.

This means Claw's product surface is not persistent panes. The product surface
is conversational code work plus extremely reachable commands.

## Mature UX Mechanics

### Composer

`input.rs` uses `rustyline` with slash command tab completion, history, Emacs
editing mode, Ctrl-J / Shift+Enter newline insertion, and different Ctrl-C
behavior depending on whether the input line is empty. This makes the composer
feel like a serious terminal-native input control rather than a raw stdin read.

### Startup And Status

`LiveCli::startup_banner` provides enough context to begin work without opening
any pane: model, permissions, branch, workspace, cwd, session id, session file,
and next commands. `/status` renders a structured report with model, usage,
workspace, git state, config files, memory files, session file, sandbox status,
and a suggested flow.

The lesson is to use compact context and on-demand status reports, not a
permanent system dashboard.

### Slash Commands

Claw has a command registry (`SLASH_COMMAND_SPECS`) and derives completion/help
from it. Commands are product verbs, not UI pane names. Some are implemented,
some are discoverability stubs, but they still live in one canonical registry.

Astra should therefore root visible slash entries in its real command registry:
`help`, `palette`, `slash`, `chat`, `prompt`, `model`, `resume`, `continue`,
`inspect`, `compact`, `sessions`, `projects`, `permissions`, `tools`,
`providers`, `config`, `usage`, `cost`, `doctor`, `mcp`, `skills`, `plugins`,
`hooks`, `memory`, `agents`, `branches`, `artifacts`, `repo`, `projectops`,
`host`, `tui`, `remote`, `research`, `reviews`, and `docs`.

Natural-language actions such as "Run tests", "Open diff", "Focus terminal",
"Assess claim", and "Plan experiment" should be palette actions or contextual
buttons, not invented top-level slash commands unless the registry grows them.

### Rendering

`render.rs` makes plain terminal output feel polished through Markdown parsing,
syntax-highlighted code fences, rendered tables, blockquotes, inline code,
colored headings, and a spinner. The important part is not decoration; it is
that assistant text and tool output remain readable in a scrolling terminal.

The enhancement plan prioritizes live Markdown rendering, collapsible tool
output, diff-aware edit display, colored diffs, pager support, and status HUD.
It explicitly keeps full-screen TUI as a later optional mode.

### Tool Output

Tool results are represented as structured transcript events with tool name,
short id, status, and a summarized payload. The enhancement plan calls for
collapsible output and diff-aware edit summaries because large terminal output
can destroy conversational readability.

Astra should show raw terminal streaming when needed, but the default transcript
should convert terminal/tool results into compact cards with expand/replay
affordances.

### Permissions

`CliPermissionPrompter` is simple but strong: it prints the tool, current mode,
required mode, reason, input, and asks for explicit approval. The UX lesson is
that permission prompts belong inline at the moment of risk, not as a permanent
first-level approvals pane.

### Sessions And Resume

Claw treats sessions as a first-class REPL capability: startup shows the current
session, `/resume` loads saved sessions, `/session` lists/switches/forks/deletes,
and tests prove resumed binaries can execute slash commands like `/status` and
`/diff`.

Astra should make session continuity obvious, but session management should
still be one command or overlay away rather than a top-level dashboard identity.

### Testability

Claw has tests for startup banner hints, report formatting, permission reports,
status contents, diff reports, JSON output contracts, and resumed slash command
behavior. This is a UX lesson: terminal UX needs contract tests for strings,
JSON surfaces, and command reachability.

## What This Means For Astra

The current `src/tui.rs` is a projection-first split-pane renderer. It exposes
conversation, input editor, status HUD, tool activity, permission overlay,
diffs, memory, branches, research DAG, and diagnostics as visible panes. That
is useful for kernel validation, but it should not be the target product UX.

The target TUI should be:

- Inline REPL / chat-first by default.
- Full-screen mode optional and conservative.
- Startup banner plus compact status line.
- Serious composer with history, slash completion, multiline input, and
  interrupt semantics.
- Research brief as a small first-screen addition to the Claw-style status
  context, not a research DAG pane.
- Tool/terminal output as transcript cards with collapse, summary, diff/test
  interpretation, and raw replay.
- Permissions as inline gates.
- Sessions, remote terminal lease, branches, memory, artifacts, reviews, and
  diagnostics reachable through the command palette and overlays.

The target web and mobile surfaces should follow the same hierarchy:
conversation first, command palette second, compact research context always
visible, and internal kernel surfaces folded into drawers or focused pages.
