---
doc_frame:
  doc_id: docs.deep_study.52_ultron_verifier_tailscale_reference_audit
  schema_version: "1"
  source_path: docs/deep_study/52-ultron-verifier-tailscale-reference-audit.md
  title: Ultron, Verifier, and Tailscale Remote Reference Audit
  doc_type: reference_audit
  lifecycle: active
  scope: cross_milestone
  milestone: M5-M11+
  summary: Source-level audit of ULTRON, LLM-as-a-Verifier, and Tailscale/mobile remote-control projects, with concrete adoption decisions for research-cli.
  key_claims:
    - The intended ULTRON reference is ModelScope `modelscope/ultron`, not the earlier mistakenly pulled GitHub Telegram bridge. ModelScope ULTRON is a collective-intelligence substrate for shared tiered memories, self-evolving skills, shared harness blueprints, and trajectory segmentation/metrics; research-cli excludes supervised-training export.
    - LLM-as-a-Verifier gives a high-value upgrade path for M9 branch winner selection and M10 research decisions: criterion decomposition, repeated pairwise verification, logprob/score-token scoring when available, caching, and tournament selection.
    - HAPI, Farfield, CodexMonitor, codexui, itwillsync, TerminalSync, Claude Code Viewer, and related projects show that mobile control should be split into app-server semantic control and PTY terminal parity, both transported over Tailscale/private overlay and both governed by research-cli authority.
  decisions:
    - Treat ModelScope ULTRON as the reference for fleet-level memory/skill/harness/trajectory evolution, not as evidence for an in-process parallel subagent scheduler.
    - Add post-M11 proof gates for memory-to-skill crystallization/re-crystallization, provenance-grounded skill verification, structure-score upgrade gating, shareable allowlisted harness profiles, and trajectory segmentation/quality metrics before claiming full superiority over ModelScope ULTRON.
    - Adopt verifier-tournament gates for future branch promotion, research decision review, and auto-merge winner selection.
    - Treat Tailscale as the default private-overlay transport; public relay remains optional future work rather than a release blocker.
    - Add mobile terminal parity as a governed PTY/xterm lane, not as an unrestricted raw terminal that can bypass permission, canonicality, or artifact governance.
    - Keep app-server/protocol control as the preferred rich mobile workbench lane for Codex-style sessions, permissions, threads, model/effort controls, and structured approvals.
    - Exclude the previously pulled `a-n-d-a-i/ULTRON` Telegram bridge from the requested ULTRON evidence set except as a historical correction note.
  interfaces:
    - docs/deep_study/19-remote-host-control-plane.md
    - docs/deep_study/25-remote-transport-auth-contract.md
    - docs/deep_study/33-advanced-systems-implementation-plan.md
    - docs/deep_study/51-m6-m10-reference-superiority-design-audit.md
  evidence_refs:
    - reference_repos/agent_teams/modelscope-ultron/README.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/MemoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/SkillHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/HarnessHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/TrajectoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/memory/memory_service.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/skill/skill_evolution.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/trajectory/segmenter.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/trajectory/trajectory_service.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/harness/allowlist.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/harness/bundle.py
    - reference_repos/requested/llm-as-a-verifier/scripts/verifier_core.py
    - reference_repos/requested/llm-as-a-verifier/scripts/run_terminal_bench.py
    - reference_repos/requested/llm-as-a-verifier/scripts/run_swe_bench.py
    - reference_repos/mobile_remote/hapi/shared/src/socket.ts
    - reference_repos/mobile_remote/hapi/shared/src/schemas.ts
    - reference_repos/mobile_remote/farfield/packages/codex-api/src/app-server-client.ts
    - reference_repos/mobile_remote/farfield/packages/codex-api/src/service.ts
    - reference_repos/mobile_remote/itwillsync/packages/cli/src/server.ts
    - reference_repos/mobile_remote/terminalsync/docs/architecture.md
    - reference_repos/mobile_remote/claude-code-monitor/src/server/index.ts
    - reference_repos/mobile_remote/parallel-code/openspec/specs/remote-access/spec.md
    - reference_repos/mobile_remote/parallel-code/electron/remote/server.ts
    - reference_repos/mobile_remote/CodexMonitor/docs/mobile-ios-tailscale-blueprint.md
    - reference_repos/mobile_remote/codexui/PROJECT_SPEC.md
  next_actions:
    - Add a dedicated collective-intelligence backlog from ModelScope ULTRON: trajectory ingest and segmentation, memory tiering signals, memory-to-skill crystallization, and harness profile share/import.
    - Freeze `VerifierTournament` schemas before widening M9 auto-merge claims beyond the current local deterministic gates.
    - Extend M5 remote plan with a governed PTY terminal-parity lane and a Tailscale detection/serve-plan helper.
    - Add remote harness coverage for reconnect, replay, lease expiry, revocation, and scrollback/cursor replay before claiming mobile full-CLI parity.
  non_goals:
    - This audit does not mark native mobile apps complete.
    - This audit does not revive public relay as a requirement; Tailscale/private overlay is the preferred near-term transport.
    - This audit does not permit remote PTY input to bypass permission or canonical project-surface gates.
    - This audit does not claim that current M7/M8/M11 implementation has already matched ModelScope ULTRON's cross-agent fleet learning, self-evolving skills, Harness Hub sharing, or trajectory feedback loop.
  generated_by: codex
  updated_at: 2026-04-27
