# Astra Code / research-cli Balanced UX Design

## Diagnosis

The product should not be designed as a generic coding dashboard, and it should not be designed as a pure research notebook either.

The design docs and implementation point to a more precise product:

> A terminal-native code agent CLI whose core experience is conversational programming, extended by project memory, research workflow intelligence, multi-agent branch exploration, repo governance, and remote continuity.

The first screen must feel like a great code CLI in the Claw Code / Claude Code / Codex lineage: a focused chat transcript, a serious composer, a compact status line, and a command palette. The research-specific value should be visible on that first screen, but only as a compact research brief: current research intent, evidence confidence, open gap, and next recommended action. Internal runtime machinery should stay folded until the user asks for it.

## Claw Code Source Lessons

This redesign is grounded in the actual Claw Code source under `reference_repos/requested/claw-code`, not just in visual memory of Claude-like CLIs. The detailed source study is recorded in `docs/superpowers/specs/2026-04-29-claw-code-tui-ux-source-study.md`.

The key lesson is that Claw's mature terminal UX is an inline REPL first. It prints a startup banner with model, permission mode, branch, workspace, cwd, session, session file, and concise workflow hints; then it returns to a simple composer. Full-screen TUI is explicitly treated as an optional later mode, not the default product shape.

Claw's quality comes from product mechanics:

- `rustyline` composer with history, slash completion, multiline input, and interrupt semantics.
- Canonical slash command registry and completion surface.
- Streaming-first assistant output.
- Markdown, table, quote, and syntax-highlighted code rendering.
- Structured status, sandbox, model, permission, cost, session, resume, and diff reports.
- Inline permission prompts at the moment of risky tool execution.
- Session persistence and resume-tested slash command behavior.
- Formatting and command contracts tested without needing a live terminal.

For Astra, this means the first-level TUI must not be a projection debugger. The current projection-first TUI can remain a migration scaffold, but the target UX should translate `conversation`, `tool_activity`, `permission_overlay`, `diff_artifact`, `memory`, `branch_review`, `research_dag`, and `diagnostics` into Claw-like transcript events, command reports, overlays, and compact context. Only chat, composer, compact status, and compact research brief belong on the first screen.

## Product Positioning

**Astra Code** is the product-facing name for the interactive coding agent.

**research-cli** remains the compatibility and kernel identity: a project-memory, multi-agent, research-aware code CLI.

The user should understand it as:

- "I can code with it like Claude Code or Codex CLI."
- "It remembers project context more rigorously."
- "It can run research workflows when my coding task is part of a research project."
- "It can inspect evidence, experiments, branches, reviews, and artifacts without turning the main UI into a debug dashboard."
- "I can continue from terminal, browser, or phone."

## Product Layers

### Layer 1: Conversation

This is the primary UX and must dominate TUI, web, and mobile.

Core surfaces:

- Agent conversation.
- Composer.
- Slash command palette.
- One compact status line.
- One compact research brief.

The first screen should not show terminal, diffs, tests, sessions, artifacts, branches, memory, or remote state as separate panes. Those are available through commands, drawers, inspectors, or transient cards when they become relevant.

### Layer 2: Coding Tools

This layer contains the mature code CLI affordances, simplified behind the conversation.

Core surfaces:

- Terminal output and tool stream.
- Diffs and code changes.
- Tests and command results.
- Permission approvals.
- Sessions and resume.
- Model/provider/config status.
- Remote control and terminal lease.

These surfaces appear as inline event cards in the transcript, or as focused overlays after a command. They should not compete with the conversation on the default screen.

### Layer 3: Research Intelligence

This is the differentiator and should be visible earlier than other internal systems.

Core surfaces:

- Active research thread or question.
- Active stage.
- Evidence confidence.
- Open gap or blocking question.
- Next recommended action.
- HITL gate when research direction needs approval.

Research UI should integrate into the coding workflow as:

- a persistent but small first-screen research brief,
- context cards in the conversation,
- command palette scope under `/research`,
- expanded inspector only when requested.

Memory, artifacts, branch search, review traces, and paper outputs support this layer, but they should not be named as primary navigation objects on the first screen.

## Core User Flows

### Flow 1: Normal Coding Task

1. User opens `research-cli` or TUI.
2. CLI resolves workspace, session, provider, permission mode.
3. User asks for a code change.
4. Agent plans, edits, runs tools, streams terminal output.
5. UI summarizes diff/tests/approvals as inline structured cards only when they occur.
6. User reviews and continues.

