# Milestone Roadmap for Building the Native Project-Memory Multi-Agent Code Agent CLI

This roadmap sequences implementation so the system stays coherent.

The order matters. The user-specific advanced capabilities should not be
implemented before the code-agent kernel is trustworthy.

## Milestone 0: Bootstrap, Protocols, And Guardrails

Goal:

Create the minimum repository skeleton, schema directory, and verification
harness.

Deliverables:

- module skeleton under `internal/`
- `.pmcli/` versioned protocol draft committed as machine-readable schemas
- top-level state-machine transition table
- executable runtime protocol spec
- kernel parity contract
- `ReviewPacket` / review trace schema
- research review and workflow contract spec
- skill manifest schema
- artifact family manifest schema
- deterministic mock model harness
- `doctor` skeleton
- conformance harness skeleton

Exit criteria:

- app boots
- doctor runs
- tests can execute in CI
- schemas validate canonical fixtures

## Milestone 1: Kernel Session Runtime

Goal:

Build the base code-agent CLI runtime before any memory or research logic.

Deliverables:

- workspace discovery
- project registry baseline
- session creation/list/load/delete
- session browse/search/export
- transcript JSONL persistence
- structured event log
- one-shot prompt mode
- interactive session loop
- JSON output contract baseline
- basic compaction command
- artifact family registry baseline

Exit criteria:

- session resume works
- compaction writes a deterministic output
- event log covers turns and tools

## Milestone 2: Permission And Tool Policy

Goal:

Make tool execution safe and predictable.

Deliverables:

- `PermissionMode`
- tool classification
- workspace write boundary checks
- promptable approvals in TUI
- provider/auth resolution surface
- config precedence/effective-view surface
- shell/file/web tool baseline

Exit criteria:

- read-only and write tools are clearly separated
- out-of-workspace writes are blocked or escalated
- permission regression suite passes

## Milestone 3: Repo Governance And Review Protocols

Goal:

Install the anti-chaos and independent-review contracts before multi-agent
execution starts producing lots of outputs.

Deliverables:

- `ArtifactFamily`
- canonical/latest/archive pointers
- append/fork/supersede decision engine
- `ReviewPacket`
- review trace writer
- cleanup-plan / cleanup-apply flow
- silent-summary candidate/review/promote queue
- fixed-name latest plus timestamped history policy

Exit criteria:

- every mutable output lands in an artifact family
- every external review produces a traceable packet and trace
- downstream reads resolve canonical outputs deterministically

## Milestone 4: Base CLI Parity And Operator Contracts

Goal:

Reach strong general-purpose code-agent quality before custom layers, and do
not leak advanced command shells ahead of their backing state machines.

Deliverables:

- operator command contracts and interactive discoverability skeleton; full
  responsive TUI panes are deferred to the host-surface milestone with remote
  control so local TUI, web, and mobile views share one runtime contract
- tool streaming
- resume and inspect commands
- compact and doctor commands
- project browse/init/status commands
- session title, rename/delete, lineage-resolution, and resume-recap semantics
- usage/cost/stats surfaces
- slash/help/command-palette semantics
- config precedence and provider/auth routing semantics
- golden operator fixtures for config/provider/session/slash/permission behavior
- MCP registry management
- MCP inspect/test/refresh semantics
- skill registry list/inspect/validate semantics
- plugin lifecycle health surface
- setup status / migrate-check / repair-hints / install-routes surfaces
- provider inspect and provider catalog refresh semantics
- boot preflight / doctor contract and degraded feature reporting
- session identity / lineage / workspace-mismatch hardening
- provider credential-shape hints and provider-prefix routing parity
- full operator contract for all shipped base commands, including exit codes,
  JSON envelopes, emitted events, and feature-graduation behavior
- executable kernel submachine transition tables and cross-machine invariants

Exit criteria:

- day-to-day code-agent workflow feels competitive with Hermes / Claw / Crush
- user can inspect session, tools, permissions, and failures without digging
- boot failures, config ambiguity, provider/auth mistakes, and degraded plugin/MCP
  state are machine-explainable rather than log-scraped
- every base-CLI command family is frozen by operator contract and backed by
  conformance fixtures

## Milestone 5: Host Surface And Remote Operator Plane

Goal:

Add local TUI plus mobile/web/desktop remote follow and control without creating
a second runtime.

Deliverables:

- shared host-surface state model for local TUI, web, and mobile projections
- local terminal TUI panes for session timeline, tool stream, permissions,
  project status, memory, branches, reviews, and research-stage state
