---
doc_frame:
  doc_id: docs.deep_study.51_m6_m10_reference_superiority_design_audit
  schema_version: "1"
  source_path: docs/deep_study/51-m6-m10-reference-superiority-design-audit.md
  title: M6-M10 Reference Superiority Design Audit
  doc_type: reference_audit
  lifecycle: active
  scope: milestone
  milestone: M6-M10
  summary: M6-M10 audit of whether the advanced-system design and implemented proof gates are high enough to exceed Happy, Happier, claw-code, Crush, OpenCode, ModelScope ULTRON, and newer research-agent references.
  key_claims:
    - M0-M10 and the terminal CLI have implementation-backed proof for their graduated project-local scopes in the active worktree; ProjectOps now includes a lease/wake supervision state machine; M11 skill-output publication is proof-backed for governed output publication plus a local command runner adapter, while non-local runner families remain future work.
    - The advanced design is architecturally stronger than the references only if its proof gates remain schema-backed, fixture-backed, and kernel-authoritative.
    - M6 explicitly absorbs Happier execution-run/catalog lessons, Happy reconnect/concurrency lessons, claw-code mock parity, Ruah task records, and Agent Team reviewer isolation.
    - M9/M10 should now absorb LLM-as-a-Verifier style criterion decomposition, repeated pairwise verifier scoring, cacheable score traces, and tournament selection for ambiguous branch/research decisions.
    - M5+ mobile work should absorb Tailscale-first direct control, app-server semantic control, and governed PTY terminal parity from HAPI, Farfield, codexui, CodexMonitor, itwillsync, TerminalSync, Parallel Code, and Claude Code Viewer.
    - EvoScientist raises the product UX bar for M5-M11 through explicit research roles, adaptive tool selection, ask_user/HITL interrupts, multi-channel adapters, MCP role routing, and installable skills; research-cli should absorb these as typed gates, traces, and projections.
    - ModelScope ULTRON raises the post-M11 collective-intelligence bar through tiered shared memory, trajectory segmentation and scoring, memory-to-skill crystallization/re-crystallization, provenance skill verification, structure-score upgrade gates, and allowlisted Harness Hub profile sharing; research-cli intentionally excludes supervised-training export.
  decisions:
    - Treat M6 as graduated only for packet-bound mock/local execution and review isolation, not broad live-provider delegation.
    - Treat reference superiority as a proof ledger, not as prose confidence.
    - Keep full local TUI and mobile full CLI parity deferred to the shared host-surface milestone so local TUI, web, and mobile clients use one projection contract.
    - Do not claim complete superiority over ModelScope ULTRON until the collective memory/skill/harness/trajectory loops are implemented or explicitly scoped as future work.
  interfaces:
    - docs/deep_study/30-milestone-execution-handbook.md
    - docs/deep_study/33-advanced-systems-implementation-plan.md
    - docs/deep_study/34-advanced-build-task-pack-backlog.md
    - docs/deep_study/40-reference-superiority-self-audit.md
    - docs/deep_study/44-base-cli-superiority-proof-gate.md
    - docs/deep_study/45-base-cli-proof-execution-matrix.md
  evidence_refs:
    - reference_repos/requested/happy/docs/protocol.md
    - reference_repos/requested/happy/docs/realtime-sync-and-rpc.md
    - reference_repos/requested/happier/docs/agents-catalog.md
    - reference_repos/requested/happier/apps/cli/src/agent/executionRuns
    - reference_repos/requested/claw-code/rust/MOCK_PARITY_HARNESS.md
    - reference_repos/requested/claw-code/rust/TUI-ENHANCEMENT-PLAN.md
    - reference_repos/requested/crush/README.md
    - reference_repos/requested/crush/internal/ui
    - reference_repos/requested/opencode/internal
    - reference_repos/requested/opencode/internal/tui
    - reference_repos/requested/llm-as-a-verifier/scripts/verifier_core.py
    - reference_repos/mobile_remote/hapi/shared/src/socket.ts
    - reference_repos/mobile_remote/farfield/packages/codex-api/src/app-server-client.ts
    - reference_repos/mobile_remote/itwillsync/packages/cli/src/server.ts
    - reference_repos/mobile_remote/terminalsync/docs/architecture.md
    - reference_repos/research_agents/EvoScientist/EvoScientist/EvoScientist.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/subagent.yaml
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/tool_selector.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/ask_user.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/consumer.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/middleware.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/mcp/README.md
    - reference_repos/agent_teams/modelscope-ultron/README.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/MemoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/SkillHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/HarnessHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/TrajectoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/skill/skill_evolution.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/harness/allowlist.py
  next_actions:
    - Keep branch evaluation and director-only promotion in M9 instead of weakening the M6 boundary.
    - Add verifier-tournament schemas before widening M9 auto-merge claims beyond deterministic local gates.
    - Add local TUI panes, remote terminal lease, replay, and Tailscale helper gates before claiming full host-surface or mobile CLI parity.
    - Add role-scoped MCP/tool routing, ToolSelectionTrace, StageReflectionPacket, and channel/HITL hardening gates as post-M11 improvement work.
    - Add ModelScope ULTRON-derived post-M11 collective-intelligence gates: trajectory ingest/segmentation, memory adoption tiers, memory-cluster skill evolution, provenance skill verification, and harness profile sharing.
    - Re-run full conformance before merging the M10/M11 worktree.
    - Re-run conformance after any M6-M10 design-plan update.
  non_goals:
    - This audit claims only the project-local M7-M8 memory scope is implemented; the vector and temporal-graph lanes are embedded local backends, not external vector DB or Neo4j/FalkorDB deployments.
    - This audit does not restart native mobile or public relay work.
    - This audit does not claim that current M11 has implemented ModelScope ULTRON-style skill crystallization, Harness Hub sharing, or trajectory feedback.
  generated_by: codex
  updated_at: 2026-04-27