Research stays visible as a short brief, even during normal coding, but it should read as context rather than a dashboard.

### Flow 2: Research-Aware Coding Task

1. User asks to implement or evaluate a method.
2. Agent links the task to a research thread or stage.
3. UI updates the compact research brief:
   - active question,
   - current stage,
   - evidence refs,
   - open gaps,
   - next recommended action.
4. Coding proceeds normally through tools, terminal, diffs, and tests.
5. Results update evidence/claim/artifact surfaces.

The main experience is still coding; research context explains why the coding matters.

### Flow 3: Branch / Multi-Agent Exploration

1. User invokes `/branches`, `/agents`, `/reviews`, or a scoped `/research` action from the command palette.
2. UI opens a branch/search inspector or task board over the conversation.
3. Candidate branches/agents are visible as alternatives, not as the main chat replacement.
4. Terminal/test/diff evidence determines which candidate is promotable.
5. User approves promotion or asks for another pass.

### Flow 4: Remote Continuity

1. User starts work in TUI.
2. Web or mobile pairs to the project.
3. Remote surfaces follow the same active session and project context.
4. Phone/web can approve, send a message, inspect results, switch servers/terminals, or resume a session when the control lease allows it.
5. Remote UI does not become a second runtime.

## Slash Command Model

The command palette is the product spine.

Commands should be grouped by user intent, but visible slash entries must stay close to the canonical command registry. Mature CLI actions such as "run tests" or "open diff" can be palette action labels, not invented top-level slash commands.

### Default Visible Slash Commands

Default visible entries should be real registry-backed product commands, not invented pane names. In the current research-cli registry, the first palette group should be:

- `/help` - discover commands and usage.
- `/palette` - open the command palette surface.
- `/slash` - inspect slash-command help.
- `/chat` - open the interactive chat lane.
- `/prompt` - run one bounded non-interactive prompt turn.
- `/model` - inspect or switch model.
- `/resume` and `/continue` - restore work.
- `/sessions` - browse, rename, delete, export, or inspect saved sessions.
- `/permissions` - pending approvals and permission history.
- `/doctor` - diagnose setup and project health.
- `/research` - current research context and research actions.

Secondary registry-backed commands remain searchable, but should not crowd the initial palette group:

- `/inspect`, `/compact`, `/projects`, `/tools`, `/providers`, `/config`, `/usage`, `/cost`, `/mcp`, `/skills`, `/plugins`, `/hooks`, `/memory`, `/agents`, `/branches`, `/artifacts`, `/repo`, `/projectops`, `/host`, `/tui`, `/remote`, `/reviews`, `/docs`.

### Palette Action Labels

These may appear as actions inside the palette or contextual cards, but should not be shown as canonical slash commands unless the registry adds them:

- Run tests.
- Open diff.
- Focus terminal.
- Approve request.
- Inspect memory.
- Check repo health.
- Plan experiment.
- Assess claim.
- Draft paper section.

### Project Intelligence Commands

- `/memory` - project memory query/explain/status.
- `/repo` - cleanup, canonicality, repo health.
- `/agents` - delegated agents and traces.
- `/branches` - branch exploration and promotion.
- `/artifacts` - inspect generated artifacts and canonical outputs.
- `/reviews` - code/research reviews.

### Research Scope

`/research` opens a scoped palette with actions such as:

- Show active thread.
- Show stage and open questions.
- Inspect evidence refs.
- Record decision.
- Classify current turn.
- Propose next stage.
- Plan experiment.
- Assess claim support.
- Prepare paper artifact.

Research actions should not crowd the default command list. They appear under `/research`, or as contextual suggestions when the current coding task is research-linked.

## TUI Design

### UX Goal

TUI should feel like the canonical product. It follows mature code CLI UX: the first-level screen is the transcript and composer, not a dashboard. Current implementation may remain projection-first during migration, but the target layout must avoid exposing internal projection names or subsystem panes as the product's main identity.

It should combine:

- Claude Code / Codex CLI style conversational coding.
- Claw Code's inline REPL discipline: startup context, simple composer, slash completion, structured reports, streaming output, and inline permission gates.
- A command palette that exposes research-cli's broader operator surface without cluttering the first screen.
- A small research brief that makes the project feel research-native.
- Coding internals folded behind inline cards, overlays, and palette actions.

### Current Implementation Gap

