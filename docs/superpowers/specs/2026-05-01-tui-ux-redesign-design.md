# Astra TUI UX/UI Redesign

Date: 2026-05-01

## Overview

Full-spectrum redesign of the Astra terminal UI covering visual polish, interaction
experience, information architecture, agent response aesthetics, and structured output
differentiation. The design language is **Refined Charcoal + Warm Amber**: a cold gray
base with warm amber accents for a professional yet approachable feel.

## 1. Color Palette

### Night theme (default)

| Role       | Current         | New             | Rationale                        |
|------------|-----------------|-----------------|----------------------------------|
| surface    | `#0e0d0c`       | `#0e0e12`       | Cool gray-blue, remove warm tint |
| panel      | `#161512`       | `#14141a`       | Cool gray sync                   |
| panel_alt  | `#1f1d1a`       | `#1c1c24`       | Selected row background          |
| text       | `#eeeae2`       | `#c8c4bc`       | Reduce brightness, less eye strain |
| muted      | `#a9a094`       | `#6b6b78`       | Cool gray label color            |
| faint      | `#6c655d`       | `#4a4a56`       | Placeholder/hint color           |
| border     | `#413b33`       | `#222230`       | Thinner, more restrained         |
| accent     | `#ef8838`       | `#d4880a`       | Amber slightly desaturated       |
| success    | `#6cb867`       | `#6cb867`       | Keep                             |
| danger     | `#e65c53`       | `#e05c53`       | Slight adjustment                |

### Day theme

| Role       | New             |
|------------|-----------------|
| surface    | `#f4f4f8`       |
| panel      | `#ffffff`       |
| panel_alt  | `#eaeaee`       |
| text       | `#18181b`       |
| muted      | `#71717a`       |
| faint      | `#a1a1aa`       |
| border     | `#d4d4d8`       |
| accent     | `#b45309`       |
| success    | `#38874a`       |
| danger     | `#b14340`       |

### Semantic output colors (shared across themes)

| Type     | Icon | Icon color    | Border color     |
|----------|------|---------------|------------------|
| Tool     | `●`  | `#d4880a`     | `#d4880a33`      |
| Code     | `◆`  | `#818cf8`     | `#818cf822`      |
| Error    | `✕`  | `#e05c53`     | `#e05c5322`      |
| Warning  | `△`  | `#eab308`     | `#eab30822`      |
| Status   | `○`  | `#38bdf8`     | `#38bdf822`      |
| Diff     | `◊`  | `#6cb867`     | `#6cb86722`      |
| Test     | `✔`  | green/red     | same as status   |
| Research | `◎`  | `#818cf8`     | `#818cf822`      |
| Text     | none | none          | none             |

## 2. Startup Banner → Compact Card

Current: ~14 lines (6-line ASCII art + 7 info lines + 1 help line).

New: ~5 lines.

```
╭─────────────────────────────────────╮
│ ◉ Astra Code                        │
│ gpt-5.5 · default · main · clean    │
│ /help 命令 · $list 技能 · /exit 退出 │
╰─────────────────────────────────────╯
```

- Remove ASCII art logo, use `◉` brand symbol in accent color.
- Key info on one line: model · permission · branch · workspace.
- Third line: shortcut hints.
- Rounded `╭╮╰╯` border in accent color.

## 3. Input Area → Pure Input + Info Bar Below

### Input box (pure input)

```
╭──────────────────────────────────────────────────╮
│ › describe the change or task…  / 命令  Enter 发送 │
╰──────────────────────────────────────────────────╯
```

- Only `›` prompt + input content.
- `/ commands`, `$ skills`, `Enter send` displayed as ghost-text hints in faint color
  after the cursor position. Disappear once user starts typing.
- Border: gray when empty, amber when input is present.

### Info bar (below input box, no border between)

```
 gpt-5.5 · high · ~/research-cli · main · clean
 ◎ Attention Scaling Laws
   lit-review ████████░░░░░░ 58% · evidence-gather
```