---

# M6-M10 Reference Superiority Design Audit

This document began as the pre-M6 entry gate and now also records the M6 proof
ledger.

It answers one question:

- is the current M6-M10 design and M6 implementation high enough to keep the
  project's claim that it can exceed the reference projects?

Short answer:

- yes, the M6 core proof gates are implementation-backed for packet-bound
  mock/local execution and review isolation
- M7-M8 are now proven for bounded working memory, digest staging, durable
  promote/query/explain/invalidate, deterministic hybrid retrieval, embedded
  vector retrieval, and local temporal-graph successor routing
- M9 is now proven for the project-local branch intelligence surface: real Git
  worktrees, executable candidate artifacts, LLM/agent mutation through a command
  adapter, real eval commands, canonicality gates, debate traces, single-winner
  promotion, gated automatic winner merge, loser archive, and strict schemas
- ModelScope ULTRON still exceeds us on cross-agent collective learning:
  trajectory-to-memory, memory-to-skill evolution, and harness profile sharing
  are future proof gates rather than current claims
- M6 remains intentionally bounded: live-provider delegation and branch
  promotion must not bypass packet, review, and canonicality gates

## 1. Current Evidence Boundary

The current evidence line is:

| Area | Current status | Claim allowed now |
|---|---|---|
| M0-M2 kernel/session/provider/permission floor | implemented and passing in the merged tree | implementation-backed foundation |
| M3/M4 repo governance and base terminal CLI | implemented with machine-readable help graduation | base terminal CLI is proof-backed, except feature-gated advanced memory |
| M5 remote operator plane | implemented as Tailscale/private-overlay PWA and local daemon substrate | remote operator substrate is projection-driven; not full mobile CLI |
| M6 multi-agent core | implemented and passing for packet-bound mock/local execution plus review isolation | M6 core is proof-backed; live-provider delegation remains gated |
| M7-M8 memory and ProjectOps | M7 working memory plus M8 durable promote/query/explain/invalidate, deterministic hybrid retrieval, embedded vector retrieval, local temporal-graph successor routing, and ProjectOps duplicate/stale/reclaim/wake ack/resolve/escalate supervision implemented | M7-M8 proof-backed for project-local governed memory and supervision |
| M9 evolutionary branch intelligence | implemented for project-local worktree/mutation/evaluation/debate/promotion/merge loop | proof-backed for local branch governance with command-adapter LLM mutation and gated winner merge |
| M10 research runtime | implemented for schema-backed nonlinear research threads, stage operations, decisions, classification, and canonical supersede refresh | proof-backed for project-local research runtime governance |
| M11 skill output publication | implemented for envelope submit/list/inspect/publish, contract-aware submit validation, native docs candidate ingress, publication gates, canonical `skill_outputs` surface, prompt/resume/compact public-latest-only projection, and local command runner adapter with side-effect audit | proof-backed for publication governance and local adapter execution; non-local runner families remain future work |
| ModelScope ULTRON collective intelligence | not implemented as a complete loop; only adjacent M7/M8 memory and M11 publication pieces exist | future-work proof gate for trajectory ingest, memory tiers, memory-to-skill evolution, and harness profile sharing |