- inline REPL enhancements inspired by claw-code: status HUD, live markdown,
  collapsible tool output, colored diffs, pager, session picker, command
  completion, permission overlay, and theme capability
- optional full-screen split-pane TUI after the inline mode is stable, with
  conversation, input/editor, sidebar/status, tool, diff, memory, branch,
  review, research-stage, and diagnostics panes
- `HostSurfaceProjection` and `HostSurfaceAction` schemas so local TUI,
  browser, and mobile clients share one projection/action contract
- Happy-based remote host compatibility layer for mobile/web follow + takeover
- remote capability publication and control-owner visibility
- remote permission-response and message-steering control path
- Tailscale-only remote transport for this product line; no self-hosted public
  relay is required or implied
- daemon APIs for `HostSurfaceProjection`, `HostSurfaceAction`, and governed
  terminal attach/replay so the PWA consumes the same contract as local TUI
- pairing, lease, reconnect, revocation, and offline-action policy
- semantic app-server lane for structured sessions, turns, approvals, model
  controls, and project projections
- governed PTY/xterm lane for exact CLI rendering, scrollback replay,
  resize/input/signal, and mobile extra keys
- runtime descriptor publication and capability-family advertisement
- local↔remote switching and control-epoch handback rules
- deterministic mock host harness and remote conformance fixtures

Exit criteria:

- local TUI and remote workbench read the same command registry and runtime
  projections, with no shadow UI-only state
- every local TUI pane has a JSON projection fixture and can be tested without
  requiring a real terminal
- permission, terminal, publication, branch, memory, and research actions from
  the TUI go through the same typed gates as CLI and remote clients
- one live session can be safely followed from a remote client without splitting runtime truth
- remote/mobile/web control remains projection-driven rather than becoming a
  second runtime
- pairing/auth/reconnect/revocation semantics are machine-testable rather than
  implied by UI behavior

## Milestone 6: Multi-Agent Core

Goal:

Add controlled multi-agent execution, not swarm chaos.

Deliverables:

- agent roles
- `.pmcli/agents/<agent_id>/` protocol
- `TaskPacket` schema and `.pmcli/agents/<agent_id>/TASK_PACKET.json` persistence contract
- per-agent workspace binding
- worktree refresh/reconcile/GC policy
- agent spawn/list/inspect/stop/traces
- director-only promotion authority
- reviewer blinding
- typed task packet and agent runtime policy
- review list/inspect/open/retry operator surface

Exit criteria:

- executor and reviewer run with isolated storage
- reviewer packet excludes executor interpretation
- agent traces are resumable and inspectable

## Milestone 7: Working Memory And Silent Consolidation

Goal:

Add the bounded memory layer needed for long-horizon work.

Deliverables:

- working-memory store
- scope-local eviction
- session-end silent summary
- periodic background consolidation
- summary candidate/review/promote protocol

Exit criteria:

- working memory stays bounded
- summaries persist across sessions
- no transcript replay is needed for normal resume

## Milestone 8: Durable Project Memory And Active Query

Goal:

Add project-level memory that is useful without becoming noise.

Deliverables:

- `MemoryRecord`
- promotion/demotion
- invalidation
- forgetting/decay
- retrieval budgets
- durable recall cache
- query/explain path for injected memory
- retrieval ranking policy and explain-backed auto-injection
- promotion authority and contested/superseded visibility rules

Exit criteria:

- retrieval is scoped and bounded
- stale or superseded records stop auto-injecting
- migration events can invalidate earlier memory
- `memory query/explain/status/invalidate` are operator-grade and conformance-backed

## Milestone 9: Branch Search And Debate

Goal:

Implement budgeted multi-branch exploration and comparison.

Deliverables:

- `SearchBatch`
- `BranchRun`
- `EvaluationContract`
- `DebatePacket`
- `DebateTrace`
- branch workspace allocator
- score comparison and archive flow
- scheduler quotas, retry semantics, and evaluation packet schema
- unique-winner promotion rule and veto-aware adjudication
- branch list/inspect/cancel/promote operator surface
- stale-base visibility and promotion refusal contracts

Exit criteria:

- branch count and concurrency are bounded
- winner promotion is explicit
- losers are archived, not merged accidentally
- debate state is inspectable and disagreement traces are auditable

## Milestone 10: Research Workflow Engine

Goal:

Build the research stack on top of the stable kernel.

Deliverables:

- stage graph for research skills
- skill manifest discovery registry
- skill contracts with input/output artifacts
- research thread and deliberation-span state for long conversations at any
  stage of the scientific process