**Line 1 — Environment context (always shown):**
`model · reasoning_effort · directory · branch · workspace_status`

**Lines 2-3 — Research context (only when active research exists):**
- `◎` + research topic name
- Progress bar + percentage + current stage name

When no research is active, only line 1 is shown (1-line info bar).

## 4. Research Progress Bar

### Three display levels

| Level       | Trigger               | Display                                        |
|-------------|-----------------------|------------------------------------------------|
| Hidden      | No active research    | Research area not rendered                     |
| Stage       | Active research       | Current stage progress bar + stage name + %    |
| Sub-task    | Executing an operation| Stage bar + indented sub-task bar              |
| Overview    | `/status` command     | All stages + overall progress                  |

### Progress estimation

- **Sub-task**: exact when countable (e.g. "2/3 papers searched" = 67%).
- **Stage**: weighted average of completed sub-tasks, or time-based heuristic.
- **Overall**: weighted by pipeline position; completing early stages advances later
  stages proportionally.

### Progress bar style

- 10-cell bar: `████░░░░░░`
- Fill color: accent `#d4880a`
- Empty color: border `#222230`
- Percentage: accent color
- Stage name: muted `#6b6b78`

### Sub-task example

```
 lit-review   ████████░░░░░░ 58% · evidence-gather
              └─ search ████████ 100% · 3/3 papers
```

### Overview example (`/status`)

```
 ◎ Attention Scaling Laws
 idea        ██████████ 100%
 refine      ██████████ 100%
 experiment  ░░░░░░░░░░   0% ← current
 paper       ░░░░░░░░░░   0%
 overall     ████████░░  40%
```

## 5. Thinking Status → Progress Bar + Timer

Current: `⠹ Astra 正在思考 · 任务 xxx · Esc 或 /exc`

New:

```
⠹ Astra · thinking  ██░░░░░░░░  3s  ·  Esc /exc
```

- Amber spinner + brand name.
- 10-cell mini progress bar in amber.
- Elapsed time in seconds.
- Interrupt hint in faint on the right.

**Progress animation**: when no real progress is available, use a breathing/pulsing
pattern (fill oscillates between 2-5 cells slowly). When streaming tokens, estimate
progress from token count.

## 6. Agent Response → Structured Output by Type

Each output type has a unique structural layout, not just a different color.

### Tool execution

```
╭──────────────────────────────────────╮
│ ● Tool · passed                      │
│                                      │
│   $ cargo test --lib                 │  ← amber command line
│                                      │
│   Running 12 tests                   │  ← stdout area
│   test read_line ... ok              │
│   test completion ... ok             │
│                                      │
│   ✓ 12 passed · 0 failed · 1.3s     │  ← summary: result + duration
╰──────────────────────────────────────╯
```

Structure: command line → stdout/stderr → summary (counts + duration).

### Code block

```
╭──────────────────────────────────────╮
│ ◆ Code · rust                        │
│   📄 src/tui.rs · L121-L135          │  ← file path + line range
│                                      │
│   fn render_status(state: &State) {  │  ← syntax-highlighted code
│       let frame = spinner();         │
│       format!("{frame} thinking")    │
│   }                                  │
│                                      │
│   4 lines · /copy                    │  ← line count + action hint
╰──────────────────────────────────────╯
```

Structure: language badge → file location → syntax-highlighted code → line count + actions.

### Diff

```
╭──────────────────────────────────────╮
│ ◊ Diff · src/tui.rs                  │
│   +12 · -3                           │  ← add/delete stats
│                                      │
│   - fn old_spinner() -> String {     │  ← red deletion lines
│   + fn render_status(                │  ← green addition lines
│   +     state: &State,               │
│   + ) -> String {                    │
│                                      │
│   3 hunks · /expand                  │  ← hunk count + expand
╰──────────────────────────────────────╯
```

Structure: file path → add/delete stats → diff content → hunk summary.

### Error

