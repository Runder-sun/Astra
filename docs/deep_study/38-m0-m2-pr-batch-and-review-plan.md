# M0-M2 PR Batch And Review Plan

This document translates the M0-M2 foundation work into mergeable PR batches.

It sits on top of:

- `31-kernel-foundation-implementation-plan.md`
- `32-first-build-task-pack-backlog.md`
- `36-repo-bootstrap-and-source-tree-manifest.md`
- `37-kernel-type-and-interface-manifest.md`
- `44-base-cli-superiority-proof-gate.md`
- `45-base-cli-proof-execution-matrix.md`

Those documents explain what must exist.

This document explains how that work should actually enter the repository
without losing architectural discipline.

Additional purpose:

- make each M0-M2 batch legible as progress toward base-CLI proof rather than
  only generic implementation progress

Language transition note:

- the active implementation language is now Rust
- `36-repo-bootstrap-and-source-tree-manifest.md` plus
  `46-rust-kernel-pivot-and-bootstrap.md` define the live source-tree mapping
- older Go-style path references in this document should be read as logical
  ownership labels, not as the current canonical file layout

## 1. Purpose

The implementation phase should not merge giant milestone branches.

That creates exactly the failure mode we are trying to avoid:

- hidden second truths
- silent interface drift
- unreviewable protocol changes
- "it kind of works" merges without fixture proof

So M0-M2 should land as a sequence of small but complete PR batches.

Each batch must have:

- one clear theme
- one clear reviewer question
- one clear demo gate
- one clear rollback boundary

## 2. Batch Design Rules

Every PR batch must obey these rules:

1. never mix new runtime truth and late-stage ergonomics in one PR
2. schema owners merge before features depending on them
3. CLI grammar changes must update golden fixtures in the same PR
4. no batch may add a second package that could plausibly own the same concept
5. a batch is not complete until its demo path is runnable from a clean temp
   project

## 3. Recommended Batch Sequence

Recommended M0-M2 sequence:

1. Batch 0 - Repository skeleton and bootstrap compile
2. Batch 1 - Kernel bundle and canonical event floor
3. Batch 2 - Workspace resolution, project registry, and `.pmcli/` bootstrap
4. Batch 3 - Session store and transcript floor
5. Batch 4 - Project and session operator surfaces
6. Batch 5 - Shared `prompt`/`chat`/`resume` runtime lane
7. Batch 6 - Inspect, compact, doctor, smoke
8. Batch 7 - Permission machine and tool registry
9. Batch 8 - Provider resolution, config precedence, and accounting surfaces
10. Batch 9 - Setup and MCP baseline
11. Batch 9.5 - Plugin and hook registry baseline
12. Batch 10 - Schema, conformance, and golden CI enforcement

This order is stricter than a generic backlog because it protects the true
authorities:

- bundle truth
- event truth
- project/session truth
- operator contract truth

## 4. Batch Specifications

## Batch 0 - Repository Skeleton And Bootstrap Compile

Purpose:

- create the first compilable source tree without business logic sprawl

Primary files:

- `Cargo.toml`
- `src/bin/research-cli.rs`
- `src/app/mod.rs`
- package skeletons under `src/`
- `tests/conformance/runtime/bootstrap.rs`

Must prove:

- the binary boots
- package ownership matches `36`
- nothing outside the approved tree is introduced

Primary reviewer question:

- did we create the exact source-tree and package boundaries the architecture
  expects?

Merge gate:

- bootstrap test passes
- repository layout matches `36`

Rollback boundary:

- pure scaffold rollback must not affect persisted runtime data

## Batch 1 - Kernel Bundle And Canonical Event Floor

Purpose:

- establish the singular persisted runtime and singular persisted event object

Primary files:

- `internal/runtime/checkpoint.go`
- `internal/runtime/reducer.go`
- `internal/events/events.go`
- `internal/events/writer.go`
- `schemas/kernel_state_bundle.schema.json`
- `schemas/event.schema.json`

