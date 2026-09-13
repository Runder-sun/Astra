# Astra Code Product UX/UI Design Spec

## Purpose

Astra Code should feel like a serious agentic coding environment rather than a daemon debug page. The primary product shape is an agent conversation with first-class slash commands, terminal streaming, structured tool output, approvals, artifacts, tests, and multi-server/session switching.

This spec is based on the code review of the current redesign branch and should guide the next implementation pass for TUI, remote web, and mobile app.

## Product Names

- **Astra Code:** Core CLI/TUI product and agent identity.
- **Astra Remote:** Installable web/mobile remote surface for monitoring and controlling Astra Code sessions.
- **research-cli:** Existing binary name can remain for compatibility until a separate rename/migration plan exists.

## UX Principles

- **Agent-first:** Chat/composer is the primary surface on every device.
- **Command-native:** `/` is the universal entry point for actions, navigation, and workflow discovery.
- **Terminal-compatible, not terminal-only:** Raw terminal output is always available, but repeated patterns should become structured cards.
- **No hidden critical workflows:** Pairing, sessions, approvals, tests, artifacts, logs, and terminal control must be reachable on desktop and mobile.
- **Contract IDs are not UI text:** Internal names stay in `data-*`, tests, and API payloads. Visible copy uses product language.
- **Watch vs Control is explicit:** Read-only browsing and mutating actions must be visually distinct.

## Reference Products

Use these as behavioral references, not as skins to copy:

- Claude Code: terminal-first agent conversation, compact command surface, natural language plus explicit tools.
- Codex CLI: streaming terminal output, clear operation boundaries, command affordances.
- opencode / Claw Code: dense developer layout, fast command interaction, low ceremony.
- Claude/Codex web and mobile surfaces: chat-first continuity, structured tool/result cards, mobile composer ergonomics.

## Selected Visual Direction

Generated prototype references live in `docs/superpowers/specs/assets/`.

| Prototype | File | Role | Decision |
| --- | --- | --- | --- |
| TUI terminal | `astra-prototype-01-tui-terminal.png` | Terminal-first TUI layout | Use as the TUI density and keyboard-command reference. It has the right terminal seriousness and clear approval/diff/test side rail. |
| Web workbench | `astra-prototype-02-web-workbench.png` | Desktop remote layout | Use only for desktop information architecture. The workflow grouping is useful, but the visual tone is too enterprise-web and should be darkened/tightened for Astra. |
| Mobile chat | `astra-prototype-03-mobile-chat.png` | Phone app layout | Use as the mobile reference. It correctly shows chat-first navigation, server reachability, bottom sheet pairing, and thumb-friendly actions. |
| Structured output | `astra-prototype-04-structured-output.png` | Semantic cards and command palette | **Primary selected reference.** It best captures the desired product: agent chat plus terminal stream, searchable `/` palette, and structured output cards. |

The selected direction is prototype 04, with prototype 03 as the mobile-specific companion and prototype 01 as the TUI-specific companion. Prototype 02 should not drive visual styling, but it validates that the desktop web product needs a left server/session rail, center agent surface, and right workbench tabs.

### Prototype Selection Rationale

- **Prototype 04 wins** because it directly fixes the largest UX gaps from review: `/` commands become a real palette, raw terminal output coexists with structured cards, and approvals/tests/diffs/artifacts are product surfaces instead of debug panels.
- **Prototype 03 is the mobile target** because it proves the phone app should use page-level navigation plus bottom sheets, not panel-level hiding. Servers and pairing are reachable without leaving chat context.
- **Prototype 01 is the TUI target** because it preserves a CLI-native mental model while adding a command list, composer, side status, approvals, diffs, tests, and terminal stream.
- **Prototype 02 is a secondary layout reference** only. Its light enterprise SaaS styling is readable, but Astra should feel closer to a polished developer agent environment than a generic infrastructure dashboard.

## Design System

- **Color palette:** Near-black ink `#101214`, warm paper `#f7f5f0`, mist `#eef2f3`, line gray `#d7dde0`, teal action `#0f766e`, amber approval `#c0842d`, red danger `#b84646`.
- **Typography:** Use the existing system font stack for UI; use the existing monospace stack for terminal, command, paths, diffs, and logs. Do not scale font size by viewport width.
- **Spacing:** 4px base grid. Compact controls use 8px gaps; panels use 12-16px internal padding; major desktop columns use 14-18px gutters.
- **Radius:** 6-8px for cards and panels; 4-6px for dense command chips; avoid oversized pill-shaped containers except status badges.
- **Elevation:** Prefer borders and subtle background bands. Use shadows only for overlays, command palette, mobile sheets, and floating nav.
- **Motion:** 120-180ms ease-out for tab/page transitions and palette open/close; respect `prefers-reduced-motion`.
- **Tone:** Operational, compact, exact. Avoid marketing copy, decorative gradients, or explanatory onboarding text inside the app.

## Information Architecture

### Shared Concepts

- **Conversation:** Agent messages, tool calls, results, and structured cards.
- **Composer:** Natural language input, `/` command entry, attachments, voice on mobile/web.
- **Command palette:** Searchable command registry with scope, description, and execution mode.
- **Terminal stream:** Raw PTY output, extra keys, resize, signal, replay.
- **Workbench:** Approvals, reviews, artifacts, branches, tests, logs.
- **Server directory:** Multiple servers, active server, health, reconnect/replay.
- **Sessions:** Session tabs/list, resume/inspect, ownership/lease state.

