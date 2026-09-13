# Fixture And CI Rollout Plan

This document turns the contract set into an executable verification program.

`17-operator-golden-fixtures.md` defines what behavior must be true.

This document defines how those fixtures land in the repository and how CI
should enforce them.

## 1. Verification Philosophy

The project should never claim parity or superiority through prose alone.

Every meaningful operator behavior must be backed by one of:

- a golden fixture
- a protocol conformance test
- a deterministic mock-provider parity harness
- a deterministic remote harness test
- a recovery/migration replay test

## 2. Test Layers

## Layer A: Schema conformance

Purpose:

- validate persisted object structure before runtime behavior

Lives in:

- `tests/conformance/schemas/`

Must cover:

- valid payloads
- invalid payloads
- additive-field tolerance
- previous-line readability

## Layer B: Kernel protocol tests

Purpose:

- validate bundle/checkpoint/session/event behavior

Lives in:

- `tests/conformance/runtime/`

Must cover:

- checkpoint write/reload
- lock order and conflict handling
- compaction recovery
- permission request lifecycle
- event ordering and terminality
- duplicate terminal-event reconciliation
- typed uncertainty after transport loss
- provenance and scope labeling on emitted events

## Layer C: Golden operator fixtures

Purpose:

- validate user-facing CLI/TUI semantics

Lives in:

- `tests/golden/`

Must cover:

- config/provider/auth
- sessions/resume/compact
- slash/help/permissions
- memory query/explain
- agents/branches/reviews
- setup/MCP/doctor

## Layer D: Remote harness

Purpose:

- validate Happy-based remote control as a projection plane, not a second runtime

Lives in:

- `tests/remote_harness/`
- `tests/golden/remote/`

Must cover:

- pair
- attach
- handoff
- takeover
- inspect-vs-execute grammar
- lease expiry
- reconnect replay
- offline-action refusal
- local keypress reclaim and ownership-epoch advancement
- typed notification payload plus receipt linkage
- tagged-union remote action legality

## Layer E: Research runtime tests

Purpose:

- validate branch/evaluation/review/repair workflows

Lives in:

- `tests/conformance/research/`

Must cover:

- stage execution mapping
- evaluation packet requirements
- review gate before promotion
- debate without direct promotion
- result-to-claim repair routing
- repair vs pivot lifecycle legality
- wake escalation and supervisor-lease linkage
- digest promotion/rejection traceability back to `ProjectOpsTick`

## 3. Fixture Delivery Phases

## Phase 0: Kernel floor

Deliver first:

- bundle/event/session schemas
- `config_precedence_model_override`
- `startup_profile_doctor_priority`
- `startup_profile_conflict_hint`
- `startup_launch_trace_complete`
- `startup_json_exits_before_repl`
- `projects_current_resolution_trace`
- `resume_latest_project_scoped`
- `resume_after_interrupt`
- `doctor_provider_matrix`
- `smoke_mutation_free`

This is the earliest point at which the product becomes credibly testable.

## Phase 1: Operator-grade CLI

Deliver:

- all config/provider/session/slash/permission fixtures from `17`
- `session_terminal_reconciliation_under_transport_loss`
- `session_creation_identity_complete`
- `session_event_provenance_and_scope_binding`
- `session_operator_logs_seq_continuity`
- `session_search_index_non_authoritative`
- `prompt_turn_trace_complete`
- `compact_derived_view_refresh`
- `setup_status_payload_complete`
- `setup_install_routes_platform_surface`
- `feature_not_graduated_typed`
- `degraded_plugin_followup_typed`
- `skills_registry_typed`
- `plugin_hook_registry_typed`
- `mcp_registry_typed`
- `projects_prune_dry_run_typed`
- `permissions_pending_history_typed`
- `conformance_result_typed`
- base command JSON-envelope tests
- envelope-and-payload paired validation for both `CommandSuccess.data` and
  `CommandFailure.data`
- degraded-state explainability fixtures
- typed non-zero JSON fixtures for ambiguity, blocked doctor, and unresolved
  project scope
- deterministic mock-provider parity scenarios for provider/auth/tool/session
  flows

Release gate:

- no shipped base command without at least one golden fixture
- no session orchestration surface graduates while duplicate terminal-state
  reconciliation, typed uncertainty, and provenance/scope labeling remain
  unverified

## Phase 2: Remote plane

Deliver:

- all remote fixtures from `22`
- `remote_pair_status_notify_takeover_typed`
- `remote_attach_inspect_vs_execute`
- `remote_handoff_inspect_vs_execute`
- `remote_action_union_payload_legality`
- `remote_local_keypress_reclaim`
- `remote_notify_payload_and_receipts`
- `remote_cached_read_model_non_authoritative`
- remote mock host harness
- replay/epoch/revocation tests
- projection-regeneration and typed-rejection tests for stale remote state