Claim discipline:

- "architected to exceed" is allowed for future M11+ slices only when tied to a frozen
  design and proof gate.
- "implemented and exceeding" is not allowed for new slices until schemas,
  fixtures, implementation, and conformance are all green.

## 2. Reference Lessons That Must Survive Implementation

## 2.1 Happy / Happy Server

Observed source lessons:

- `happy/docs/protocol.md` uses scoped connections:
  `user-scoped`, `session-scoped`, and `machine-scoped`.
- Durable updates are separated from ephemeral presence and usage.
- User-level update sequence numbers provide monotonic reconciliation.
- Versioned fields use optimistic concurrency via `expectedVersion`.
- `happy/docs/realtime-sync-and-rpc.md` keeps normal sync and point-to-point
  RPC on one transport while tracking reconnect and target disappearance.

Adoption rule:

- For M6-M10, any daemon/agent/branch/memory projection that can be replayed
  must have a monotonic cursor or sequence.
- Mutating records must have either compare-and-swap semantics or an explicit
  single-writer gate.
- Agent liveness is allowed to be ephemeral; agent output, trace, task packet,
  and final status are durable.

What we should exceed:

- Happy's server can own broad synced state. `research-cli` must keep remote
  and agent UIs as projections over kernel-owned project state.

## 2.2 Happier

Observed source lessons:

- `happier/docs/agents-catalog.md` makes agents catalog-driven and
  capability-driven rather than screen-specific.
- `happier/packages/protocol/src/executionRunStartRequest.ts` distinguishes
  execution intent, retention policy, run class, IO mode, resume handle, and
  replay seed.
- `happier/apps/cli/src/agent/executionRuns` has explicit run lifecycle
  managers for start, ensure/resume, finish, stop, actions, markers, and
  budget acquisition.
- Happier's review, delegate, memory-hints, and replay-summary profiles show
  that subagent work needs typed intent profiles, not a generic "run agent"
  blob.
- Happier's mobile/UI subagent panels are useful, but they are display
  surfaces; the runtime contract still needs to live below them.

Adoption rule:

- M6 `TaskPacket` must not be a thin prompt wrapper. It must include intent,
  retention, run class, IO mode, resume/replay policy, budget, scope, write
  authority, and output manifest obligations.
- `AgentRuntimeRecord` must model lifecycle explicitly:
  `queued`, `running`, `succeeded`, `failed`, `cancelled`, `timeout`.
- Reviewers, executors, delegates, memory-hint generators, and future research
  stage workers should be typed roles/profiles with allowed actions.

What we should exceed:

- Happier has rich cross-surface execution runs. `research-cli` must additionally
  make every run project-governed, git-aware, reviewable, and canonicality-safe.

## 2.3 ModelScope ULTRON

Correction: the intended ULTRON reference is `modelscope/ultron`, not the
earlier mistakenly pulled GitHub Telegram bridge. The ModelScope project is best
understood as a collective-intelligence layer across agents:

- Memory Hub ingests text or session `.jsonl`, deduplicates and merges memories,
  assigns HOT/WARM/COLD tiers from adoption signals, and retrieves with
  semantic ranking, tier boost, time decay, and sanitization.
