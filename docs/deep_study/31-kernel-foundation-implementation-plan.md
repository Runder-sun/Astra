# Kernel Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the M0-M2 foundation of `research-cli`: bundle/session/event persistence, base CLI entrypoints, doctor/smoke, permission state, tool registry, provider/config surfaces, and schema/conformance bootstrap.

**Architecture:** Start from one runtime truth (`KernelStateBundle`) and one protocol surface (`.pmcli/` + `schemas/`). Build the kernel and schema harness first, then layer operator commands and permission/provider behavior on top of that truth. No remote, multi-agent, or memory automation enters before the kernel floor is stable.

**Tech Stack:** Current implementation uses a Rust-first repository layout
(`Cargo.toml`, `src/`, `tests/`, `schemas/`). If later crates are introduced,
preserve the same module boundaries, protocol contracts, and test surfaces.

Language transition note:

- logical ownership in this document remains canonical
- exact Rust path mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`
- Batch 0 and all new implementation work should follow the Rust paths, not the
  earlier Go-style bootstrap paths

---

## 1. Scope

This plan covers:

- Milestone 0: Bootstrap, protocols, guardrails
- Milestone 1: Kernel session runtime
- Milestone 2: Permission, tools, providers, config

It intentionally excludes:

- remote/mobile/web runtime
- multi-agent/branch/review runtime
- ProjectOps automation
- durable project memory
- research workflow execution

Advanced schemas and type names may still be reserved earlier in `28` and `37`
so later milestones do not invent competing payloads, but those reservations do
not make remote/multi-agent/memory/ProjectOps implementation part of M0-M2.

Proof-first execution note:

- `44-base-cli-superiority-proof-gate.md` defines what evidence must eventually
  exist before claiming the base CLI exceeds the reference set
- `45-base-cli-proof-execution-matrix.md` maps that proof gate onto the M0-M2
  execution order in this document
- during M0-M2, work that unblocks Claw-class and Hermes-class proof takes
  precedence over broader advanced-surface reservation work

## 2. Required File Skeleton

Create or reserve these files first:

- `cmd/research-cli/main.go`
- `internal/app/app.go`
- `internal/commands/registry.go`
- `internal/commands/help.go`
- `internal/projects/registry.go`
- `internal/projects/current.go`
- `internal/runtime/runtime.go`
- `internal/runtime/checkpoint.go`
- `internal/runtime/preflight.go`
- `internal/runtime/features.go`
- `internal/runtime/reducer.go`
- `internal/events/events.go`
- `internal/events/writer.go`
- `internal/session/index.go`
- `internal/session/identity.go`
- `internal/session/lineage.go`
- `internal/session/store.go`
- `internal/session/transcript.go`
- `internal/session/compact.go`
- `internal/session/mission_frame.go`
- `internal/workspace/resolve.go`
- `internal/permissions/policy.go`
- `internal/permissions/requests.go`
- `internal/tools/registry.go`
- `internal/tools/shell.go`
- `internal/tools/file.go`
- `internal/tools/web.go`
- `internal/providers/resolve.go`
- `internal/providers/trace.go`
- `internal/providers/auth.go`
- `internal/config/config.go`
- `internal/config/sources.go`
- `internal/doctor/doctor.go`
- `internal/doctor/smoke.go`
- `internal/plugins/registry.go`
- `internal/plugins/hooks.go`
- `internal/telemetry/usage.go`
- `internal/telemetry/stats.go`
- `internal/tui/repl.go`
- `internal/tui/palette.go`
- `scripts/run_mock_parity_harness.sh`
- `schemas/kernel_state_bundle.schema.json`
- `schemas/project_state.schema.json`
- `schemas/event.schema.json`
- `schemas/command_success.schema.json`
- `schemas/command_failure.schema.json`
- `schemas/feature_gate_result.schema.json`
- `schemas/policy_refusal_result.schema.json`
- `schemas/followup_required_result.schema.json`
- `schemas/prune_result.schema.json`
- `schemas/permission_pending_list.schema.json`
- `schemas/permission_history_result.schema.json`
- `schemas/conformance_result.schema.json`
- `schemas/session.schema.json`
- `schemas/session_identity.schema.json`
- `schemas/session_lineage.schema.json`
- `schemas/session_summary.schema.json`
- `schemas/compact_result.schema.json`
- `schemas/mission_frame.schema.json`
- `schemas/goal_alignment_trace.schema.json`
- `schemas/session_inspection.schema.json`
- `schemas/project_inspection.schema.json`
- `schemas/current_project_pointer.schema.json`
- `schemas/project_resolution_trace.schema.json`
- `schemas/permission_decision_trace.schema.json`
- `schemas/provider_resolution_trace.schema.json`
- `schemas/runtime_feature_status.schema.json`
- `schemas/smoke_result.schema.json`
- `schemas/task_packet.schema.json`
- `tests/conformance/runtime/`
- `tests/conformance/schemas/`
- `tests/golden/config/`
- `tests/golden/plugins/`
- `tests/golden/projects/`
- `tests/golden/provider/`
- `tests/golden/sessions/`
- `tests/golden/permissions/`
- `tests/mock_provider/`

## 3. Execution Order

### Task 1: Repository bootstrap and module shell

**Files:**
- Create: `cmd/research-cli/main.go`
- Create: `internal/app/app.go`
- Create: `internal/runtime/runtime.go`
- Create: `internal/events/events.go`
- Test: `tests/conformance/runtime/bootstrap_test.go`

- [x] Define the CLI entrypoint and a minimal app bootstrap path.
- [x] Create empty-but-compilable runtime, session, and event packages.
- [x] Add one smoke test proving the binary boots and exits cleanly in no-op mode.
- [x] Add one fixture helper for temp project roots under `tests/`.

**Done when:**
- binary boot path exists
- test harness can create a temp project root

### Task 2: Kernel bundle and event envelope

**Files:**
- Create: `internal/runtime/checkpoint.go`
- Create: `internal/runtime/reducer.go`
- Modify: `internal/events/events.go`
- Create: `schemas/kernel_state_bundle.schema.json`
- Create: `schemas/event.schema.json`
- Test: `tests/conformance/runtime/kernel_bundle_test.go`
- Test: `tests/conformance/runtime/checkpoint_reducer_test.go`
- Test: `tests/conformance/schemas/event_schema_test.go`

- [x] Implement `KernelStateBundle` serialization shape.
- [x] Implement `KernelEventEnvelope` serialization and validation helpers.
- [x] Write the first checkpoint save/load path to `project_state.json`.
- [x] Implement reducer-owned checkpoint publication with stale base detection.
- [x] Add conformance tests for valid/invalid event payloads and bundle reload.

**Done when:**
- checkpoint round-trip works
- stale checkpoint publication reloads/replays instead of overwriting
- event envelope validates against schema

### Task 3: Workspace, project resolution, and registry

**Files:**
- Create: `internal/workspace/resolve.go`
- Create: `internal/workspace/hash.go`
- Create: `internal/projects/registry.go`
- Create: `internal/projects/current.go`
- Create: `internal/session/index.go`
- Modify: `internal/app/app.go`
- Test: `tests/conformance/runtime/workspace_resolution_test.go`
- Test: `tests/golden/sessions/project_scope_test.go`
- Test: `tests/golden/projects/project_registry_test.go`

- [x] Implement workspace root detection and workspace hash generation.
- [x] Implement current-project resolution rules from cwd and explicit flags.
- [x] Implement global `ProjectRegistryEntry` persistence and current-project pointer storage.
- [x] Define `data_dir` semantics and default it to `<workspace_root>/.pmcli`.
- [x] Persist project-scoped `.pmcli/` bootstrap directories.
- [x] Create the rebuildable session-search index owner and bind it to project
  scope without making it runtime authority.
- [x] Add ambiguity/error-path tests for unresolved or conflicting project scope.

**Done when:**
- current project resolution is deterministic
- cross-project registry commands have a real owner and persistence path
- `.pmcli/` bootstrap path is stable and testable

### Task 4: Session store and transcript persistence

**Files:**
- Create: `internal/session/store.go`
- Create: `internal/session/transcript.go`
- Create: `internal/session/identity.go`
- Create: `internal/session/lineage.go`
- Modify: `internal/session/index.go`
- Create: `schemas/session.schema.json`
- Create: `schemas/session_identity.schema.json`
- Create: `schemas/session_lineage.schema.json`
- Test: `tests/conformance/runtime/session_store_test.go`
- Test: `tests/golden/sessions/resume_latest_project_scoped_test.go`

- [x] Implement session identity, create/list/load/delete APIs.
- [x] Implement resumable `SessionIdentity` and `SessionLineageRecord` persistence.
- [x] Implement transcript JSONL persistence with `control`, `message`, `tool_call`, `tool_result`, and `summary_reference` line types.
- [x] Implement project-scoped session namespace behavior.
- [x] Implement rebuild and query behavior for session browse/search/title recap.
- [x] Add tests for resume, continue, and ambiguous alias refusal.

**Done when:**
- session lifecycle is persisted and reloadable
- session search remains rebuildable from canonical session artifacts
- transcript lines validate against schema

### Task 4.5: Project and session operator surfaces

**Files:**
- Modify: `cmd/research-cli/main.go`
- Modify: `internal/projects/registry.go`
- Modify: `internal/session/store.go`
- Test: `tests/golden/projects/`
- Test: `tests/golden/sessions/`

- [x] Implement `projects list`, `projects current`, `projects register`, `projects init`, `projects status`, and `projects prune`.
- [x] Implement `sessions list`, `sessions browse`, `sessions search`, `sessions export`, `sessions rename`, `sessions delete`, `sessions prune`, and `sessions stats` baseline.
- [x] Freeze `CommandSuccess` and `CommandFailure` envelopes so every base command returns one canonical machine-readable result shape.
- [x] Freeze `FeatureGateResult`, `PolicyRefusalResult`, and `FollowupRequiredResult` so advanced non-zero lanes also use canonical typed payloads.
- [x] Freeze `ProjectRegistryList`, `ProjectStatus`, and `ProjectResolutionTrace` JSON fixtures so `projects list/current/status` cannot invent ad hoc aggregate payloads during implementation.
- [x] Freeze `PruneResult` so `projects prune` dry-run/apply output is machine-readable instead of command-local JSON.
- [x] Freeze `SessionBrowseResult` and `ResumeAmbiguityResult` so browse/resume discovery stays machine-readable and ambiguity-safe.
- [x] Add JSON output fixtures for project/session list and status surfaces.
- [x] Add tests for current-project pointer fallback, cwd-vs-pointer precedence, recency ordering, and ambiguous session lookup refusal.

**Done when:**
- promised first-release project/session browsing surfaces have explicit owners
- project/session operator JSON is fixture-backed

### Task 5: One-shot turn path and interactive shell skeleton

**Files:**
- Modify: `cmd/research-cli/main.go`
- Create: `internal/commands/registry.go`
- Create: `internal/commands/help.go`
- Modify: `internal/runtime/runtime.go`
- Create: `internal/runtime/preflight.go`
- Modify: `internal/session/transcript.go`
- Create: `internal/tui/repl.go`
- Create: `internal/tui/palette.go`
- Test: `tests/conformance/runtime/turn_lifecycle_test.go`
- Test: `tests/golden/sessions/prompt_mode_test.go`

- [x] Implement `prompt` one-shot execution shell with stubbed provider/tool path.
- [x] Implement `chat`, `resume`, and `continue` routing to the same runtime lane.
- [x] Implement one canonical slash/help command registry shared by REPL and
  top-level CLI help rendering.
- [x] Implement baseline `/help`, `/status`, `/doctor`, `/session`, `/projects`,
  `/permissions`, `/usage`, and `/cost` discoverability surfaces.
- [x] Implement a command-palette/TUI discoverability skeleton that reads from
  the same registry rather than a second command list.
- [x] Freeze `HelpSurfaceReport` and `PaletteSurfaceReport` so machine-readable help and palette exports cannot diverge from the canonical command registry.
- [x] Freeze `ProfileResolutionTrace`, `InterruptResult`, `RetryResult`, and `UndoRefusal` payloads so startup and recovery semantics are schema-owned before richer UX is layered on top.
- [x] Add profile-aware startup fixtures for resolved profile tracing and conflict hints.
- [x] Add startup and prompt fixtures proving `InteractiveLaunchResult` and `TurnResult` expose project/profile/provider routing traces without extra commands.
- [x] Enforce that `research-cli --json` and `chat --json` return launch inspection and exit before any REPL output.
- [x] Implement `RuntimePreflightReport` emission before turn/session execution.
- [x] Load and inject the project `MissionFrame` projection into prompt,
  resume, and continue lanes when a frame exists.
- [x] Emit canonical `command`, `turn`, `session_open`, `session_resume`, and `runtime_preflight` events.
- [x] Add tests for interruption-safe turn completion and persisted transcript output.

**Done when:**
- one-shot turn and interactive shell share one session runtime
- REPL slash/help discoverability is owned by one registry
- event order is deterministic
- prompt/resume context carries the same project goal hierarchy when available

### Task 6: Inspect, compact, doctor, and smoke

**Files:**
- Create: `internal/session/compact.go`
- Create: `internal/doctor/doctor.go`
- Create: `internal/doctor/smoke.go`
- Modify: `internal/session/lineage.go`
- Create: `schemas/session_summary.schema.json`
- Create: `schemas/compact_result.schema.json`
- Create: `schemas/session_inspection.schema.json`
- Create: `schemas/project_inspection.schema.json`
- Modify: `cmd/research-cli/main.go`
- Test: `tests/golden/sessions/compact_test.go`
- Test: `tests/golden/provider/doctor_provider_matrix_test.go`

- [x] Implement `inspect` current-session baseline.
- [x] Implement `inspect --project` core payload only, with graduated section-status fields.
- [x] Implement `compact` writing summary refs and compacted markdown view.
- [x] Preserve `MissionFrame` through compact by storing a `mission_frame_ref`
  and copying only a bounded immutable projection into compacted markdown.
- [x] Emit `GoalAlignmentTrace` when compacted work appears to drift from the
  project max goal, milestone goal, or current implementation goal.
- [x] Implement compaction lineage publication and recap-safe resume metadata.
- [x] Implement `doctor` and `smoke` with read-only-safe preflight checks.
- [x] Freeze `RuntimePreflightReport`, `DoctorReport`, and `SmokeResult` JSON fixtures so health reporting and end-to-end smoke stay machine-contract-backed from the first shipped baseline.
- [x] Add fixtures for blocked/degraded/ready doctor states and for mutation-free smoke execution.
- [x] Add compaction fixtures for refreshed derived views and `11`-lane deferred repair visibility.
- [x] Require `CommandFailure.data` coverage for blocked doctor, ambiguous resume, and unresolved project cases.
- [x] Extend `CommandFailure.data` coverage to `not_graduated` and degraded-but-loadable exit `11` lanes.

**Done when:**
- operator can inspect and compact without logs
- doctor and smoke emit machine-explainable failures or blocked outcomes

### Task 7: Permission state machine and tool registry

**Files:**
- Create: `internal/permissions/policy.go`
- Create: `internal/permissions/requests.go`
- Create: `internal/runtime/features.go`
- Create: `internal/tools/registry.go`
- Create: `internal/tools/shell.go`
- Create: `internal/tools/file.go`
- Create: `internal/tools/web.go`
- Create: `schemas/permission_decision_trace.schema.json`
- Create: `schemas/runtime_feature_status.schema.json`
- Test: `tests/conformance/runtime/permission_machine_test.go`
- Test: `tests/golden/permissions/permission_mode_test.go`

- [x] Implement permission request/pending/granted/denied flow.
- [x] Persist `PermissionDecisionTrace` and runtime feature degradation status.
- [x] Implement tool classification: read-only, mutating, destructive.
- [x] Enforce workspace-boundary and mode checks before mutating tools run.
- [x] Bind each approved tool run to the full effective tool input, including
  command/content/path payload digest, so a stale approval cannot be replayed
  for a different mutation under the same tool name and target path.
- [x] Freeze `PermissionPendingList` and `PermissionHistoryResult` fixtures for `permissions pending/history`.
- [x] Add fixtures for approval, denial, expiry, and blocked write attempts.

**Done when:**
- tool execution is policy-gated
- permission decisions are persisted and inspectable

### Task 8: Provider resolution, auth checks, and config surfaces

**Files:**
- Create: `internal/providers/resolve.go`
- Create: `internal/providers/trace.go`
- Create: `internal/providers/auth.go`
- Create: `internal/config/config.go`
- Create: `internal/config/sources.go`
- Create: `schemas/provider_resolution_trace.schema.json`
- Modify: `cmd/research-cli/main.go`
- Create: `tests/mock_provider/README.md`
- Create: `scripts/run_mock_parity_harness.sh`
- Test: `tests/golden/config/config_precedence_test.go`
- Test: `tests/golden/provider/provider_resolution_test.go`

- [x] Implement config precedence resolution.
- [x] Implement provider auth-status and minimal test path.
- [x] Implement `ProviderResolutionTrace` emission for alias, prefix, auth-shape, and base-url decisions.
- [x] Implement config get/set/effective/sources.
- [x] Freeze `ModelCatalogList`, `ModelCurrentResult`, `ProviderStatusList`, and `ConfigValueResult` output fixtures for `model *`, `providers list`, and `config get`.
- [x] Freeze `ProviderCatalogState` and `SessionStats` fixtures so catalog freshness and session-accounting summaries stay schema-owned rather than command-local JSON.
- [x] Freeze `EffectiveConfigReport`, `UsageSummary`, and `StatsSummary` output fixtures so provider/config/accounting surfaces stay comparable to `claw-code` rather than drifting per command.
- [x] Implement deterministic mock-provider scenarios for provider/auth/session
  parity.
- [x] Add fixtures for alias resolution, explicit-provider priority, and auth-shape hints.

**Done when:**
- config/provider ambiguity is machine-explainable
- provider/auth/session parity is executable without live vendor dependence
- provider/config golden fixtures pass

### Task 8.5: Usage, cost, and stats accounting baseline

**Files:**
- Create: `internal/telemetry/usage.go`
- Create: `internal/telemetry/stats.go`
- Modify: `cmd/research-cli/main.go`
- Test: `tests/golden/provider/`
- Test: `tests/golden/sessions/`

- [x] Implement session and project usage accumulation baseline.
- [x] Implement `usage`, `cost`, and `stats` machine-readable output with accounting-quality labels.
- [x] Distinguish exact from estimated accounting in the returned payloads.
- [x] Add fixtures for empty usage, active-session usage, and degraded accounting cases.

**Done when:**
- `usage`, `cost`, and `stats` are no longer contract-only promises
- accounting quality is explicit in operator output

### Task 9: Setup and MCP baseline

**Files:**
- Modify: `cmd/research-cli/main.go`
- Create: `internal/mcp/registry.go`
- Create: `internal/mcp/test.go`
- Test: `tests/golden/provider/setup_status_test.go`
- Test: `tests/golden/provider/mcp_refresh_test.go`

- [x] Implement `setup status`, `setup migrate-check`, `setup repair-hints`, `setup install-routes`.
- [x] Implement `mcp list`, `mcp inspect`, `mcp test`, `mcp refresh` baseline.
- [x] Freeze `SetupStatusReport`, `MigrateCheckReport`, `RepairHintsReport`, `InstallRoutesReport`, `MCPListResult`, `MCPInspectionResult`, `MCPTestResult`, and `MCPRefreshResult` fixtures before wiring degraded-state behavior into doctor/setup.
- [x] Add setup fixtures for payload completeness and platform-specific install-route visibility.
- [x] Wire setup/MCP surfaces into doctor and degraded feature reporting.

**Done when:**
- setup and MCP are real operator surfaces, not placeholders

### Task 9.5: Plugin and hook registry baseline

**Files:**
- Modify: `cmd/research-cli/main.go`
- Create: `internal/plugins/registry.go`
- Create: `internal/plugins/hooks.go`
- Test: `tests/golden/plugins/plugin_registry_test.go`

- [x] Implement `plugins list`, `plugins inspect`, and `plugins validate`
  baseline.
- [x] Implement `hooks list`, `hooks inspect`, and `hooks test` baseline.
- [x] Freeze `PluginListResult`, `PluginInspectionResult`,
  `PluginValidationResult`, `HookListResult`, `HookInspectionResult`, and
  `HookTestResult` fixtures before wiring degraded-state behavior into doctor.
- [x] Wire plugin and hook degradation into doctor and setup repair hints.

**Done when:**
- plugin and hook product surfaces have explicit owners
- degraded plugin state is machine-readable rather than hidden in logs

### Task 10: CI floor for M0-M2

**Files:**
- Create: `tests/conformance/run_conformance.sh`
- Create: `tests/golden/run_golden.sh`
- Create: `.github/workflows/pr-fast.yml` or equivalent CI file
- Test: all above

- [x] Add schema validation job.
- [x] Add runtime conformance job.
- [x] Add curated golden fixture job for M0-M2.
- [x] Include `research-cli conformance --json` payload validation in the CI floor.
- [x] Add fixtures for `projects prune`, `permissions pending/history`, and
  `research-cli conformance` typed payloads.
- [x] Block merge on failing protocol/golden tests.

**Done when:**
- M0-M2 can regress only through visible fixture failures

## 4. Exit Criteria

This plan is complete when:

- kernel bundle/event/session truth is live
- prompt/chat/resume/continue/inspect/compact/doctor/smoke work
- permission/tool/provider/config/setup/MCP/plugin/hook surfaces work at
  baseline quality
- schema and golden tests exist and run in CI

## 5. Do Not Start Yet

Before this plan passes, do not start:

- remote pair/attach/handoff/takeover
- agent spawn/branch/review runtime
- durable memory promotion
- ProjectOps cleanup automation
- research DAG execution

Those layers depend on this foundation being stable.
