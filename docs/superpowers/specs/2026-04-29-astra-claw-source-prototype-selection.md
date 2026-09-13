# Astra Code Claw-Source Prototype Selection

Generated with the imagegen CLI fallback using `gpt-image-2`, based on the
Claw-source-grounded UX direction:

- code CLI conversation is the primary product surface,
- research appears as a compact first-screen brief,
- terminal, diffs, tests, approvals, sessions, memory, branches, artifacts,
  reviews, diagnostics, remote servers, and terminal leases stay folded behind
  palette actions, inline cards, overlays, drawers, or focused views.

Prompts are saved at:

- `docs/superpowers/specs/assets/claw-source-prototypes/prompts.jsonl`
- `docs/superpowers/specs/assets/claw-source-prototypes/mobile-retry-prompts.jsonl`

Contact sheets:

- `docs/superpowers/specs/assets/claw-source-prototypes/tui-contact-sheet.png`
- `docs/superpowers/specs/assets/claw-source-prototypes/web-contact-sheet.png`
- `docs/superpowers/specs/assets/claw-source-prototypes/mobile-contact-sheet.png`

## Generated Candidates

### TUI

- `astra-tui-claw-inline-repl.png`
- `astra-tui-claw-command-palette.png`
- `astra-tui-claw-tool-output.png`
- `astra-tui-claw-research-brief.png`

### Web

- `astra-web-chat-first-workbench.png`
- `astra-web-command-results-drawer.png`
- `astra-web-research-aware-code-session.png`
- `astra-web-remote-continuity.png`

### Mobile

- `astra-mobile-chat-first.png`
- `astra-mobile-inbox-approvals.png`
- `astra-mobile-work-terminal-sheet.png`

The fourth mobile fleet/server candidate repeatedly failed with upstream image
generation errors after the first 11 images completed. Keep the prompt in the
prompt archive and retry later if a fourth mobile candidate is needed.

## Recommended Reference Mix

### TUI Winner

Primary reference: `astra-tui-claw-inline-repl.png`.

Why:

- It most directly captures the Claw-style inline REPL default.
- Chat, status, research brief, and slash palette are all visible without
  becoming a pane dashboard.
- It shows terminal/tool/diff/session capabilities as folded cards and command
  rows rather than persistent first-level panes.

Use together with:

- `astra-tui-claw-tool-output.png` for inline structured tool cards, tests,
  diff summaries, and permission prompts.
- `astra-tui-claw-command-palette.png` for slash palette density and grouping.

Avoid copying:

- Any layout that turns research into a right-side permanent panel.
- Any bottom action grid that makes diff/test/result actions look like peer
  top-level slash commands instead of palette actions or inline cards.

### Web Winner

Primary reference: `astra-web-chat-first-workbench.png`.

Why:

- It keeps chat/composer dominant.
- The research brief is visible but small.
- Results, terminal, tools, diffs, tests, approvals, artifacts, and memory are
  folded into a lower drawer instead of becoming top-level dashboards.

Use together with:

- `astra-web-command-results-drawer.png` for a strong command/results drawer
  pattern and approval/diff cards.
- `astra-web-research-aware-code-session.png` for the research inspector
  treatment, but keep the inspector optional.
- `astra-web-remote-continuity.png` only for compact remote/session/lease
  affordances; do not copy its heavy persistent navigation.

Avoid copying:

- Large left rails that make the product feel like a generic admin dashboard.
- Right-side research panels that compete with the active coding thread.

### Mobile Winner

Primary reference: `astra-mobile-chat-first.png`.

Why:

- It feels like a code CLI chat companion, not a compressed desktop debug page.
- It shows the research brief as a compact card.
- It keeps terminal, diffs, tests, approvals, sessions, model, branches, and
  artifacts reachable as actions rather than first-level clutter.

Use together with:

- `astra-mobile-inbox-approvals.png` for approval, failed-test, review, research
  gate, and remote handoff inbox patterns.
- `astra-mobile-work-terminal-sheet.png` for the focused Work/terminal sheet,
  structured output cards, extra keys, tests, diffs, artifacts, and inline
  permission gate.

Still needed:

- A successful generated Fleet/Servers mobile candidate. The design target is
  clear even without the image: mobile Fleet should show multi-server,
  multi-terminal, lease owner, handoff/takeover, disabled controls without
  lease, and resume/switch actions while keeping Chat as the product default.

## Design Decisions To Carry Forward

- TUI defaults to inline REPL / chat-first.
- Full-screen split panes are advanced mode only.
- Research appears first-level only as a brief: thread, stage, evidence, gap,
  next action.
- Terminal output streams but collapses into structured cards.
- Permission prompts appear inline at the risky action.
- Palette entries must be registry-backed commands; natural-language rows are
  actions, not fake slash commands.
- Web uses drawers and inspectors, not a dashboard landing page.
- Mobile uses Chat, Inbox, Work, Context, Fleet/Servers, with Chat as default.
- Remote controls are lease-aware everywhere.
