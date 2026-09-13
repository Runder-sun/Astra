# Astra Remote Web & Mobile UX Redesign

Date: 2026-05-02

## Overview

Product-grade redesign of Astra Remote (web PWA + mobile). The current
implementation reads as a daemon debug dashboard: raw contract IDs, unlabeled
form fields, scroll-heavy right rail, no multi-terminal orchestration, and a
static research brief. This spec transforms it into a polished remote coding
agent control surface.

Design language inherits the TUI's **Refined Charcoal + Warm Amber** palette
and adds graphical elements appropriate for richer display surfaces.

## 1. Design System

### Color Palette (Dark theme — default)

| Token         | Value      | Usage                                |
|---------------|------------|--------------------------------------|
| surface       | `#0e0e12`  | Page background                      |
| panel         | `#14141a`  | Card/panel background                |
| panel_alt     | `#1c1c24`  | Selected row, hover background       |
| text          | `#c8c4bc`  | Primary text                         |
| muted         | `#6b6b78`  | Secondary text, labels               |
| faint         | `#4a4a56`  | Placeholder, hint                    |
| border        | `#222230`  | Card borders, dividers               |
| accent        | `#d4880a`  | Primary action, brand, interactive   |
| accent_soft   | `#d4880a22`| Selected background tint             |
| success       | `#6cb867`  | Pass, connected, complete            |
| danger        | `#e05c53`  | Fail, error, disconnected            |
| warning       | `#eab308`  | Warning, approval pending            |
| info          | `#38bdf8`  | Status, informational                |
| indigo        | `#818cf8`  | Code, research, diff accent          |

### Light theme (Day)

| Token         | Value      |
|---------------|------------|
| surface       | `#f4f4f8`  |
| panel         | `#ffffff`  |
| panel_alt     | `#eaeaee`  |
| text          | `#18181b`  |
| muted         | `#71717a`  |
| faint         | `#a1a1aa`  |
| border        | `#d4d4d8`  |
| accent        | `#b45309`  |
| success       | `#38874a`  |
| danger        | `#b14340`  |

### Typography

```
--sans: "Inter", "Aptos", "Segoe UI", system-ui, sans-serif;
--mono: "JetBrains Mono", "SF Mono", "Cascadia Code", ui-monospace, monospace;
```

- UI body: 14px / 1.45 line-height
- Panel headings: 15px / weight 760
- Eyebrow labels: 11px / weight 820 / uppercase
- Mono content (terminal, paths, code): 13px / 1.5 line-height

### Spacing & Radius

- Base grid: 4px
- Card padding: 16px
- Gap between sections: 12px
- Radius: 10px (cards), 8px (buttons, inputs), 6px (chips, tags)
- Elevation: borders preferred; shadows only for overlays (`0 8px 32px rgba(0,0,0,0.3)`)

## 2. Desktop Layout → Three-Zone Workbench

```
┌─────────────┬───────────────────────────────┬────────────────────┐
│  NAV RAIL   │        AGENT SURFACE          │    WORKBENCH       │
│             │                               │                    │
│ ◉ Astra     │  ┌─ Agent Chat ─────────────┐ │  [Approvals] [Art] │
│ ○ Server A  │  │                           │ │  [Tests] [Logs]    │
│ ● Server B  │  │  Structured timeline      │ │                    │
│ ○ Server C  │  │  with typed cards         │ │  Approval card     │
│             │  │                           │ │  ● pending         │
│ ── Sessions ─│  │  ● Tool · cargo test     │ │  Run mutation?     │
│ sess_abc    │  │  ◆ Code · src/tui.rs      │ │  [Approve] [Deny]  │
│ sess_def ← │  │  ◊ Diff · +12 -3          │ │                    │
│             │  │                           │ │  ── Tests ──       │
│ ── Research ─│  ├─ Research Progress ──────┤ │  21/21 passed     │
│ ◎ Scaling   │  │ ◎ Scaling Laws            │ │  ✓ read_line 0.02s│
│   Laws      │  │ survey ██████░░ 60%       │ │                    │
│ 60% survey  │  ├─ Terminal Stream ─────────┤ │  ── Artifacts ──   │
│             │  │ $ cargo test --lib        │ │  report.md         │
│             │  │ test result: ok           │ │  demo.diff         │
│             │  ├─ Composer ────────────────┤ │                    │
│             │  │ › describe the change…    │ │                    │
│             │  │ / cmds · $ skills · Send  │ │                    │
│             │  │ gpt-5.5 · high · clean    │ │                    │
└─────────────┴───────────────────────────────┴────────────────────┘
```