Release gate:

- no remote command graduates without harness coverage
- no remote projection-derived surface graduates unless mismatch regeneration is
  fixture-backed
- no remote action family graduates while still accepting generic payload
  objects outside the tagged-union contract

## Phase 3: Multi-agent and memory

Deliver:

- agent stop/traces fixtures
- branch stale-base and promote-gate fixtures
- review fresh-thread and retry-linkage fixtures
- memory explain/invalidation fixtures
- `memory_promotion_state_legality`
- `memory_invalidation_after_rollback`
- `projectops_digest_promotion_traceable`
- `projectops_digest_rejected_typed`
- `memory_status_typed`
- `artifacts_list_inspect_typed`
- `repo_cleanup_plan_apply_typed`
- `wake_event_escalation_typed`
- projection-backed workbench fixtures for memory/branch/review summaries

Release gate:

- no memory auto-inject surface graduates without promotion/invalidation
  legality coverage
- no ProjectOps digest promotion graduates without tick-to-digest traceability
  coverage
- no supervisor/wake surface graduates without stale-lease escalation coverage

## Phase 4: Research runtime

Deliver:

- evaluation packet completeness tests
- debate trace integrity tests
- StageExecutionMap routing tests
- result-to-claim repair loop tests
- `research_stage_gate_repair_pivot_chain`
- stage-output-or-null-result enforcement
- gate-linkage replay tests across repair/pivot successors

Release gate:

- no research stage family graduates unless gate, repair, and pivot transitions
  are replay-testable from structured artifacts
- no claimed autonomous research loop may bypass `StageExecutionMap`,
  `TaskPacket`, or gate linkage validation

## 4. CI Job Layout

Recommended CI matrix:

```text
ci/
├── pr-fast
├── pr-extended
├── merge-full
└── nightly-deep
```

### `pr-fast`

Runs on every pull request.

Covers:

- formatter/lint
- schema validation
- small kernel tests
- a curated smoke subset of golden fixtures

Budget target:

- under 10 minutes

### `pr-extended`

Runs on labeled PRs or default branch merges.

Covers:

- full golden CLI fixtures
- full runtime conformance
- deterministic replay tests

Budget target:

- under 30 minutes

### `merge-full`

Runs on merge to protected branches.

Covers:

- complete golden suite
- schema migrations
- remote harness
- review/branch/research gates

Budget target:

- slower but release-representative

### `nightly-deep`

Runs nightly.

Covers:

- fuzzier recovery cases
- replay gap simulation
- larger remote reconnect matrices
- repeated compact/resume cycles
- long-horizon memory decay/regression checks
- repeated digest promote/reject cycles
- wake escalation and reclaim churn
- research-stage repair/pivot replay regressions

## 5. Determinism Rules

Fixtures must avoid hidden non-determinism.

Rules:

- freeze clocks where practical
- freeze provider/model harness responses
- do not require paid live APIs for golden fixtures
- isolate temp directories and project roots
- use recorded traces or mock hosts for remote and model paths

If a test cannot be deterministic, it belongs outside the golden suite.

Derived-state rule:

- any rebuildable index, cache, or projection used by operator surfaces must
  have at least one fixture proving that canonical project-local truth wins and
  the derived view is regenerated rather than trusted

## 6. Release Gates

The project may not claim a graduated feature unless:

- required schema tests pass
- all feature-family golden fixtures pass
- degraded-state refusal paths pass
- recovery/replay semantics pass where relevant

Graduation examples:

- `remote handoff` requires remote harness coverage
- `branches promote` requires branch + review + evaluation coverage
- `memory explain` requires explain-backing and invalidation coverage
- `ProjectOps` digest promotion requires tick lineage and rejection coverage
- supervised-run wake handling requires lease, escalation, and reclaim coverage
- research-stage repair/pivot routing requires replayable gate-linkage coverage

## 7. Failure Triage Classes

When CI fails, classify first:

- protocol drift
- operator regression
- remote harness regression
- fixture expectation bug
- migration compatibility failure

This prevents teams from masking protocol problems as "just a test issue."

## 8. Ownership

Recommended ownership:

- protocol/schema failures -> protocol owner
- CLI/operator failures -> operator owner
- remote harness failures -> remote owner
- research/runtime failures -> research runtime owner
- release gate and flaky classification -> verification owner

## 9. First Build Checklist

Before writing advanced features, ensure CI already has:

- schema validation runner
- golden fixture runner
- event assertion helper
- JSON envelope matcher
- temp workspace/project bootstrap helper
- deterministic remote mock-host harness
- event-stream reconciliation helper for contradictory terminal-state traces
- checkpoint replay helper for advanced family reducers

Those tools should be built once and reused everywhere.

## 10. Outcome

If followed, this rollout plan turns the architecture into a continuously
provable product rather than a large unverified codebase.