---

# Ultron, Verifier, and Tailscale Remote Reference Audit

This audit records the source-level lessons from the newly pulled references:

- `reference_repos/agent_teams/modelscope-ultron`
- `reference_repos/requested/llm-as-a-verifier`
- `reference_repos/mobile_remote/hapi`
- `reference_repos/mobile_remote/farfield`
- `reference_repos/mobile_remote/CodexMonitor`
- `reference_repos/mobile_remote/codexui`
- `reference_repos/mobile_remote/claude-code-viewer`
- `reference_repos/mobile_remote/247-claude-code-remote`
- `reference_repos/mobile_remote/opencode-dashboard`
- `reference_repos/mobile_remote/itwillsync`
- `reference_repos/mobile_remote/terminalsync`
- `reference_repos/mobile_remote/claude-code-monitor`
- `reference_repos/mobile_remote/parallel-code`

The goal is not to copy features. The goal is to identify what should raise the
upper bound of `research-cli` while preserving its stronger invariant:
project truth is owned by the kernel, Git/canonicality gates, schemas, and
`.pmcli`; remote clients and helper agents are projections or governed actors.

## 1. ULTRON

Correction: the intended ULTRON reference is the ModelScope community project
`modelscope/ultron` at `reference_repos/agent_teams/modelscope-ultron`. The
previously pulled `a-n-d-a-i/ULTRON` checkout is a different Telegram bridge and
must not be used as evidence for the requested reference.

ModelScope ULTRON's source-level contribution is not a classic "planner/coder/
reviewer team scheduler". Its stronger idea is a fleet-level collective
intelligence substrate that lets many agents share learned experience:

- Memory Hub stores tiered collective memories, deduplicates and merges related
  records, assigns HOT/WARM/COLD tiers from adoption signals, supports semantic
  search with tier boost and time decay, and sanitizes sensitive content.
- Trajectory Hub ingests session `.jsonl`, uses LLM task segmentation to split
  long conversations into task segments, fingerprints each segment for
  incremental idempotency, scores segments with trajectory metrics, extracts
  memories only from eligible segments. Its supervised-training export path is
  not adopted by research-cli.
- Skill Hub clusters related memories and crystallizes them into multi-step
  workflow skills. Re-crystallization is triggered by new memory accumulation
  and must pass a provenance verifier plus a structure-score upgrade gate.