### Left Nav Rail (240px)

**Top section — Brand + status:**
```
◉ Astra Code
● Server B connected
model gpt-5.5 · reasoning high
```

**Server list (click to switch, dot indicates active):**
```
○ lab-workstation
● gpu-cluster-a
○ macbook-pro
```

Each server row shows: alias (or hostname) · connection state pill · active session count.

Click opens that server's agent surface and terminal stream. Multiple servers
can be viewed in the terminal mosaic (see §4).

**Session list:**
```
sess_abc  "Fix auth middleware"  2m ago
sess_def  "Refactor TUI"        active ←
```

Active session highlighted with accent border. Click switches session.

**Research context (collapsible):**
```
◎ Scaling Laws
  survey ██████░░ 60%
  evidence-gather
```

10-cell progress bar (same as TUI). Only shown when research is active.

### Center — Agent Surface

Four vertical zones in one scrollable column:

1. **Agent chat** — Structured timeline with typed cards (same icons/colors as TUI)
2. **Research progress** — Full pipeline bar (when active)
3. **Terminal stream** — xterm.js-style terminal for the active server
4. **Composer** — Input bar with `/` and `$` shortcuts, info bar

### Right Workbench (320px)

Tabbed interface, not a scroll stack:

- **Approvals** — Pending approval cards with risk indicator, approve/deny
- **Artifacts** — File list + preview pane
- **Tests** — Pass/fail summary + per-test detail
- **Logs** — Event log with filter

## 3. Mobile Layout → Five-Tab App

```
┌──────────────────────────────┐
│  ◉ Astra Code · gpu-cluster-a│  ← brand bar with server name
├──────────────────────────────┤
│                              │
│  Agent chat                  │  ← default tab
│  with structured cards       │
│                              │
│  ◎ Scaling Laws              │  ← research card in timeline
│  survey ██████░░ 60%         │
│                              │
├──────────────────────────────┤
│  › message or /command       │  ← sticky composer
│  / cmds  $ skills  ● Send   │
├──────────────────────────────┤
│ Chat │ Work │ Term │ Fleet   │  ← bottom tab bar
└──────────────────────────────┘
```

### Tabs

| Tab      | Content                                                |
|----------|--------------------------------------------------------|
| Chat     | Agent timeline, research progress, composer            |
| Work     | Approvals, artifacts, tests, results                   |
| Term     | Terminal stream + extra keys + resize                  |
| Fleet    | Server list, server health, terminal mosaic overview   |

Composer stays sticky above the tab bar on Chat tab.

### Fleet tab → Multi-terminal mosaic

```
┌──────────────────────────────┐
│  Fleet                       │
│  ● gpu-cluster-a (3 terms)   │
│  ○ lab-workstation (1 term)  │
│  ○ macbook-pro (idle)        │
├──────────────────────────────┤
│  ┌──────────┐ ┌──────────┐   │
│  │ gpu:0    │ │ gpu:1    │   │  ← tap to expand
│  │ $ cargo  │ │ $ python │   │
│  │ test...  │ │ train... │   │
│  └──────────┘ └──────────┘   │
│  ┌──────────────────────┐    │
│  │ gpu:2                │    │
│  │ $ astra tui launch   │    │
│  │ ...                  │    │
│  └──────────────────────┘    │
│                              │
│  [+ Attach to new terminal]  │
└──────────────────────────────┘
```