### Command Registry

Every command should have:

- `command`: `/terminal`
- `title`: `Terminal`
- `description`: `Attach, send input, resize, or signal the governed terminal`
- `scope`: `global | chat | workspace | terminal | code | context`
- `requires_control`: boolean
- `target_surface`: `chat | terminal | workbench | artifacts | servers`

Initial commands:

- `/help`
- `/palette`
- `/slash`
- `/chat`
- `/prompt`
- `/model`
- `/sessions`
- `/permissions`
- `/terminal`
- `/terminal replay`
- `/memory`
- `/artifacts`
- `/research`
- `/approve <request-id>`
- `/deny <request-id>`
- `/interrupt`

Natural language work actions such as Run tests, Open diff, Focus terminal,
Check repo health, Inspect memory, and Assess claim may appear inside the
palette or as inline cards, but they should not be rendered as top-level slash
commands unless the command registry adds them.

## TUI Design

### Layout

TUI should be a real interactive surface:

```text
+ Astra Code ---------------------------------------------------------------+
| project research_cli | server local | session active | Watch | model ... |
+ Chat --------------------------------------------------------------------+
| assistant  I inspected the branch...                                      |
| tool       cargo test remote_daemon:: ...                                 |
| result     21 passed                                                      |
+ Structured --------------------------------------------------------------+
| Approval required | Run mutation? | Approve / Deny                        |
| Diff summary      | 3 files changed                                       |
+ Composer ----------------------------------------------------------------+
| /research current branch evidence _                                       |
| /help /palette /sessions /terminal /permissions /memory                    |
+ Keys --------------------------------------------------------------------+
| enter send | / commands | ctrl-s sessions | ctrl-t terminal | ctrl-c stop |
+-------------------------------------------------------------------------+
```

### Required Interactions

- Printable text edits the composer.
- `/` opens command palette when typed at command position.
- Arrow keys navigate command palette and history.
- Enter submits selected command or message.
- Escape closes palette/modal.
- `ctrl-s` opens session selector.
- `ctrl-t` focuses terminal stream.
- Approval prompts render as modal/inline cards with approve/deny.

### TUI Anti-Goals

- Do not only redraw static projections.
- Do not advertise keybindings that do not work.
- Do not show internal pane IDs unless debugging mode is explicitly enabled.

## Desktop Web Design

### Layout

Desktop keeps a three-zone architecture:

- **Left rail:** Brand, server switcher, pairing, workspace status, sessions.
- **Center:** Agent chat, structured timeline, slash composer, terminal stream.
- **Right workbench:** Tabs for Approvals, Artifacts, Tests, Logs.

### Key Changes from Current Branch

- Convert right rail from a long scroll stack into tabs.
- Keep approvals visible as a first-class workflow.
- Replace `R` refresh label with a recognizable icon/text affordance.
- Replace native file input surface with a polished attachment button and file preview list.
- Map all visible status names through product labels.

## Mobile App Design

### Mobile Navigation

Bottom tabs:

- **Chat:** Agent conversation, semantic cards, composer.
- **Servers:** Server directory, pairing, workspace status, watch/control.
- **Terminal:** Terminal stream, extra keys, resize/signal.
- **Workbench:** Approvals, sessions, message, tests, logs.
- **Artifacts:** Code diffs, artifact list, result previews.

Each tab is a page, not a single panel. Critical workflows must not be hidden by CSS.

### Mobile Interaction Pattern

- Chat remains the default landing page.
- Servers and Workbench can open dense controls in bottom sheets.
- Composer is sticky above the bottom nav.
- `/` opens a full-screen command palette or bottom sheet depending on keyboard state.
- Approval cards are thumb-friendly and show risk before action.

## Structured Output Design

Raw tool output should be transformed into cards when possible:

- **Tool call card:** command/tool name, args summary, status.
- **Terminal output card:** collapsed raw stream with copy/open controls.
- **Test result card:** pass/fail counts, failing names, rerun action.
- **Diff card:** files changed, insert/delete summary, open artifact.
- **Approval card:** requested mutation, risk, approve/deny.
- **Artifact card:** path/family, syntax, preview.

Raw logs remain accessible, but they are not the primary reading mode.

## Accessibility and Quality Gates

- Every bottom tab must expose one visible heading and one primary control at mobile width.
- Focus rings must be visible on command palette rows, buttons, tabs, and composer.
- Touch targets should be at least 40px high on mobile.
- Text must not overlap or clip at 390px width.
- Internal contract IDs are banned from visible copy.
- Screenshots required before merge:
  - Desktop web `1440x900`
  - Tablet/narrow desktop `1024x768`
  - Mobile chat `390x844`
  - Mobile workbench `390x844`
  - TUI command palette in a real terminal

## Acceptance Criteria

- TUI accepts typed input and opens a `/` command palette.
- Web and mobile share one command registry.
- Mobile pairing, status, sessions, approvals, tests, artifacts, logs, terminal, and chat are all reachable.
- No visible `conversation_primary_surface`, `artifact_code_diff_pane`, or `test_result_panel`.
- Targeted conformance tests pass.
- Screenshots show no overlap, clipped primary labels, or debug-page styling.