Must prove:

- `.pmcli/project_state.json` stores `KernelStateBundle`
- all persisted events use `KernelEventEnvelope`
- checkpoint publication is reducer-owned rather than last-writer-wins
- no second event or state object is introduced

Primary reviewer question:

- is runtime truth singular and schema-owned from the first real PR?

Demo gate:

- create a temp project
- write a bundle checkpoint
- append a `runtime_preflight` and `session_open` event
- simulate a stale publish attempt and verify reload/replay behavior
- reload both successfully

Rollback boundary:

- revert restores pre-feature scaffold only

## Batch 2 - Workspace Resolution, Project Registry, And `.pmcli/` Bootstrap

Purpose:

- make project identity deterministic and safe

Primary files:

- `internal/workspace/resolve.go`
- `internal/workspace/hash.go`
- `internal/projects/registry.go`
- `internal/projects/current.go`
- project bootstrap helpers in `internal/app/`
- workspace tests under `tests/conformance/runtime/`

Must prove:

- cwd and explicit path resolve to one project binding
- workspace hash is stable
- project registry entries and current-project pointer persist outside `.pmcli/`
- `data_dir` defaults to `<workspace_root>/.pmcli`
- forbidden/system paths are rejected
- symlink escapes are rejected

Primary reviewer question:

- can project scope ever become ambiguous, unsafe, or host-destructive?

Demo gate:

- clean temp repo resolves correctly
- register project and set current-project pointer
- forbidden path returns machine-readable failure
- `.pmcli/` bootstrap lands in the correct project root

Rollback boundary:

- no existing session or transcript semantics are changed

## Batch 3 - Session Store And Transcript Floor

Purpose:

- establish project-scoped session identity and typed transcript persistence

Primary files:

- `internal/session/store.go`
- `internal/session/transcript.go`
- `internal/session/index.go`
- `internal/session/identity.go`
- `internal/session/lineage.go`
- `schemas/session.schema.json`
- session tests

Must prove:

- create/list/load/delete session works
- session identity and lineage are explicit rather than inferred from browse UX
- transcript lines are typed and append-only
- session search remains rebuildable from canonical session artifacts
- project scope is preserved during resume

Primary reviewer question:

- is session truth singular and reloadable without reading logs manually?

Demo gate:

- create session
- append `control`, `message`, and `summary_reference` transcript lines
- reload the same session from disk

Rollback boundary:

- bundle and event persistence remain compatible

## Batch 4 - Project And Session Operator Surfaces

Purpose:

- make the promised `projects *` and `sessions *` surface real instead of
  contract-only

Primary files:

- `cmd/research-cli/main.go`
- `internal/projects/registry.go`
- `internal/session/store.go`
- `internal/session/index.go`
- `tests/golden/projects/`
- `tests/golden/sessions/`

Must prove:

- `projects list/current/register/init/status/prune` have explicit owners
- `sessions list/browse/search/export/rename/delete/prune/stats` have explicit owners
- browse/search/recap behavior is owned by one rebuildable session index
- `ProjectRegistryList` and `ProjectStatus` are the only machine-readable
  aggregate payloads for project list/current/status lanes
- project/session JSON output is deterministic and fixture-backed

Primary reviewer question:

- are project/session management claims actually entering implementation, not
  just product contracts?

Demo gate:

- register two projects
- switch current-project pointer
- list projects by recency
- list and search sessions within one project scope

Rollback boundary:

- registry and session browsing rollback must not alter canonical bundle/event
  truth

## Batch 5 - Shared Prompt/Chat/Resume Runtime Lane

Purpose:

- prevent one-shot and interactive modes from diverging into separate runtimes

Primary files:

- `cmd/research-cli/main.go`
- `internal/commands/registry.go`
- `internal/commands/help.go`
- `internal/tui/repl.go`
- `internal/tui/palette.go`
- `internal/runtime/runtime.go`
- `internal/runtime/preflight.go`
- transcript and event wiring as needed