```
╭──────────────────────────────────────╮
│ ✕ Error · failed                     │
│                                      │
│   type mismatch                      │  ← error type (red bold)
│   expected `String`, found `&str`    │  ← error message
│                                      │
│   src/tui.rs:142:18                  │  ← file location (indigo)
│                                      │
│   hint: use .to_string()             │  ← fix suggestion (cyan)
│   → value.to_string()                │
╰──────────────────────────────────────╯
```

Structure: error type → message → location → fix suggestion.

### Test results

```
╭──────────────────────────────────────╮
│ ✔ Test · 12/12 passed                │  ← pass/total summary
│                                      │
│   ✓ read_line          0.02s         │  ← per-test: status + name + time
│   ✓ completion         0.01s         │
│   ✕ edge_case          0.03s         │  ← failure in red
│     Expected "ok" got "err"          │  ← failure detail indented
│                                      │
│   12 total · 3.2s                    │  ← summary
╰──────────────────────────────────────╯
```

Structure: pass-rate summary → per-test results → failure details → totals.

### Warning

```
╭──────────────────────────────────────╮
│ △ Warning · warn                     │
│                                      │
│   unused variable                    │  ← warning type
│   `x` is defined but never read      │  ← message
│                                      │
│   src/tui.rs:88:12                   │  ← location
│   hint: prefix with _                │  ← suggestion
╰──────────────────────────────────────╯
```

### Status

```
╭──────────────────────────────────────╮
│ ○ Status · info                      │
│                                      │
│   Build completed                    │  ← action description
│   142 files · 3 warnings · 2.3s     │  ← metric summary
╰──────────────────────────────────────╯
```

### Research evidence

```
╭──────────────────────────────────────╮
│ ◎ Research · evidence                │
│                                      │
│   Transformer scaling laws           │  ← topic
│   arxiv:2301.01234                   │  ← source
│                                      │
│   "Performance scales predictably    │  ← quote (italic muted)
│    with model size and data."        │
│                                      │
│   confidence: high · 3 citations     │  ← confidence + citations
╰──────────────────────────────────────╯
```

### Folded state (all types)

```
╭──────────────────────────────────────╮
│ ● Tool · passed · 47 lines folded    │
│   cargo test --lib · /expand         │
╰──────────────────────────────────────╯
```

First line: icon + type + status + fold count.
Second line: title/summary + expand action.

## 7. Command/Skill Dropdown → Unified Amber Style

Current: `╭─ 命令 ↑↓ 选择…` + gray list.

New:

```
╭──────────────────────────────────────╮
│ ◉ 命令  ↑↓ 选择 · Enter 采用 · 输入过滤 │
│ ┃ /help                              │  ← selected: amber bar + bg
│   /model                             │
│   /reasoning                         │
│   /language                          │
╰──────────────────────────────────────╯
```

- Title row: amber `◉` + type name (命令/技能) + navigation hints.
- Selected row: left `┃` amber vertical bar + amber tinted background.
- Unselected rows: aligned indent, muted text.
- Commands and skills share the same visual style; only title text differs.

## 8. Help Panel → Categorized Groups

Current: flat text list.

New:

```
◉ Commands

  Core
    /help     → 查看帮助
    /model    → 切换模型
    /exit     → 退出

  Session
    /resume   → 恢复会话
    /history  → 查看历史

  Skills: $list · $skill-name · $skill-name <input>
```

Commands grouped by category with category headers. Each command shows typed form + summary.

## Files to Modify

| File                  | Changes                                                |
|-----------------------|--------------------------------------------------------|
| `src/tui.rs`          | Palette, banner, input area, info bar, spinner, help, dropdown, response rendering, progress bar logic |
| `src/tui_repl.rs`     | Dropdown hint rendering, prompt string                 |
| `src/tui_markdown.rs` | Code block rendering with file location, line count    |
| `src/tui_output.rs`   | Block classification, fold state, per-type formatting  |

## Verification

```bash
cargo fmt --check
git diff --check
cargo test --lib tui_repl tui_markdown tui_output tui -- --nocapture
```

For release confidence:

```bash
cargo test --lib -- --nocapture
cargo build --release --bin astra --bin research-cli
```