- Trajectory Hub segments long session logs into independent task spans, uses
  fingerprints for idempotent incremental tracking, labels segment quality, and
  extracts memories only from eligible segments. Its training-export lane is a
  reference observation, not a research-cli adoption target.
- Skill Hub clusters related memories into reusable workflows, then
  crystallizes and re-crystallizes skills with a separate provenance verifier
  and a structure-score upgrade gate.
- Harness Hub publishes/imports allowlisted agent workspace blueprints while
  excluding secrets, sessions, logs, and hidden files.

Adoption rule:

- M7/M8 memory should grow from project-local retrieval into a governed
  trajectory-to-memory loop with segment fingerprints, quality thresholds, and
  precise invalidation by segment tag.
- M11 skill publication should grow into memory-cluster skill evolution, but
  evolved skills must still enter through `SkillOutputEnvelope` candidates and
  publication gates.
- Harness/profile sharing must be allowlist-only, backup-before-apply, and
  canonical-surface aware.
- Supervised-training export is out of scope; trajectory evidence must improve
  memory, skills, harness profiles, routing, and verifier calibration without
  becoming an authority path for changing project truth.

What we should exceed:

- ULTRON is stronger than our current implementation on cross-agent collective
  learning. `research-cli` should exceed it by adding the same learning loops
  under stricter Git/canonicality, artifact-family, research-DAG, and
  publication-governance rules.

## 2.4 claw-code

Observed source lessons:

- `claw-code/rust/MOCK_PARITY_HARNESS.md` proves behavior through a
  deterministic mock provider and clean-environment CLI harness.
- `claw-code/rust/PARITY.md` records honest surface-vs-behavior gaps instead of
  hiding incomplete parity.

Adoption rule:

- M6 must ship with a deterministic mock-agent harness before it claims any
  multi-agent behavior is graduated.
- The proof matrix must separate `design_frozen`, `schema_ready`,
  `fixture_ready`, and `implemented_and_passing`.

What we should exceed:

- claw-code proves CLI and tool behavior well. `research-cli` should extend the
  same proof rigor to project-native agents, review isolation, memory, and
  branch promotion.

## 2.5 Crush / OpenCode

Observed source lessons:

- Crush is strong on TUI ergonomics, provider/model configuration, permission
  prompts, skills, MCP, notifications, project context initialization, and
  terminal-first daily use.
- OpenCode has SQLite-backed sessions/messages, pubsub, diff/history services,
  LSP integration, file change tracking, and auto-compact behavior.
- claw-code's Rust TUI plan is a useful incremental bridge from command-first
  REPL to polished TUI: extract monolithic REPL/render/session logic, add status
  HUD, live markdown, collapsible tool output, colored diffs, internal pager,
  session picker, themes, then optional full-screen split panes.
- OpenCode's `internal/tui` shows the component taxonomy needed for a serious
  TUI: chat page, editor, message list, sidebar, status, session/model/file/
  command dialogs, permission dialog, logs table, layout/overlay primitives,
  and theme manager.
- Crush's `internal/ui` reinforces the need for first-class chat, completions,
  dialog, diffview, image/attachment, notification, style, and permission
  integration rather than plain terminal dumping.

Adoption rule:

- M6-M10 operator surfaces must remain terminal-first and inspectable before
  any mobile-specific expansion.
- Agent outputs and branch candidates need diff/history visibility, not only
  final summaries.
- Future local TUI, browser, and mobile panels must read the same command
  registry and runtime state; they cannot introduce a second command surface or
  UI-only authority.
- Current implementation has an interactive launch skeleton and command/JSON
  operator surfaces, not a complete terminal TUI. Full TUI belongs with the
  governed host-surface/remote-control milestone.
- Build the TUI as projection-first: every pane must have a schema-backed
  `HostSurfaceProjection` slice and every user action must emit a typed
  `HostSurfaceAction` through the command/permission/canonicality gates.
- Ship inline REPL upgrades before full-screen mode. The inline path should
  cover status HUD, markdown/tool/diff rendering, pager, collapsible tool
  output, session picker, slash palette, and permission overlay.