- Harness Hub syncs allowlisted workspace profiles across agent products,
  shares them through short-code/curl import, backs up overwritten workspaces,
  and excludes sensitive files such as `.env`, `auth.json`, sessions, logs, and
  hidden files.

What this means for `research-cli`:

- Our current M7/M8 memory is stronger on project-local canonicality, explicit
  invalidation, proof-backed query/explain surfaces, and Git/artifact lineage.
- ULTRON is stronger on cross-agent accumulation: trajectory-to-memory,
  memory-to-skill crystallization, re-crystallization, skill catalog indexing,
  and shareable harness profiles.
- Therefore we should not claim full superiority over ModelScope ULTRON until
  these collective-intelligence loops have their own schemas, fixtures,
  implementation, and proof gates.

Adoption decisions:

- Add a `TrajectoryIngest` lane that can parse session JSONL, segment long
  conversations into independent task spans, fingerprint spans, and invalidate
  memories by segment tag when a span changes.
- Extend durable memory with explicit tier/adoption signals: retrieved,
  inspected, merged, injected, superseded, and cited-in-output. HOT/WARM/COLD
  should be explain-visible and must not bypass trust/canonicality gates.
- Add `KnowledgeCluster` and `SkillEvolutionRecord` concepts so repeated
  project-local memories can propose reusable skills. Publication still goes
  through M11 `SkillOutputEnvelope`; the evolution engine may propose but cannot
  directly publish project truth.
- Add a provenance verifier for evolved skills: every step/claim must be
  grounded in source memories or rejected as hallucinated/contradicted.
- Add a structure-score upgrade gate before any re-crystallized skill replaces
  a previous version.
- Add a governed harness/profile export lane for agent blueprints, but make it
  allowlist-only, secret-excluding, backup-before-apply, and canonical-surface
  aware.
- Keep M6/M9 scheduling lessons from EvoScientist, Agent Team, Happier, Squad,
  AutoGen, and branch-runtime references. ModelScope ULTRON complements those
  with shared learning, not with a full runtime scheduler.

## 2. LLM-as-a-Verifier

`llm-as-a-verifier/scripts/verifier_core.py` is directly relevant to M9 and
M10. Its core pattern is:

- decompose evaluation into named criteria
- compare candidates pairwise rather than relying on one absolute score
- repeat each pair/criterion verification several times
- use score tokens and logprobs when the provider supports them
- cache every pair/criterion/repetition result
- select the winner by a round-robin tournament

The benchmark adapters add domain-specific criteria:

- Terminal-Bench: specification adherence, output match, error signal detection
- SWE-bench Verified: root-cause analysis, code quality, empirical verification

The most important instruction is methodological: do not trust the agent's
narration. Trust terminal output, diffs, reproducer output, and final artifacts.

Adoption decision for M9:

- introduce a `VerifierTournament` object for branch batches
- score candidates across explicit criteria tied to the task packet and eval
  packet
- run repeated pairwise comparisons before promotion when the merge is not
  trivially decided by deterministic tests
- cache verifier calls by candidate pair, criterion, repetition, and evidence
  digest
- require the winner to still pass deterministic eval, canonicality, and merge
  gates; verifier preference is not sufficient by itself

Adoption decision for M10:

- use the same criteria/tournament pattern for high-impact `supersede`,
  `pivot`, `fork winner`, and research-claim decisions
- carry alternative interpretations and evidence references into
  `ResearchTurnClassification` instead of relying on a single classifier answer

This is a real ceiling upgrade over ordinary review prompts. It gives
research-cli a way to make branch selection and research decisions less
dependent on one model sample, while still staying evidence-grounded.

## 3. HAPI

HAPI is the strongest full remote-control architecture reference in this pass.
The source shows a real split:

- CLI wraps agent sessions.
- Hub owns sync, Socket.IO, SSE, push, optional Telegram, and tunnel exposure.
- Shared schemas model sessions, machines, agent state, permission requests,
  terminal channels, todos, team state, metadata versions, and machine state.