Each server shows a summary card with terminal count. Expanding shows a 2-column
grid of live terminal thumbnails. Tap a thumbnail to go full-screen terminal
(returns to Term tab with that server+terminal focused).

## 4. Multi-Terminal Mosaic (Desktop)

On desktop, the center terminal stream zone supports a mosaic layout when
multiple remote terminals are attached:

```
┌──────────────┬──────────────┐
│ gpu-a:0      │ gpu-a:1      │
│ $ cargo test │ $ cargo build│
│ ...ok        │ ...done      │
├──────────────┼──────────────┤
│ gpu-b:0      │              │
│ $ python     │  + attach    │
│ train.py     │  new terminal│
└──────────────┴──────────────┘
```

- Grid: 2×2 max on desktop (4 terminals), 2×1 on tablet (2 terminals)
- Each tile shows: server alias · terminal index · live stream
- Click tile to focus (enlarges, shows input/signal controls)
- `+` button to attach new terminal to any connected server
- Drag corner to resize tiles

## 5. Research Progress Visualization

### Chat card (inline)

```
┌─────────────────────────────────────┐
│ ◎ Research · Scaling Laws           │
│                                     │
│  idea      ██████████ 100%          │
│  refine    ██████████ 100%          │
│  experiment ░░░░░░░░░░   0% ← now   │
│  paper     ░░░░░░░░░░   0%          │
│  overall   ████████░░  40%          │
│                                     │
│  evidence: 3 papers · confidence:   │
│  stage: experiment-design           │
└─────────────────────────────────────┘
```

- Full pipeline view with per-stage progress bars
- Current stage highlighted with accent color and `← now` indicator
- Overall weighted progress bar
- Evidence count, confidence, current stage detail below bars
- Animated fill on progress updates (150ms ease-out)

### Desktop left rail (compact)

```
◎ Scaling Laws
  survey ██████░░ 60%
  evidence-gather
```

Same 10-cell bar as TUI, compact single-line.

### Mobile research card

Same as inline card but rendered as a scrollable section in the chat timeline.
Tapping the card opens a full-screen research detail view with:
- All stages with progress bars
- Evidence list with source + confidence
- Decision log
- `/research` command to modify pipeline

## 6. Structured Output Cards (Shared Web/Mobile)

Each output type uses the same icon + color scheme as the TUI:

| Type     | Icon | Color      | Card structure                                  |
|----------|------|------------|-------------------------------------------------|
| Tool     | `●`  | `#d4880a`  | Command → stdout area → summary (counts+time)   |
| Code     | `◆`  | `#818cf8`  | Language badge → file location → code → lines   |
| Error    | `✕`  | `#e05c53`  | Error type → message → location → fix hint      |
| Warning  | `△`  | `#eab308`  | Warning type → message → location → suggestion  |
| Status   | `○`  | `#38bdf8`  | Action description → metric summary             |
| Diff     | `◊`  | `#6cb867`  | File path → add/del stats → diff content → hunks|
| Test     | `✔`  | pass/fail  | Pass-rate → per-test list → failure details     |
| Research | `◎`  | `#818cf8`  | Topic → source → quote → confidence + citations |
| Text     | —    | —          | Plain markdown text                             |

Each card has a colored left border (2px) in its type color.
Cards can be collapsed (shows title row + line count + expand button).
Streaming cards show a pulsing border animation.

## 7. Composer & Command Palette

### Composer

```
┌────────────────────────────────────────────────────┐
│ › describe the change or task…                      │
└────────────────────────────────────────────────────┘
  / commands   $ skills   ● Send
  gpt-5.5 · high · ~/research-cli · main · clean
```

- Border: `border` when empty, `accent` when has content
- Ghost text hints in `faint` color
- Info bar below shows model · reasoning · directory · branch · workspace

### Command Palette (`/` trigger)

```
┌──────────────────────────────────────┐
│ ◉ Commands                           │
│   ↑↓ navigate · Enter apply · filter │
│ ┃ /help        → View help           │
│   /model       → Switch model        │
│   /terminal    → Attach terminal     │
│   /research    → Research pipeline   │
╰──────────────────────────────────────╯
```

