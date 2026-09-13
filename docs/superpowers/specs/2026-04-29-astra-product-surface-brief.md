# Astra Product Surface Brief for Prototype Generation

This brief is the source of truth for the second prototype generation pass. The goal is not to hand-layout screenshots. The goal is to give gpt-image-2 the complete product surface and ask it to explore high-quality UI arrangements for TUI, desktop web, and mobile app.

## Product

- **Astra Code:** agentic coding CLI/TUI for local and remote software engineering work.
- **Astra Remote:** web and mobile companion for monitoring, controlling, and resuming Astra Code sessions across machines.
- **Compatibility name:** `research-cli` may remain as binary/project identity during migration.

## Audience

- Engineers and research engineers running long agentic coding/research tasks.
- Users who switch between local terminal, browser dashboard, and phone while jobs continue.
- Users managing multiple servers, terminals, sessions, branches/worktrees, artifacts, approvals, and test results.

## Complete Product Surface

### Agent Conversation

- Main chat stream with user, assistant, tool call, tool result, terminal event, approval event, and review result turns.
- Natural language composer.
- `/` command entry and searchable command palette.
- Structured timeline that can collapse/expand raw details.
- Multi-session tabs and resume/inspect controls.
- Model/status/context display.

### Slash Commands

Initial commands:

- `/help`
- `/model`
- `/sessions`
- `/terminal`
- `/review`
- `/memory`
- `/servers`
- `/approvals`
- `/artifacts`
- `/tests`
- `/logs`
- `/diff`
- `/branch`
- `/worktree`
- `/permissions`

Each command needs title, short description, scope, control requirement, and target surface.

### Terminal

- Streaming terminal output.
- Raw PTY panel.
- Structured conversion of terminal output into cards.
- Terminal attach/bridge/send/resize/signal/replay.
- Extra keys: Ctrl-C, Esc, Tab, Enter, Ctrl-D.
- Multiple terminal support and switching.

### Structured Output

Cards for:

- Build steps.
- Test results.
- Diff summaries.
- Approval requests.
- Artifact previews.
- Command results.
- Tool calls and tool failures.
- Memory/context references.
- Branch/worktree status.
- Review gates.

### Approvals and Permissions

- Watch mode vs Control mode.
- Pending approval cards with command, target, risk, requester, and approve/deny actions.
- Permission history and recent decisions.
- Mutating actions visibly require control authority.

### Servers and Remote Control

- Multiple servers with alias, URL, health, active state, and reconnect/replay.
- Pairing flow with workspace, client ID, pair ticket, and control token.
- Server switcher available from every surface.
- Remote daemon health and transport status.

### Sessions, Branches, and Worktrees

- Session list and active session.
- Session resume/inspect.
- Branch/worktree status.
- Review and promotion gates.
- Long-running tasks that continue in background.

### Artifacts, Diffs, Tests, Logs

- Artifact family list and artifact preview.
- Code/diff viewer.
- Test/result panel with pass/fail counts and details.
- Event log remains available but is not primary UX.
- Copy/open/rerun actions.

### Memory and Context

- Memory/context panel.
- Project notes, facts, snippets, relevant context.
- Context budget/status.

### Mobile-Specific Requirements

- Chat-first default.
- Bottom navigation or equivalent page navigation.
- Pairing, servers, sessions, approvals, terminal, artifacts, tests, logs, and workbench must be reachable.
- Composer should support `/`, image attachments, voice/dictation, and send.
- Dense controls can use bottom sheets.
- Touch targets must be thumb-friendly.

### TUI-Specific Requirements

- Real interactive composer.
- `/` opens command palette.
- Keyboard-first navigation.
- Terminal-native, compact, readable.
- No fake advertised keybindings.
- Product labels instead of internal contract IDs.

### Web-Specific Requirements

- Desktop workbench supports multiple servers and terminals.
- Central agent conversation remains primary.
- Workbench makes approvals/artifacts/tests/logs discoverable without long debug-scroll.
- Browser page should feel like a polished developer tool, not a generic SaaS dashboard.

## Visual Direction

Ask the model to learn from the interaction quality and information density of:

- Claude Code CLI: terminal-first agent conversation, slash commands, skill/command discoverability, compact command UX.
- Codex CLI/TUI: local terminal coding agent, command output, review/test/diff workflows.
- Codex web/cloud/app: background coding tasks, worktrees/branches, skills/automations, permissions, multi-task continuity, mobile continuity.
- opencode and Claw Code: dense developer UX, command-driven operation, minimal ceremony.

Do not copy logos, exact UI, or product names from those references. Use them as product-quality and interaction references only.

## Palette

- Ink: `#101214`
- Warm paper: `#f7f5f0`
- Mist: `#eef2f3`
- Line gray: `#d7dde0`
- Teal action: `#0f766e`
- Amber approval: `#c0842d`
- Red danger: `#b84646`
- Optional cool accent: muted blue-gray or desaturated indigo, used sparingly.

## Negative Requirements

- Do not create a marketing hero.
- Do not create a generic debug dashboard.
- Do not leak internal contract IDs like `conversation_primary_surface`, `artifact_code_diff_pane`, or `test_result_panel`.
- Do not use decorative gradient blobs, stock imagery, fake customer logos, or emoji.
- Do not make unreadable microtext the main design.
- Do not hard-code a layout from this brief; choose the strongest layout for the product surface.