- Make full-screen TUI optional and feature-gated. It can add split panes and
  richer keyboard navigation, but not a separate runtime or hidden state.

What we should exceed:

- Crush/OpenCode are strong interactive products. `research-cli` must pair that
  ergonomics bar with stricter project truth, git-native canonicality, and
  schema-backed advanced runtime legality.

## 3. Design Strengths Already Above The References

The current design is genuinely strong in these ways:

1. One authority line:
   `KernelStateBundle`, canonical events, project-local `.pmcli/`, and
   machine-global remote state are separated.
2. Canonicality as product management:
   M3/M4 require exactly one active public doc/code surface before promotion.
3. Proof-first status vocabulary:
   `implemented_and_passing` and `feature_gated` prevent accidental claims.
4. Remote projection discipline:
   M5 rejects app-owned runtime truth and keeps Tailscale as transport only.
5. Research-specific ambition:
   memory, ProjectOps, branch search, debate, review, and research runtime are
   planned as native systems rather than separate scripts.

These are real architecture advantages.

They become reference superiority only when the M6-M10 proof gates are also
implemented.

## 4. Weak Spots Before M6

The current M6-M10 design still has five weak spots:

1. M6 `TaskPacket` is under-specified relative to Happier execution runs.
   It names packet binding, but not enough fields for intent/profile,
   retention, replay, IO mode, budget, output manifest, and restart legality.
2. M6 lifecycle fixtures are too coarse.
   "inspect and stop safely" is necessary, but not enough. We need start,
   stop, timeout, crash, resume/replay, stale packet, invalid scope, and
   output-manifest fixtures.
3. Review blinding needed mechanical proof.
   The M6 implementation now includes an explicit `ReviewInputRedaction` /
   blinded review-packet gate and a fixture proving executor interpretation is
   absent from the default packet and prompt snapshot.
4. M7-M8 memory promotion is high-risk.
   Candidate, promotion, invalidation, decay, and explain surfaces are planned,
   but stale memory and rollback invalidation must become release-blocking
   tests before auto-injection is trusted.
5. M9 branch search needs stronger git semantics.
   It must treat worktree base SHA, dirty state, merge base, conflict result,
   canonicality audit, evaluation packet, and review gate as first-class
   promotion evidence.

## 5. Non-Negotiable M6 Entry Gates

Before implementing broad M6 behavior, update the M6 task plan so these are
explicit acceptance gates:

1. `TaskPacket` schema includes:
   `intent`, `role_profile`, `retention_policy`, `run_class`, `io_mode`,
   `resume_policy`, `replay_seed_ref`, `budget`, `scope`, `write_authority`,
   `success_criteria`, `output_manifest_required`, and `review_gate_required`.
2. `AgentRuntimeRecord` schema includes:
   durable lifecycle status, timestamps, pid/process handle when available,
   task-packet ref, output-manifest ref, trace refs, stop reason, failure code,
   and latest heartbeat.
3. `AgentTrace` separates:
   input packet, runtime events, tool actions, permission decisions,
   output records, and final status.
4. Reviewer runs require a blinded input builder by default.
5. Stopping an agent must be idempotent and inspectable.
6. Invalid packet, stale scope, missing write authority, and dirty forbidden
   worktree cases must return typed failures.
7. Mock-agent conformance must run without live providers.
8. `help --json` must mark `agents` and related surfaces as feature-gated until
   these gates are implemented and passing.

## 6. M7-M10 Proof Gates

M7:

- [x] bounded working memory append log
- [x] deterministic eviction and pinning
- [x] digest candidates are staged, not silently promoted
- [x] compaction-triggered session summary is reviewable and traceable

M8:

- [x] durable memory promote/query/explain/status/invalidate
- [x] superseded memory stops auto-injecting
- [x] rollback/cleanup restore invalidates affected memory records
- [x] retrieval budget and provenance are visible to the operator
- [x] hybrid query backend fuses exact identifier, source-artifact,
  provenance, support-ref, lexical, semantic-lite, vector-embedding,
  temporal-graph, and temporal-trust lanes with RRF so path/source-backed
  memories can beat noisy lexical hits and superseded memories can route to
  current successors