Must prove:

- `prompt`, `chat`, `resume`, and `continue` use the same turn machinery
- REPL slash/help and command-palette discoverability use one canonical command
  registry
- runtime preflight is emitted and inspectable before expensive execution starts
- canonical events are emitted in the right order
- interruption-safe turn completion exists at baseline

Primary reviewer question:

- do all core operator modes ride one runtime lane?

Demo gate:

- run one prompt turn
- continue in chat
- resume the session
- open `/help` and confirm the same registry backs human and machine-readable
  help output
- verify one coherent session timeline

Rollback boundary:

- leaves session store intact and recoverable

## Batch 6 - Inspect, Compact, Doctor, Smoke

Purpose:

- expose operator-grade observability and recovery paths early

Primary files:

- `internal/session/compact.go`
- `internal/doctor/doctor.go`
- `internal/doctor/smoke.go`
- command wiring in `cmd/research-cli/main.go`

Must prove:

- `inspect` explains session/project state without raw log scraping
- `inspect --project` returns core payload plus explicit graduated section status
- `compact` preserves lineage and raw history
- `doctor` health lanes are returned through frozen `RuntimePreflightReport`
  and `DoctorReport` payloads rather than ad hoc text/status pairs
- `doctor` and `smoke` emit deterministic readiness states

Primary reviewer question:

- can operators understand runtime state and environment failures quickly?

Demo gate:

- create session
- compact it
- inspect it
- inspect project core state and verify non-graduated sections are labeled
- run doctor in ready and degraded conditions

Rollback boundary:

- compact summaries can be discarded without transcript loss

## Batch 7 - Permission Machine And Tool Registry

Purpose:

- make the kernel safe enough for real coding actions

Primary files:

- `internal/permissions/policy.go`
- `internal/permissions/requests.go`
- `internal/runtime/features.go`
- `internal/tools/registry.go`
- `internal/tools/shell.go`
- `internal/tools/file.go`
- `internal/tools/web.go`

Must prove:

- permission requests persist and resolve once
- permission decision traces and degraded feature states are machine-readable
- write/destructive tools are policy-gated
- path boundary checks actually block invalid writes

Primary reviewer question:

- is tool execution safe by construction rather than by convention?

Demo gate:

- read-only tool succeeds under safe mode
- mutating tool asks for approval
- denied request blocks execution
- expired request cannot be reused

Rollback boundary:

- removing this batch must not corrupt sessions or bundle truth

## Batch 8 - Provider Resolution, Config Precedence, And Accounting Surfaces

Purpose:

- make provider/model/config behavior explicit and inspectable
- make `usage`, `cost`, and `stats` explicit implementation targets

Primary files:

- `internal/providers/resolve.go`
- `internal/providers/auth.go`
- `internal/providers/trace.go`
- `internal/telemetry/usage.go`
- `internal/telemetry/stats.go`
- `internal/config/config.go`
- `internal/config/sources.go`
- `tests/mock_provider/`
- `scripts/run_mock_parity_harness.sh`

Must prove:

- config precedence follows the frozen order
- explicit provider beats ambient credentials
- provider routing decisions are returned as `ProviderResolutionTrace`, not only
  as final route strings
- failures explain auth and routing reasons concretely
- `model list/current`, `providers list`, and `config get` return frozen
  aggregate payloads rather than ad hoc command-local JSON shapes
- `usage`, `cost`, and `stats` produce machine-readable accounting output
- accounting/config outputs are backed by frozen `EffectiveConfigReport`,
  `UsageSummary`, and `StatsSummary` payloads
- provider/session parity can be exercised without a live vendor dependency

Primary reviewer question:

- if a turn chooses the wrong model/provider, can the operator explain why?

Demo gate:

- run `config effective`
- run `config sources`
- run `providers auth-status`
- run `providers test` with one passing and one blocked provider
- run `usage`, `cost`, and `stats` with explicit accounting-quality labels
- run mock-provider parity scenarios for provider/auth/session routing