`src/tui.rs` currently renders an alternate-screen frame from a projection model and lists panes for agent chat, command input, workspace, terminal output, approvals, diffs, memory, branches, research plan, and diagnostics. `src/host_surface.rs` similarly treats many kernel concepts as pane plans. That is acceptable as a conformance scaffold, but it should be treated as internal scaffolding, not final product UX.

The target TUI should invert that model:

- Render a Claw-like startup banner and status line from the projection.
- Use the composer as the center of interaction.
- Show research as a compact brief, not `research_dag`.
- Convert terminal/tool output into inline transcript cards with expand/replay.
- Open permissions, sessions, diffs, memory, branches, artifacts, reviews, diagnostics, and remote lease state through palette actions or overlays.
- Keep full-screen split panes behind an explicit advanced mode.

### Recommended Layout

```text
+ Astra Code ----------------------------------------------------------------+
| research_cli  main  gpt-5.x  write | session latest | Research: UX evidence 4 |
+ Thread --------------------------------------------------------------------+
| user       redesign the remote UX                                          |
| assistant  I'll inspect the current surfaces and propose a plan.           |
|                                                                            |
| Research brief                                                             |
|   Stage: design review   Gap: mobile reachability   Next: revise palette    |
|                                                                            |
+ Composer ------------------------------------------------------------------+
| Ask Astra, or type / for commands _                                         |
| > /research  Current thread, evidence, gaps, next action                    |
|   /sessions  Resume or inspect sessions                                     |
|   /permissions Pending approvals                                            |
+ Keys ----------------------------------------------------------------------+
| enter send | / palette | ctrl-t terminal | ctrl-s sessions | esc close      |
+----------------------------------------------------------------------------+
```

### TUI Rules

- Chat/composer is always central.
- Default launch should be inline REPL / chat-first; full-screen split-pane mode is advanced.
- Startup should show model, permission mode, branch, workspace, cwd, session, and next commands in a concise banner.
- Composer should support command completion, history, multiline input, and clear interrupt semantics.
- Terminal, diffs, tests, approvals, sessions, memory, branches, and artifacts are not first-level panes.
- Terminal stream is one keystroke away and can occupy a focused overlay or replace the transcript temporarily.
- Diffs/tests/approvals appear as inline coding cards when events occur.
- Research context appears on the first screen as a compact brief, not a separate app.
- Internal surfaces (`memory_state`, `artifact_family`, `branch_batch`) are translated into user-facing labels.
- Default palette shows canonical commands first; research actions appear under `/research`.
- Tool output should stream, then collapse into readable cards with command, status, summary, duration, and expand/replay affordance.
- Permission prompts appear inline when a tool needs approval, showing tool, required mode, reason, input summary, and approve/deny actions.
- Status, diff, cost, sessions, permissions, and research reports should have text and JSON contracts that are testable without a terminal.

## Desktop Web Design

### UX Goal

Web is the expanded workbench for the same coding session. It should make inspection and remote continuity easier than the TUI, but the first screen should still feel like a chat-centered code CLI, not an operations dashboard.

### Recommended Information Architecture

- **Primary canvas:** active agent thread, composer, and compact research brief.
- **Collapsible command/results drawer:** terminal, diff, tests, approvals, artifacts.
- **Project switcher:** compact project/session/server control, not a persistent heavy rail by default.
- **Context inspector:** opens on demand for Research, Code, Permissions, Memory, Artifacts.

The inspector is contextual and may be pinned on wide screens. On first load, research summary is visible as a compact brief; detailed Code/Tests/Approvals/Memory/Artifacts stay collapsed until selected.

### Web Rules

- Do not make a generic dashboard landing page.
- Do not expose every kernel subsystem as a top-level tab.
- Keep the active conversation and composer visible on the first screen.
- Keep research stage/evidence/gap/next action visible on the first screen.
- Remote server/pairing is a navigator/fleet function, not the whole product.
- Research graph/evidence matrix can be expanded from the Research inspector, but stays secondary to the coding thread.

## Mobile App Design

### UX Goal

Mobile is a companion for continuing and supervising coding/research work, not a compressed desktop debug page. It should feel like a chat app for a code CLI with a research-aware status card.

### Recommended Navigation

- **Chat:** active conversation, composer, and research brief.
- **Inbox:** approvals, failed tests, review requests, research gates.
- **Work:** terminal, diffs, tests, artifacts for the active task.
- **Context:** session, memory, research thread, evidence.
- **Fleet:** servers, terminals, pairing, takeover/handoff, lease state.