- [x] ProjectOps supervision rejects duplicate live ownership, detects stale
  leases from heartbeat/stale-after, preserves reclaim lineage, rejects
  missing-lease wakes, supports explicit no-owner escalation, and exposes
  wake ack/resolve/escalate transitions through the terminal CLI

M9:

- each branch run records base SHA, head SHA, worktree path, dirty state,
  evaluation packet, review packet, and canonicality audit
- stale-base handling blocks promotion until refresh or explicit rejection
- losing branches are archived or cleaned through governed artifact families

M10:

- `StageExecutionMap` exists before stage runtime
- every stage transition emits a typed execution record
- repair/pivot/retry edges are replayable
- the implemented M10 hot path refreshes a research-runtime DocFrame for
  governed `supersede` operations
- general skill-generated plans, reviews, reports, and handoffs refresh
  DocFrames only after the post-M10/M11 `SkillOutputEnvelope` publication
  runtime lands

## 7. Final Pre-M6 Verdict

The current design is strong enough to proceed to M6 only if M6 starts as a
proof-first implementation of packet-bound execution runs.

It is not enough to add "agents list/inspect/stop" around generic child
processes.

The implementation should begin with:

1. schema freeze for `TaskPacket`, `AgentRuntimeRecord`, `AgentTrace`, and
   `AgentOutputManifest`
2. deterministic mock-agent harness
3. lifecycle fixtures
4. blinded review packet builder
5. terminal command registry feature-gate update

Only after those pass should the project add richer delegation, branch
scheduler integration, memory-hint agents, or UI affordances.

This keeps the project ahead of the references on the dimension that matters:
the native runtime can make agent work packet-bound, replayable, inspectable,
and project-governed instead of only chat-thread- or process-bound. The target
is not just more surfaces, but stronger authority, stronger evidence, and a
single canonical project truth.

## 8. M6 Implementation Progress

As of 2026-04-26, M6 core has graduated these implementation-backed proof
slices:

1. deterministic mock-agent lifecycle without live providers
2. bounded local runner execution in an isolated git worktree
3. external task-packet validation with invalid packet, stale scope, missing
   write authority, and duplicate-agent rejection
4. dirty source-worktree typed rejection before local agent worktree creation
5. local runner timeout and crash classification with persisted lifecycle,
   trace, stdout/stderr, status, and output-manifest records
6. output-manifest validation over concrete output refs
7. `agents replay <agent-id>` replay/resume semantics from a prior local
   `TaskPacket`
8. default reviewer blinding with schema-backed redaction proof
9. `reviews retry <review-id>` retry semantics with fresh-thread execution,
   `retry_of`, `retry_attempt`, and preserved `compare_against` linkage

The local runner is explicitly based on reference lessons from OpenAGS
`DIRECTIVE.md` / `STATUS.md` handoff files, OpenClaw-style task/runtime state,
Ruah-style task record validation, and AutoResearchClaw-style stage
accountability. It persists the task packet, workspace binding, directive,
status, stdout/stderr, runtime record, output manifest, and trace before
exposing the run through `agents list`, `inspect`, `stop`, `replay`, and
`traces`. The reviewer blinding and retry slices absorb ARIS reviewer
independence and Agent Team reviewer-lockout lessons by making executor
interpretation a redacted input source, keeping retries fresh-threaded, and
preserving compare linkage explicitly rather than through hidden chat context.

M6 deliberately graduates packet-bound mock/local execution and review
isolation, not unconstrained live-provider delegation. Evolutionary branch intelligence,
director-only promotion, lineage, evaluation, and debate remain M9 scope; they are not
allowed to weaken the M6 claim or create a second project truth surface.

## M9 Upgrade Note - Evolutionary Branch Intelligence

M9 should absorb the strongest branch-optimization lessons from AVO / Agentic
Variation Operators, AlphaEvolve-style evolutionary code search,
karpathy/autoresearch keep-or-discard loops, ruah worktree claims, Ralph quality
gates, and AutoResearchClaw branch exploration. The proof target is not merely
"search exists". The proof target is that multiple executable hypotheses can be
created, evaluated, debated, repaired, archived, and promoted under Git-native
canonical governance while exposing only one latest public project truth.