Rollback boundary:

- command parsing and session store remain stable

## Batch 9 - Setup And MCP Baseline

Purpose:

- graduate setup/MCP from placeholders into first-class operator surfaces

Primary files:

- `internal/mcp/registry.go`
- `internal/mcp/test.go`
- setup-related command handlers

Must prove:

- `setup status` and `setup repair-hints` are real
- `mcp list/inspect/test/refresh` exist at baseline quality
- setup/MCP lanes are backed by frozen `SetupStatusReport`,
  `MigrateCheckReport`, `RepairHintsReport`, `InstallRoutesReport`, and
  `MCPServerRecord` payloads
- degraded features surface through doctor/status pathways

Primary reviewer question:

- are environment and MCP problems explainable before users hit runtime failure?

Demo gate:

- run setup status on a clean project
- inspect MCP registry
- simulate one broken MCP target and surface a concrete hint

Rollback boundary:

- no corruption of core kernel/session state

## Batch 9.5 - Plugin And Hook Registry Baseline

Purpose:

- make plugin and hook product surfaces explicit instead of leaving them hidden
  behind doctor-only degraded state

Primary files:

- `internal/plugins/registry.go`
- `internal/plugins/hooks.go`
- command wiring in `cmd/research-cli/main.go`
- `tests/golden/plugins/`

Must prove:

- `plugins list/inspect/validate` are real surfaces
- `hooks list/inspect/test` are real surfaces
- degraded plugin or hook state is machine-readable

Primary reviewer question:

- are plugin and hook lifecycles now productized enough to match top-tier code
  agent CLI expectations?

Demo gate:

- list plugins
- inspect one degraded plugin
- test one hook in safe mode

Rollback boundary:

- plugin and hook productization rollback must not touch session/runtime truth

## Batch 10 - Schema, Conformance, And Golden CI Enforcement

Purpose:

- lock the floor so future work cannot regress silently

Primary files:

- `tests/conformance/runtime/`
- `tests/conformance/schemas/`
- `tests/golden/`
- CI workflow files

Must prove:

- schema validation runs in CI
- curated golden fixtures run in CI
- merge is blocked on protocol and fixture regressions

Primary reviewer question:

- will later milestones be forced to honor the contracts we just wrote?

Demo gate:

- green CI for conformance + golden suite
- one intentionally broken fixture shows expected failure mode

Rollback boundary:

- CI-only rollback should not change runtime code paths

## 5. Reviewer Assignment Guidance

Each batch should get one primary reviewer lens.

Recommended lenses:

- Batch 0-2: architecture and persistence reviewer
- Batch 3-5: operator/runtime reviewer
- Batch 6-8: safety/platform reviewer
- Batch 9: protocol/conformance reviewer

If a batch needs more than one primary reviewer lens, it is probably too large.

## 6. Required PR Template Questions

Every M0-M2 PR description should answer these questions explicitly:

1. Which canonical object or interface becomes real in this PR?
2. Which schema owners are introduced or changed?
3. Which commands or fixtures become newly supported?
4. What is the exact demo path from a clean temp project?
5. What architectural risk does this PR specifically avoid?

These questions stop implementation from drifting into vague "foundation work"
PRs.

## 7. Required Demo Matrix

Before M0-M2 is declared complete, the combined batches must demonstrate:

- boot from clean project
- deterministic project resolution
- session create and resume
- prompt/chat unified runtime
- inspect and compact
- doctor and smoke
- permission-gated tool execution
- provider/config explainability
- setup and MCP inspection
- green schema/conformance/golden CI

If any of these are still "documented but not demoable," M0-M2 is not done.

## 8. Final Rule

The only acceptable reason to deviate from this batch order is if a smaller
sequence preserves the same architectural invariants more clearly.

"Convenient for one engineer" is not enough.

"Protects runtime truth and review quality better" is enough.