- Permission requests are first-class records, not ad hoc chat text.
- Remote terminal events are typed as open/write/resize/close/ready/output/exit.

Useful HAPI patterns:

- `session-alive` and `machine-alive` separate liveness from durable content
- metadata and agent state have explicit versions
- runner mode can start sessions remotely without losing session identity
- terminal transport is a feature, but permission handling remains a separate
  state channel
- push and Telegram are notification channels, not runtime authorities

Adoption decision:

- keep our current M5 rule that the phone app is a projection, not the owner
- add HAPI-style versioned remote state and permission queues where our M5
  harness is still shallow
- treat future runner support as a governed `AgentRuntimeRecord` creator, not a
  separate process manager hidden behind the app

## 4. Farfield and codexui

Farfield and codexui both point to the same high-value direction: use Codex
`app-server` semantics when available instead of scraping a raw terminal.

Observed source patterns:

- child-process app-server transport over JSON-RPC
- schema validation around thread list/read/start/resume, turn start/steer,
  turn interrupt, model list, collaboration mode, and user-input responses
- server requests for command approval, file-change approval, and user input
- strict stream-state reduction from snapshots plus patches
- WebSocket or SSE projection to browser clients

Adoption decision:

- mobile full CLI should have two lanes:
  - semantic app-server lane for sessions, turns, approvals, files, models,
    reasoning effort, collaboration mode, and interrupts
  - PTY lane for exact terminal parity when semantic protocol coverage is not
    enough
- app-server lane is preferred for governance because approvals and turns are
  structured
- PTY lane must still be captured as governed terminal events

This is the path to being better than raw mobile terminal projects: the phone
gets terminal parity when needed, but the primary interface remains semantic,
inspectable, and schema-backed.

### Local TUI Reference Layer

claw-code's Rust TUI enhancement plan gives the local terminal implementation
sequence that should sit under the same host-surface design:

- keep command-first inline REPL as the default fast path
- extract rendering, input, session management, and formatting into focused
  modules before adding richer UI behavior
- add status HUD, live token/cost/turn timing, git branch, and permission mode
- render streamed markdown, tool output, and diffs as structured UI, not raw
  dumps
- collapse or page long tool output by default
- add interactive session picker, slash palette, argument completion, and
  theme/capability detection
- make full-screen split panes optional after the inline path is stable

OpenCode and Crush raise the component bar for the optional full-screen mode:
chat page, editor/input, message list, sidebar, status, permission dialog,
session/model/file/command dialogs, logs table, layout/overlay primitives,
theme manager, diff view, image/attachment display, notifications, and
completion surfaces.

Adoption decision:

- `research-cli` should introduce `HostSurfaceProjection` before implementing
  visual panes. Local TUI, browser, and mobile should all consume the same
  projection and emit the same typed action envelopes.
- TUI rendering must be testable without a real terminal through snapshot JSON
  and projection reducer fixtures.
- Full-screen TUI is useful, but it should be feature-gated and optional. The
  inline REPL must remain available for SSH, tmux, CI, minimal terminals, and
  automation-heavy workflows.

## 5. Tailscale and PTY Mobile Terminal Projects

The best direct Tailscale/mobile terminal references are `itwillsync`,
`terminalsync`, `claude-code-monitor`, and `parallel-code`.

### itwillsync

itwillsync demonstrates:

- one-command QR pairing
- local WiFi or Tailscale mode
- node-pty plus xterm.js
- per-session random tokens
- NaCl secretbox encryption for WebSocket frames
- scrollback replay on reconnect
- mobile extra keys and reconnect UX
- a dashboard for multiple terminal sessions

Its architecture is intentionally agent-agnostic: if a tool runs in a terminal,
it can be mirrored. That is useful for parity, but insufficient for
research-cli governance unless wrapped.

### TerminalSync

TerminalSync is simpler and security-focused:

- no daemon
- no persistent keys
- single-process PTY mirror
- ephemeral X25519 key exchange plus PSK from QR payload
- HKDF-derived session key
- XSalsa20-Poly1305 frames
- short authentication string for MITM detection
- scrollback replay by output sequence

It is a good model for an ephemeral, direct-control mode when the user wants
"open this exact running terminal on my phone" and does not need a long-lived
dashboard.

### parallel-code

Parallel Code combines git worktree isolation, per-agent PTY sessions, and a
phone monitor. Its `openspec/specs/remote-access/spec.md` is unusually concrete:

- remote server is explicitly user-started and singleton
- WiFi and Tailscale URLs are advertised together
- bearer tokens are session-scoped, generated from cryptographic RNG, and not
  persisted
- WebSocket first-message auth is preferred over URL-token auth
- unauthenticated clients time out
- concurrent connections, frame size, input length, resize dimensions, and
  agent id length are bounded
- agent list, status, scrollback, output, input, resize, kill, subscribe, and
  unsubscribe are separate protocol messages

Adoption decision:

- borrow its explicit remote-access spec style for our future M5 terminal lane
- use first-message auth where possible so tokens do not have to live in URLs
- bound every remote PTY operation before it reaches the terminal lease
- keep git worktree isolation and merge governance stricter than Parallel Code:
  research-cli already has M9 canonicality, debate, and publication gates

### claude-code-monitor and CodexMonitor

These show pragmatic Tailscale support:

- detect Tailscale IP through CLI first, then network interfaces
- validate the Tailscale CGNAT range
- expose QR/mobile URLs only when the user asks
- keep hook-derived session state in a local store
- expose focus, keystrokes, screen capture, and permission prompt status

Adoption decision:

- add `remote tailscale status/serve-plan` as a future helper surface
- implement Tailscale detection through CLI plus interface fallback
- validate `100.64.0.0/10` addresses instead of trusting arbitrary command
  output
- keep QR tokens short-lived and session-scoped
- add reconnect/cursor replay tests before claiming full mobile parity

## 6. Claude Code Viewer, 247, and OpenCode Dashboard

Claude Code Viewer contributes a useful product boundary:

- read-only session/log browsing can be separated from send/approve control
- password or bearer auth is necessary even on private overlays
- PWA push notification is valuable for long sessions

247 Claude Code Remote contributes the fast path for raw terminal parity:

- xterm.js + WebSocket + node-pty + tmux gives a very complete mobile terminal
  surface quickly
- persistence should come from tmux or an equivalent terminal/session layer

OpenCode Dashboard contributes monitoring separation:

- dashboards should observe and classify blocked/stale work
- dashboards should not own the agent runtime truth
- encrypted feed and API keys are useful even on headless machines over
  Tailscale

Adoption decision:

- remote workbench should expose at least two modes:
  - watch-only mode for low-risk browsing
  - control mode gated by lease, permission state, and human approval
- terminal parity can use tmux/PTTY-style mechanics, but promotion,
  publication, memory, and branch merge remain governed by research-cli

## 7. Concrete Research-CLI Upgrades

### 7.1 M9/M10 Verifier Tournament

Add a future schema family:

```text
VerifierCriterion {
  criterion_id
  name
  description
  evidence_requirements[]
  ground_truth_note
}

VerifierPairScore {
  batch_id
  candidate_a
  candidate_b
  criterion_id
  repetition_index
  score_a
  score_b
  provider
  model
  scoring_mode
  evidence_digest
  raw_trace_ref?
}

VerifierTournament {
  tournament_id
  scope
  candidate_refs[]
  criteria[]
  repetitions
  pair_scores[]
  winner_ref
  deterministic_gate_result_ref
  canonicality_gate_result_ref
  merge_gate_result_ref?
}
```

Acceptance rule:

- a verifier tournament may recommend a winner
- only deterministic eval plus canonicality plus merge/publication gates may
  promote a winner