Implementation note: the graduated M9 surface now creates real detached Git
worktrees for each candidate, writes an executable hidden candidate artifact in
that worktree, mutates candidates through an LLM/agent command adapter, evaluates
candidates with real shell commands, blocks failed eval/canonicality cases from
promotion, persists debate evidence, enforces one promoted winner per batch,
automatically applies the winner's public diff, and archives losers without
publishing stale public docs/code. The merge excludes hidden candidate artifacts
and rolls back on post-merge canonicality failure.

2026-04-27 verifier-tournament upgrade: `LLM-as-a-Verifier` shows that the next
ceiling increase is not just "more review". It is criterion decomposition,
repeated pairwise scoring, cacheable pair/criterion/repetition traces, and
round-robin tournament selection grounded in observed output, diffs, tests, and
artifact evidence rather than agent self-assessment. Future M9/M10 promotion
claims should add a `VerifierTournament` gate for ambiguous branch winners,
high-impact `supersede` operations, and research `pivot` decisions. The
tournament may recommend a winner, but deterministic eval, canonicality, and
merge/publication gates remain mandatory before project truth changes.

2026-04-27 mobile/Tailscale upgrade: HAPI, Farfield, codexui, CodexMonitor,
itwillsync, TerminalSync, Parallel Code, Claude Code Viewer, and adjacent remote projects show
that mobile full-CLI parity needs two lanes. The semantic lane should use
app-server/protocol control for sessions, turns, approvals, model/effort
settings, project status, memory, branches, reviews, and research-stage
projections. The terminal-parity lane should use governed PTY/xterm transport
for exact CLI rendering, resize/input/signal, mobile extra keys, and scrollback
replay. Both lanes should run over Tailscale/private overlay first; public relay
and native app packaging are optional future work. Neither lane may bypass
permission queues, terminal leases, canonicality gates, or publication
transactions.

## 10. EvoScientist Upgrade Note - Research UX Middleware

The EvoScientist audit adds a different kind of reference pressure than the
kernel and branch-search references. EvoScientist is not stronger on project
truth or canonicality, but it is stronger on research-agent product polish.

Source-level lessons:

- `EvoScientist.py` keeps non-agent CLI commands fast through lazy runtime
  initialization, then composes workspace, skills, memory, MCP, subagents, and
  middleware only for real agent sessions.
- `subagent.yaml` defines six concrete research roles: planner, research, code,
  debug, data-analysis, and writing. The planner's reflection JSON is a useful
  shape for future stage repair and todo updates.
- `tool_selector.py` conditionally runs adaptive tool selection only above a
  threshold, always includes reflection/delegation tools, and records selected
  tools for display.
- `ask_user.py` and `channels/consumer.py` implement interrupt/resume flows for
  clarification and HITL approval, including pending reply slots, timeouts, and
  channel delivery.
- `channels/middleware.py` implements practical remote/channel hygiene:
  deduplication, sender/channel allowlists, pairing, group-history context,
  mention gating, formatting, and bounded background behavior.
- `mcp/README.md` shows role-scoped MCP exposure through `expose_to` and
  wildcard tool allowlists.

Adoption rule:

- role prompts become typed role templates under M6 `TaskPacket`
- planner reflection becomes a typed future `StageReflectionPacket` feeding M10
  `repair`, `retry`, `pivot`, `fork`, `supersede`, and todo-update records
- adaptive tool selection becomes an auditable `ToolSelectionTrace`, never an
  invisible capability grant
- ask_user/HITL becomes M10 pending human gates and typed resume decisions
- remote/channel messages become adapter envelopes over the same kernel-owned
  project truth
- MCP and skill tools route by role and scope; skill outputs still publish only
  through `SkillOutputEnvelope`

Comparative verdict:

- EvoScientist is ahead on ready-made multi-channel research-agent UX.
- Research-cli remains ahead on authority if these lessons are implemented as
  schema-owned gates, traces, and projections rather than prompt-only behavior.