- Overlays the composer area
- `┃` amber bar for selected row
- Fuzzy search filters as user types
- Skills shown in same palette with `◉ Skills` header when `$` is typed

## 8. Server & Terminal UX

### Server connection card

Instead of raw form fields, show:

```
┌──────────────────────────────────────┐
│ ◉ gpu-cluster-a                      │
│   100.64.0.23:18766                  │
│   ● connected · uptime 4h · 3 terms │
│                                      │
│   [Disconnect] [Reconnect] [Shell]   │
└──────────────────────────────────────┘
```

- No raw "Pair Ticket" or "Client ID" fields visible
- Pairing happens through a simple "Add Server" flow:
  1. Enter URL (or scan QR on mobile)
  2. One-tap pair (token auto-generated)
  3. Server appears in nav rail

### Terminal tile

```
┌──────────────────────────────────────┐
│ gpu-a:0 · bash · 80×24              │
│ ┌──────────────────────────────────┐ │
│ │ $ cargo test --lib               │ │
│ │ running 21 tests                 │ │
│ │ test read_line ... ok            │ │
│ │ ...                              │ │
│ └──────────────────────────────────┘ │
│ [Ctrl-C] [Esc] [Tab] [Enter] [Ctrl-D]│
└──────────────────────────────────────┘
```

- Terminal renders with monospace font, proper ANSI color support
- Extra keys bar below terminal
- Resize handle (desktop) or pinch-to-zoom (mobile)
- Input textarea with send button

## 9. Approval Cards

```
┌──────────────────────────────────────┐
│ △ Approval · pending                 │
│                                      │
│   Run mutation test on database?     │
│   Risk: medium                       │
│   Source: cargo test -- --mutate     │
│                                      │
│   [✓ Approve]          [✕ Deny]      │
└──────────────────────────────────────┘
```

- Warning icon + amber left border
- Risk indicator (low/medium/high) with color
- Source command shown in mono
- Two action buttons: Approve (success color) / Deny (danger color)
- Auto-appears in workbench and as inline chat card

## 10. Files to Modify

| File                      | Changes                                                    |
|---------------------------|------------------------------------------------------------|
| `src/assets/mobile/index.html` | Complete HTML restructure: nav rail, workbench tabs, mosaic, new card types |
| `src/assets/mobile/styles.css` | New design system, color tokens, card styles, mosaic grid, mobile tabs |
| `src/assets/mobile/app.js`     | Multi-server state, mosaic terminal management, research progress, new card rendering, redesigned command palette |
| `src/remote/daemon.rs`         | API endpoints for multi-terminal attach, research progress, server health aggregation |

## 11. Verification

```bash
# Visual
Open http://localhost:18766 in Chrome at 1440x900 (desktop) and 390x844 (mobile simulation)
Verify: no internal IDs visible, amber accent, structured cards, mosaic works

# Functional
Connect to 2+ servers simultaneously
Attach 3+ terminals and verify mosaic layout
Submit prompt and verify structured output cards
Trigger approval and verify card rendering
Start research pipeline and verify progress bars

# Build
cargo build --release --bin astra
cargo test --lib -- --nocapture
```

## 12. Acceptance Gate

The redesign is NOT acceptable if any of these are true:

- Any internal contract ID (`conversation_primary_surface`, `artifact_code_diff_pane`, `test_result_panel`) is visible in the UI
- Raw form fields for Pair Ticket / Client ID / Session ID are exposed in the primary interface
- Color scheme is still teal (`#0f766e`) instead of amber (`#d4880a`)
- Multiple servers cannot be connected simultaneously
- Terminal mosaic does not render for 2+ active terminals
- Research progress shows only static text (no visual progress bars)
- Approvals are buried in a scroll stack instead of being tab-first
- Mobile composer is not sticky above tab bar
- Any tab is unreachable at 390px width