### Mobile Rules

- Chat remains default.
- Approval and failed-run notifications go to Inbox.
- Research context appears as a compact card on Chat and expands in Context.
- Terminal is opened as a focused sheet/page.
- Server pairing is reachable, but not the product's main identity.
- Remote controls are lease-aware: show paired/unpaired, owner, handoff/takeover gate, and disabled terminal controls when there is no lease.
- Composer supports `/`, voice, images, and send.

## Visual Style

### Tone

- Developer-native.
- Quietly premium.
- Calm first level; dense only inside overlays and inspectors.
- More terminal/productivity tool than SaaS dashboard.
- Research-aware through semantic cards and evidence labels, not academic decoration.

### Palette

- Ink: `#101214`
- Surface dark: `#171a1d`
- Warm paper: `#f7f5f0`
- Mist: `#eef2f3`
- Line gray: `#d7dde0`
- Teal action: `#0f766e`
- Amber approval/evidence: `#c0842d`
- Red danger: `#b84646`
- Optional desaturated indigo for research context only.

### Component Language

- Command palette rows with command, title, scope, description.
- Tool/result cards with concise status and expand affordance.
- Evidence/research cards visually distinct but quiet.
- Approval cards with risk, command, target, approve/deny.
- Terminal panels with real monospace output.
- Status badges for project/session/model/permission/server.
- Research brief card with stage, evidence count/confidence, open gap, and next action.

## What To Avoid

- A research notebook as the primary UI.
- A kernel/debug dashboard as the primary UI.
- A generic remote terminal app.
- A generic SaaS admin console.
- Exposing internal state names as visible navigation.
- Making memory/branches/artifacts/sessions the first-level product identity.
- Hiding core workflows on mobile.
- Advertising keybindings that are not implemented.

## Prototype Generation Brief

When generating new gpt-image-2 prototypes, the prompt should say:

> Design a high-quality UI for Astra Code / research-cli, a terminal-native conversational coding agent in the Claw Code / Claude Code / Codex CLI lineage. The first-level interface should be mostly chat transcript, composer, compact status, and a small research brief. Do not expose terminal, diffs, tests, sessions, memory, branches, artifacts, remote servers, or repo governance as persistent first-level panes. Those capabilities should appear as inline cards, command palette actions, overlays, drawers, or contextual inspectors. The product differentiator is research-aware coding: show active research stage, evidence confidence, open gap, and next recommended action on the first screen without turning it into a research notebook. Use canonical command names where visible; action labels may be natural language. Do not show unregistered slash commands as primary commands.

Generate candidates in this structure:

- TUI candidates: explore chat-first transcript/composer, compact status, research brief, and command palette.
- Web candidates: expand the chat-first surface into an optional inspector/drawer model without making a dashboard.
- Mobile candidates: chat-first companion with research brief, Inbox, Work, Context, and lease-aware Fleet.

Selection criteria:

- Does it feel like a first-class code CLI?
- Is research intelligence visible on the first screen but not dominant?
- Are terminal, diff, test, approval, session workflows discoverable without being persistent first-level panes?
- Are memory/research/artifact/branch capabilities discoverable through palette/inspector/context?
- Is the layout implementable with current Rust TUI plus static web/mobile assets?
- Does it avoid debug-dashboard language?

## Generated Prototype References

The Claw-source-grounded prototype run is documented in `docs/superpowers/specs/2026-04-29-astra-claw-source-prototype-selection.md`.

Reference assets:

- TUI primary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-tui-claw-inline-repl.png`
- TUI secondary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-tui-claw-tool-output.png`
- Web primary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-web-chat-first-workbench.png`
- Web secondary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-web-command-results-drawer.png`
- Mobile primary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-mobile-chat-first.png`
- Mobile secondary: `docs/superpowers/specs/assets/claw-source-prototypes/astra-mobile-inbox-approvals.png`
- Mobile work surface: `docs/superpowers/specs/assets/claw-source-prototypes/astra-mobile-work-terminal-sheet.png`

Carry forward the combined direction: TUI uses `inline-repl` as the product skeleton and `tool-output` for event cards; Web uses `chat-first-workbench` with optional command/results drawer; Mobile uses `chat-first` as the default with Inbox and Work as focused companion tabs. A fourth mobile Fleet/Servers image failed due upstream image generation errors, but the target behavior remains lease-aware multi-server and multi-terminal control in a secondary tab.