Implementation status:

- implemented for M9 branch promotion: `branches verify` creates a persisted
  `VerifierTournament`, emits repeated `VerifierPairScore` records across
  explicit criteria, stores a cacheable evidence digest, attaches the tournament
  to `BranchRun`, and lets `branches promote --require-verifier` block when the
  tournament winner does not match the promoted branch
- still future: LLM/logprob scoring providers and M10 research-decision
  tournament use for `pivot`, `fork winner`, `supersede`, and claim validation

### 7.2 Local TUI And Mobile Remote Control Split

The active implementation does not yet have a complete local TUI; it has an
interactive skeleton and command/JSON operator surfaces. Future host surfaces
should therefore be built together instead of treating local TUI and phone UI as
separate products. Local terminal TUI, browser workbench, and mobile workbench
must all read the same kernel-owned projections.

Future local/remote control should be split:

```text
LocalTerminalTui {
  session timeline
  tool stream
  permission queue
  project/memory/branch/review panels
  research-stage panel
}

MobileSemanticControl {
  session/thread browse
  turn start/steer/interrupt
  permission approval
  model/effort/collaboration mode
  project/memory/branch/review projections
}

MobileTerminalParity {
  PTY attach
  xterm.js rendering
  resize/input/signal
  scrollback replay
  extra mobile keys
  governed event capture
}
```

Acceptance rule:

- local TUI, browser, and phone clients are all projections over the same
  command registry and runtime state
- semantic control is preferred for any operation that maps to a schema
- PTY input is allowed only through a terminal lease and must emit auditable
  remote terminal events
- neither lane may publish docs/code, promote memory, merge a branch, or change
  artifact latest pointers without the existing governance transaction

### 7.3 Tailscale-First Deployment

Public relay is no longer necessary for the near-term M5 path. The preferred
path is:

1. local daemon binds to localhost or selected private interface
2. `remote tailscale status` detects whether a tailnet IP exists
3. `remote tailscale serve-plan` prints a machine-readable plan for
   `tailscale serve` or direct `http://100.x.y.z:<port>/`
4. phone browser/PWA connects over tailnet
5. app remains a projection over `.pmcli` and machine-global remote state

This keeps deployment simple while avoiding a premature public relay service.

## 8. Updated Superiority Judgment

The current CLI still exceeds these references on runtime authority and
project governance for its implemented scopes:

- ModelScope ULTRON is ahead of the pre-M15 implementation on fleet-level
  collective learning: trajectory-to-memory extraction, memory-to-skill
  crystallization/re-crystallization, allowlisted harness sharing, external
  skill catalog indexing, while research-cli intentionally leaves that export
  path out.
- `research-cli` is ahead of ModelScope ULTRON on project-local authority:
  Git-aware canonicality, one-public-latest artifact governance, reversible
  publication, explicit invalidation, branch promotion gates, and research DAG
  operations.
- itwillsync/TerminalSync give better raw terminal parity than our current M5,
  but they do not have project memory, branch governance, research runtime,
  artifact publication, or canonical surface management.
- HAPI/Farfield/codexui have stronger mobile/session UX than our current M5,
  but they are not research-governance runtimes.
- LLM-as-a-Verifier currently exceeds our implemented M9 branch judging on
  verifier sophistication; we should absorb its tournament method before making
  stronger auto-merge claims.

Therefore the precise claim is:

- `research-cli` is ahead in governed research-runtime architecture and
  project-truth discipline
- after M15/M16/M18, it is ahead of ModelScope ULTRON in local governance for
  trajectory memory, skill evolution, and advisory feedback calibration; M17
  harness profile sharing remains deferred
- it is not yet ahead in full mobile terminal parity
- it should adopt ModelScope ULTRON's collective-intelligence loops, verifier
  tournaments, and Tailscale/PTY/app-server remote lanes to exceed the
  references in learning quality, research quality, and mobile UX