- design/doc management stage
- implementation stage before experiments
- planner/executor/publisher split
- experiment result ingestion into memory and artifacts
- repair-and-rerun edges from result-to-claim back into docs/code/plan
- nonlinear `ResearchStageExecution` graph with `advance`, `retry`, `repair`,
  `pivot`, `fork`, `supersede`, `abandon`, and `human_override` operations
- M9-backed `fork` semantics for competing plans, implementations, repairs,
  experiment designs, and research narratives
- queue-vs-direct experiment routing semantics
- integrity-aware result-to-claim gate
- supervisor lease / heartbeat / wake-queue semantics for queued runs
- research-native remote workbench panels over the Happy host substrate
- attach/handoff/terminal/ownership contracts hardened with Happier-style
  capability gating
- remote pair/status/attach/takeover/notify command semantics and golden fixtures
- stage execution policy, retry classes, repair edges, and role authority rules
- CLI research status/inspect/decide/record surfaces so the operator can see
  and correct inferred research position without knowing internal stage names
- advisory `ResearchTurnClassification` with evidence, confidence,
  alternatives, and natural-language confirmation gates before high-impact
  operation edges
- M10-owned research-runtime DocFrame publication for governed `supersede`
  operations

Exit criteria:

- research workflows reuse kernel primitives
- no research stage writes undocumented outputs
- long research conversations resume with the correct active thread, stage,
  deliberation mode, open questions, agreed decisions, and recommended next
  action
- low-confidence or high-impact natural-language classifications pause as
  pending operations rather than mutating public docs, code, experiments, or
  irreversible DAG edges
- failed results route deterministically into doc/code/plan repair before rerun
- research graph history distinguishes retry, repair, pivot, fork, supersede,
  abandon, and human override rather than flattening them into a linear rerun
- forked research directions use M9 search/evaluation/debate/promotion before
  updating the public canonical project surface
- research ops semantics are concrete enough to match ARIS-grade operation
- remote clients can inspect research state, branches, and ProjectOps without bypassing kernel invariants
- remote attach/handoff/terminal behavior is explicit, explainable, and policy-bounded
- remote operator flows are fixture-tested alongside the base kernel parity suite

## Milestone 11: Meta-Optimization And Self-Improvement

Goal:

Use accumulated traces to improve skill behavior and routing, and graduate
skill outputs into a universal publication runtime.

Deliverables:

- meta-optimization logs
- reviewer-gated skill patch suggestions
- usage analytics over sessions, branches, and reviews
- proactive project-ops triggers, digest promotion analytics, and cleanup governance metrics
- general `SkillOutputEnvelope` ingress for every skill-produced plan, review,
  report, handoff, experiment result, patch, figure, dataset, and paper section
- publication gate that turns private candidates into review candidates or the
  single public-latest artifact for an artifact family
- `SkillManifest` and `ResearchSkillContract` extensions for output kinds,
  DocFrame requirements, canonicality policy, and human-gate policy
- `skills outputs list`, `skills output submit`, `skills output inspect`,
  `skills publish --inspect`, `skills publish --execute`, and
  `docs publish-candidate` operator surfaces
- proof that direct public writes by skills are rejected unless they pass
  envelope validation and the DocFrame publication gate
- prompt/resume/compact behavior that reads public-latest skill outputs by
  default and includes private candidates only by explicit operator selection

Current implementation status:

- implemented: `SkillOutputEnvelope` candidate ingress, output listing,
  output inspection, publication inspect/execute, artifact-family latest update,
  and `.pmcli/canonical_surface.json` `skill_outputs` publication
- implemented: schema coverage for skill output artifacts, envelopes, list,
  inspection, submit, publication result, and publication gate
- implemented: multi-artifact envelope submission and publication-gate
  enforcement for explicit human approval plus non-public policies
- implemented: contract-aware output-kind/DocFrame/policy/human-gate submit
  validation, native `docs publish-candidate` ingress, and prompt/resume/compact
  public-latest-only skill output context projection
- implemented: local command skill runner adapter with file-level side-effect
  audit, contract `allowed_write_scopes` enforcement, and governed envelope
  submission
- implemented: specialized DocFrame-publication schema for skill-generated docs
- not yet implemented: non-local adapter families

Exit criteria:

- self-improvement proposals are auditable
- no automatic behavior drift without review

## Milestone 12: Verifier Tournament And Decision Quality

Goal:

Make branch and research-decision selection less dependent on a single reviewer
sample while preserving deterministic gates as the only authority that can
change project truth.

Deliverables:

- `VerifierCriterion`
- `VerifierPairScore`
- `VerifierTournament`
- criterion-level evidence requirements and ground-truth notes
- repeated pairwise scoring across candidate branches
- cacheable evidence digest per pair/criterion/repetition trace
- local deterministic verifier baseline that scores evaluation packets, debate
  traces, canonicality, and implementation risk
- `branches verify <branch-id> --against <branch-id>` operator surface
- `branches inspect` projection of the latest verifier tournament
- `branches promote --require-verifier` gate that refuses promotion unless the
  verifier winner matches the promoted branch

Current implementation status:

- implemented: M12 branch-promotion verifier schemas, deterministic local
  scoring, persisted tournament records under `.pmcli/branches/verifier_tournaments/`,
  branch-run tournament references, inspect projection, help surface update,
  and promotion-time `--require-verifier` enforcement
- not yet implemented: LLM/logprob verifier providers and M10
  research-decision tournament wiring

Exit criteria:

- verifier preference remains advisory until deterministic eval, canonicality,
  and merge/publication gates pass
- every score is tied to explicit criteria and an evidence digest
- repeated pairwise scores are machine-readable and fixture-tested
- branch promotion can opt into a verifier gate without weakening existing M9
  deterministic promotion rules

## Milestone 13: Research UX Middleware Runtime

Goal:

Turn research-agent UX from prompt conventions into a typed runtime middleware
that helps the CLI remember the current scientific role, expose the right tools,
reflect on stage progress, pause for the user when needed, and recover from tool
failures without mutating project truth directly.

Design principle:

M13 is one coherent middleware loop, not a set of disconnected conveniences:

`ResearchRoleTemplate -> ToolSelectionTrace -> StageReflectionPacket -> HITL gate / tool recovery`

This loop sits between the M10 research workflow engine and future M14 local TUI
and remote control surfaces. It may create persisted advisory records and
pending gates, but it cannot publish docs, merge code, promote branches, or
change canonical project state without the existing M3/M9/M10/M11 gates.

Deliverables:

- typed `ResearchRoleTemplate` records for planner, researcher, coder,
  debugger, analyst, writer, verifier, and operator roles
- deterministic role-scoped capability projection that narrows candidate tools
  and MCP/skill exposure without granting authority beyond existing policy
- auditable `ToolSelectionTrace` with requested role, candidate tools,
  selected tools, filtered tools, always-include tools, selection strategy, and
  fallback reason
- typed `StageReflectionPacket` that records completed work, unmet success
  signals, suggested stage modifications, skill suggestions, new-stage
  candidates, todo updates, and next action
- `HitlPendingGate` records for clarification, approval, and recovery decisions,
  including typed options and resume policy
- structured `ToolErrorRecovery` records for failed tools, retryability,
  suggested alternatives, and whether human recovery is required
- `research middleware plan` operator surface that builds the full middleware
  packet for the active or specified research thread/stage
- status/inspect projection of pending middleware gates so future TUI and remote
  clients can render and resume them through the same runtime contract
- JSON schemas and operator fixtures for every M13 packet type

Reference absorption:

- From EvoScientist, absorb six-role research UX, planner reflection JSON,
  adaptive tool selection with always-included thinking/delegation tools,
  ask_user/HITL interrupts, and tool-error-to-agent-message recovery.
- Improve on EvoScientist by persisting every decision as schema-bound records,
  enforcing role-scoped authority ceilings, distinguishing clarification,
  approval, and recovery gates, and keeping fallback behavior auditable rather
  than silently fail-open.

Exit criteria:

- a research turn can be mapped to a role, bounded tool set, stage reflection,
  and pending human gate without changing public project truth
- selected and filtered tools are explainable and inspectable
- heavy/runtime-only behavior is represented lazily as planned capabilities,
  not initialized during lightweight CLI inspection
- tool failures become structured recovery choices rather than opaque stderr
- M14 host surfaces can consume M13 packets without inventing UI-only state

## Recommended Validation Gates

Before moving from one milestone to the next, require:

- passing tests for the milestone's module set
- smoke run in a real repo
- resume/recovery check if sessions are involved
- doctor output clean enough for the new surface

## What Not To Do

Do not start with:

- full research pipeline
- full long-term memory
- multi-agent branch search
- repo auto-cleanup

Those should sit on top of a validated kernel, not substitute for it.

## Immediate Next Implementation Slice

If coding starts next, the first slice should be:

1. workspace module
2. session store
3. event log
4. permission policy
5. artifact family registry
6. tool registry
7. shell CLI + minimal TUI shell

That is the smallest slice that proves this project is becoming an excellent
code agent CLI rather than only a design exercise.
