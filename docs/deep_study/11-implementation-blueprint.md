# Implementation Blueprint for the Native Project-Memory Multi-Agent Code Agent CLI

This document turns the revised architecture into a concrete implementation
blueprint.

It is still pre-code, but it is now specific enough that an engineer can start
building modules in order instead of guessing the system shape.

Language note:

- the active kernel implementation language is Rust
- the remote/mobile/web control plane remains a `Happy`/`Happier`-derived
  TypeScript/Node adaptation
- research and experiment skill runners remain Python sidecars
- canonical Rust path mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`

## 1. Build Target

The first shippable target is:

- a terminal-native code agent CLI
- with resumable project sessions
- explicit permission and workspace policy
- structured project-local persistence under `.pmcli/`
- bounded working memory and durable project memory
- controlled multi-agent execution with reviewer blinding
- artifact-family governance to prevent repo chaos

Research workflow automation should be built on top of that kernel, not mixed
into the kernel.

## 2. Proposed Repository Layout

Recommended top-level layout:

```text
research_cli/
├── Cargo.toml
├── src/
│   ├── bin/
│   │   └── research-cli.rs
│   ├── app/
│   ├── projects/
│   ├── runtime/
│   ├── session/
│   ├── workspace/
│   ├── permissions/
│   ├── tools/
│   ├── agents/
│   ├── memory/
│   ├── artifacts/
│   ├── branches/
│   ├── reviews/
│   ├── projectops/
│   ├── remotehost/
│   ├── research/
│   ├── telemetry/
│   ├── events/
│   ├── plugins/
│   ├── mcp/
│   ├── tui/
│   └── doctor/
├── schemas/
├── docs/
└── tests/
```

If the project is later split into multiple crates/packages, the ownership
boundaries should still follow this module map.

## 3. Module Responsibilities

### `src/app`

Owns service wiring only.

Should not contain business rules.

Responsibilities:

- initialize config
- initialize workspace binding
- initialize project-registry binding
- construct runtime services
- start TUI or CLI mode

### `src/projects`

Owns the cross-project registry and current-project pointer.

Responsibilities:

- register/list/prune project entries
- persist current-project pointer and recency
- resolve `data_dir` for a registered workspace
- expose project-scoped status independent of session internals

Primary types:

- `ProjectRegistryEntry`
- `CurrentProjectPointer`
- `ProjectRegistry`
- `ProjectLocator`

### `src/runtime`

Owns the top-level run loop.

Responsibilities:

- turn execution
- model call orchestration
- tool dispatch
- compaction trigger checks
- event emission
- checkpoint writes

Primary interfaces:

- `RunTurn(input TurnInput) -> TurnResult`
- `ResumeSession(sessionID string) -> ResumeResult`
- `CompactSession(sessionID string) -> CompactResult`
- `Shutdown()`

### `src/session`

Owns the session store, compaction metadata, and resumable search/index
services.

Responsibilities:

- create/list/load/delete session
- maintain workspace-scoped session namespace
- store session transcript JSONL
- store compacted summaries
- maintain session title/lineage metadata
- maintain a rebuildable session search index for browse/search/recap
- expose recap generation inputs without making the index a second authority

Primary types:

- `Session`
- `SessionStore`
- `SessionSummary`
- `CompactionRecord`
- `SessionTitleRecord`
- `SessionSearchIndex`

### `src/workspace`

Owns project-root identity and workspace binding.

Responsibilities:

- resolve workspace root
- compute workspace hash
- manage worktree or sandbox registration
- expose current project state path

Primary types:

- `Workspace`
- `WorkspaceBinding`
- `WorkspaceFingerprint`

### `internal/permissions`

Owns policy evaluation and promptable approval.

Responsibilities:

- tool permission policy
- path boundary checks
- session-level permission grants
- branch workspace policy narrowing

Primary types:

- `PermissionMode`
- `PermissionRule`
- `PermissionRequest`
- `PermissionDecision`
- `PermissionPolicy`

### `internal/tools`

Owns built-in tool registry and execution bridges.

Responsibilities:

- register tool specs
- classify read-only vs write vs destructive tools
- call shell, file, web, MCP, and repo tools
- emit normalized tool results

Primary interfaces:

- `Tool`
- `ToolRegistry`
- `ToolExecutor`
- `ToolResult`

### `internal/agents`

Owns multi-agent orchestration and per-agent storage.

Responsibilities:

- create and manage agent roles
- bind each agent to workspace or worktree
- persist private memory and trace
- enforce reviewer independence

Primary types:

- `AgentSpec`
- `TaskPacket`
- `AgentRuntime`
- `AgentRole`
- `AgentWorkspace`
- `AgentOutputManifest`

### `internal/memory`

Owns working memory, durable memory, invalidation, and retrieval.

Responsibilities:

- working memory push/read/flush
- durable memory promotion/demotion
- retrieval routing and budgets
- invalidation and forgetting
- silent consolidation

Primary types:

- `MemoryRecord`
- `WorkingMemoryRecord`
- `RecallRoute`
- `RecallBudget`
- `InvalidationEvent`

### `internal/artifacts`

Owns artifact families and canonical pointers.

Responsibilities:

- register artifact families
- decide append/fork/supersede/reject
- manage latest and canonical pointers
- archive superseded outputs

Primary types:

- `Artifact`
- `ArtifactFamily`
- `ArtifactPromotion`
- `WriteDecision`

### `internal/branches`

Owns branch search and grid-style exploration.

Responsibilities:

- create branch batches
- allocate branch workspaces
- enforce concurrency and budget
- collect scores and archive losers

Primary types:

- `SearchBatch`
- `BranchRun`
- `EvaluationContract`

### `internal/reviews`

Owns independent review and adjudication.

Responsibilities:

- open review job
- blind reviewer inputs
- store review trace
- resolve approve/reject/revise outcomes

Primary types:

- `ReviewJob`
- `ReviewPacket`
- `ReviewOutcome`
- `ReviewTrace`

### `internal/projectops`

Owns proactive project-level maintenance and explainability.

Responsibilities:

- decide when project-ops ticks run
- write progress digest candidates
- generate cleanup proposals
- supervise queued research runs
- assemble project-level explain views

Primary types:

- `ProjectOpsTick`
- `ProgressDigestCandidate`
- `RepoCleanupProposal`
- `ExperimentSupervisorLease`
- `WakeEvent`

### `internal/remotehost`

Owns the Happy-derived remote host integration.

Responsibilities:

- machine pairing and remote session binding
- session follow/takeover/reattach control
- remote notification and approval routing
- host-side protocol adaptation for mobile/web/desktop clients
- research workbench data projection for remote surfaces
- runtime-event to session-envelope projection
- attach/handoff/terminal capability evaluation
- bounded remote terminal service and control-owner tracking

Primary types:

- `RemoteMachine`
- `RemoteSessionBinding`
- `RemoteTakeoverEvent`
- `RemoteCapabilityMatrix`
- `AttachEligibility`
- `HandoffEligibility`
- `RemoteTerminalLease`
- `RemoteHostAdapter`
- `RemoteWorkbenchProjection`

This module should be further split into:

- `transport`
- `projection`
- `actions`
- `capabilities`
- `terminal`
- `workbench`

See `docs/deep_study/20-happy-happier-integration-blueprint.md`.

### `internal/research`

Owns research-stage orchestration only after the kernel is stable.

Responsibilities:

- map skills to stage graph
- declare input/output contracts
- coordinate doc/code/experiment repair loops
- connect results to memory and artifacts

Primary types:

- `ResearchStage`
- `ResearchSkillContract`
- `ResearchRun`

### `internal/events`

Owns structured event emission and subscription.

Responsibilities:

- append JSONL events
- broadcast runtime notifications
- feed TUI and background summarizers

Primary types:

- `Event`
- `EventType`
- `EventSink`
- `EventSubscriber`

### `internal/plugins`

Owns plugin lifecycle and degraded-mode behavior.

Responsibilities:

- validate config
- manage lifecycle
- expose health and discovery
- support degraded partial availability
- expose plugin registry and manifest provenance
- own hook registration and hook health surfaces

This module should sit on the `HostAdapterAPI` side, not absorb runtime state
ownership.

### `internal/mcp`

Owns MCP server registration and resource/tool access.

Responsibilities:

- MCP client lifecycle
- resource listing and reads
- tool discovery refresh
- instructions injection

### `internal/setup`

Owns install, repair, schema validation, and conformance checks.

Responsibilities:

- setup flow
- migration checks
- schema validation
- repair suggestions
- conformance runner

### `internal/tui`

Owns the terminal UI only.

Responsibilities:

- render sessions, tools, agents, branches, and reviews
- subscribe to event stream
- prompt for permission and control actions

It must not own runtime logic.

### `internal/doctor`

Owns diagnostics.

Responsibilities:

- environment checks
- provider checks
- MCP checks
- permissions and workspace checks
- data store health checks

### `internal/telemetry`

Owns operator-visible usage, cost, and stats accounting.

Responsibilities:

- session usage accumulation
- project usage rollups
- estimated and exact cost reporting
- accounting quality labeling
- operator stats surfaces

## 4. Core Data Types

## `ProjectState`

`ProjectState` is the project-scoped payload nested inside the persisted
`KernelStateBundle` checkpoint. It is not a competing top-level runtime truth.

```text
ProjectState {
  project_id
  workspace_root
  workspace_hash
  current_stage
  stage_owner
  stage_mode
  active_session_id
  last_checkpoint_at
  transition_log[]
}
```

Per-family active IDs, counts, and summaries live in the corresponding family
aggregate states inside `KernelStateBundle`, not as duplicate top-level fields
inside `ProjectState`.

The canonical family aggregate set reserved in `KernelStateBundle` must include
all operator-visible runtime families, even when some remain skeletal in early
milestones:

- session
- turn
- agent
- review
- branch
- memory
- repo
- artifacts
- projectops
- permission
- feature
- rollback
- remote

## `MemoryRecord`

```text
MemoryRecord {
  id
  namespace
  type
  surface
  body
  provenance
  source_session
  source_artifacts[]
  confidence
  status
  valid_from?
  valid_to?
  supersedes[]
  superseded_by?
  usage_count
  last_used_at?
}
```

## `ArtifactFamily`

```text
ArtifactFamily {
  family_id
  kind
  canonical_id?
  latest_id?
  archive_ids[]
  supersession_chain[]
  merge_policy
  write_mode
  promotion_history[]
  source_branch_ids[]
}
```

## `SearchBatch`

```text
SearchBatch {
  batch_id
  objective
  wave
  max_branches
  max_parallel
  budget_tokens
  budget_tool_calls
  evaluation_contract
  stop_rule
  status
}
```

## `ReviewJob`

```text
ReviewJob {
  review_id
  target_kind
  target_paths[]
  reviewer_role
  blinded
  compare_against?
  status
  outcome?
}
```

## `ReviewPacket`

```text
ReviewPacket {
  review_id
  target_paths[]
  objective
  reviewer_role
  review_model
  fresh_thread
  blinded
  blinding_policy
  blind_context[]
  banned_context[]
  redaction {
    schema_version
    policy
    allowed_count
    redacted_count
    redacted_sources[]
  }
  compare_against?
  evidence_required[]
  output_schema
  trace_id
}
```

## `TaskPacket`

```text
TaskPacket {
  runtime_version
  migration_version
  task_packet_id
  schema_version
  conformance_line
  canonical_path
  retention_policy
  atomic_write_policy
  owner_session_id
  owner_scope_kind
  agent_role
  objective
  scope_summary
  allowed_paths[]
  forbidden_paths[]
  input_artifact_refs[]
  expected_outputs[]
  permission_mode
  tool_budget
  token_budget
  change_envelope_ref?
  rollback_snapshot_required
  reviewer_blinding_required
  success_criteria[]
  failure_contract[]
}
```

Canonical schema owner:

- `schemas/task_packet.schema.json`

This section is an informative blueprint rendering of the canonical schema.

Rules:

- every launched agent run binds exactly one `TaskPacket`
- `TaskPacket` is the persistence truth for scope, budget, and write authority
- reviewer packets may set empty allowed paths when the review is read-only
- a task packet must validate before an agent may enter `running`
- breaking changes require a new schema line rather than in-place mutation

## 5. `.pmcli/` File Formats

All canonical object/snapshot `.pmcli` files must contain `schema_version` and
validate against repository-published schemas.

Append-only line-record logs remain canonical too, but they validate per line
kind against their published schemas rather than through a
`CanonicalObjectEnvelope`.

The exact transition, migration, atomic-write, and review/research contracts
are specified in:

- `docs/deep_study/14-executable-runtime-protocol.md`
- `docs/deep_study/15-research-review-and-contracts.md`

### `project_state.json`

Canonical serialized `KernelStateBundle` checkpoint.

Encoding:

- `CanonicalObjectEnvelope`
- payload: `KernelStateBundle`

### `project_meta.json`

Project-local metadata snapshot used to align the project-local protocol root
with the global operator registry.

Required contents:

- `project_id`
- `workspace_root`
- `workspace_hash`
- `data_dir`
- `protocol_version`

Encoding:

- `CanonicalObjectEnvelope`
- payload: `ProjectMeta`

### `sessions/<session_id>/transcript.jsonl`

Append-only transcript and tool trace.

Line types:

- `control`
- `message`
- `tool_call`
- `tool_result`
- `summary_reference`

### `sessions/<session_id>/summary.md`

Human-readable compaction view.

Derived from canonical event/session state.

### `sessions/<session_id>/meta.json`

Session-local metadata:

- session identity
- title and lineage summary
- status
- created/updated timestamps

### `indexes/session_search.sqlite`

Rebuildable session search and recap index for browse/search surfaces.

Rules:

- derived from canonical session metadata and transcript artifacts
- may be deleted and rebuilt without data loss
- must never outrank transcript or session metadata truth
- may use SQLite/FTS or an equivalent indexed store as long as rebuild
  semantics stay deterministic

### `events/events.jsonl`

Append-only structured event log for the whole project.

### `agents/<agent_id>/TRACE.jsonl`

Append-only agent-local event stream.

### `agents/<agent_id>/TASK_PACKET.json`

Canonical persisted task binding for that agent.

Required properties:

- schema-valid `TaskPacket`
- immutable once execution starts except additive metadata fields
- written before the agent lifecycle may transition from `ready` to `running`

### `agents/<agent_id>/OUTPUT_MANIFEST.json`

Final machine-readable output index for that agent:

- artifacts created
- workspace binding
- review visibility class
- promoted outputs

### `remote/control_owner.json`

Project-local projected control-owner state for the active session bindings.

Required contents:

- current `binding_id`
- current control owner projection
- `ownership_epoch`
- projection freshness metadata

### `remote/BINDINGS.json`

Current remote session bindings and control-lease projections.

### `remote/cursors/<session_id>.json`

Per-session remote replay cursor.

### `remote/projections/<session_id>.json`

Latest `SessionEnvelopeProjectionV1` exported to remote clients.

### Machine-global remote authority

Pairing, machine identity, remote lease, and capability authority are not
stored inside project-local `.pmcli/remote/`.

Canonical location:

- `$XDG_STATE_HOME/research-cli/remote/`
- fallback: `~/.local/state/research-cli/remote/`

Canonical files:

- `machine_identity.json`
- `lease.json`
- `capabilities.json`

Rule:

- project-local remote files may project or bind machine-global remote
  authority, but may never replace it

### `memory/working.jsonl`

Fast append log for working memory items before consolidation.

### `artifacts/families.json`

Registry of all artifact families and latest/canonical pointers.

### Global operator registry

Cross-project operator state is not stored inside the project-local `.pmcli/`
tree.

Canonical location:

- `$XDG_STATE_HOME/research-cli/registry/`
- fallback: `~/.local/state/research-cli/registry/`

Canonical files:

```text
registry/
├── projects.json
├── current_project.json
└── locks/
```

Rules:

- `projects.json` stores `ProjectRegistryEntry` records only
- `current_project.json` stores the current-project pointer only
- registry state may point at a project's `.pmcli/`, but it may not replace or
  shadow project-local runtime truth
- both registry files are `CanonicalObjectEnvelope` registry objects

### Schema and conformance directory

The repository should publish:

```text
schemas/
├── project_state.schema.json
├── event.schema.json
├── session.schema.json
├── task_packet.schema.json
├── review.schema.json
├── memory.schema.json
├── artifact.schema.json
├── branch.schema.json
├── remote_lease.schema.json
├── remote_binding.schema.json
└── control_lease.schema.json
```

and

```text
tests/conformance/
├── fixtures/
└── run_conformance.sh
```

## 6. Runtime Flow

### Turn flow

1. load `KernelStateBundle` checkpoint from `project_state.json`
2. load active session state
3. resolve working memory budget
4. resolve durable recall budget
5. inject scoped context
6. run model turn
7. gate tool calls through permission policy
8. emit events
9. update working memory
10. schedule silent consolidation
11. checkpoint project state

Checkpoint publication rule:

- mutating subsystems append canonical events first
- bundle publication is performed by a checkpoint reducer, not by arbitrary
  last-writer-wins replacement
- reducer input is the previous committed `seq_cursor` plus unapplied event
  range from `events/events.jsonl`
- reducer publish must fail closed when its base cursor is stale, then reload
  and replay

### Agent flow

1. director creates `AgentSpec` and `TaskPacket`
2. runtime allocates agent workspace binding
3. create `.pmcli/agents/<agent_id>/`
4. persist `TASK_PACKET.json`, directive, and private memory seed
5. run agent
6. collect output manifest
7. if reviewer: keep blinded packet
8. if executor: propose artifacts, but do not promote yet

### Branch-search flow

1. planner opens `SearchBatch`
2. allocate N branch workspaces
3. spawn branch agents
4. gather artifacts and scores
5. run independent review through a `ReviewPacket`
6. promote winner or open another wave
7. archive losing branches

## 7. CLI Surface

This section is an implementation-oriented preview only.

The authoritative frozen first-release command registry lives in:

- `docs/deep_study/16-kernel-parity-contract.md`
- `docs/deep_study/23-cli-operator-contract.md`

The first stable command set should therefore stay aligned with those docs:

- `research-cli`
- `research-cli prompt`
- `research-cli chat`
- `research-cli resume`
- `research-cli continue`
- `research-cli sessions`
- `research-cli projects`
- `research-cli model`
- `research-cli providers`
- `research-cli config`
- `research-cli permissions`
- `research-cli compact`
- `research-cli doctor`
- `research-cli smoke`
- `research-cli inspect`
- `research-cli usage`
- `research-cli cost`
- `research-cli stats`
- `research-cli agents`
- `research-cli branches`
- `research-cli reviews`
- `research-cli memory`
- `research-cli artifacts`
- `research-cli repo`
- `research-cli skills`
- `research-cli mcp`
- `research-cli setup`
- `research-cli remote`
- `research-cli conformance`

The frozen first-release operator contract is specified in:

- `docs/deep_study/16-kernel-parity-contract.md`

## 8. TUI Surface

The TUI should have first-class panes for:

- conversation
- tool activity
- agent list and status
- branch/search waves
- review queue
- memory summary
- memory explain / recall provenance
- artifact families
- project registry / project status
- pending permissions
- usage / cost / stats
- command palette or slash-command discoverability

## 9. Verification Strategy

Kernel verification should follow the Claw-style discipline:

- unit tests for policies and schemas
- deterministic mock model harness
- replay tests for session resume
- permission regression tests
- event-sequence contract tests
- workspace boundary tests
- conformance tests for `.pmcli` schema lines

Memory verification should include:

- bounded working-memory tests
- invalidation tests
- retrieval budget tests
- consolidation idempotence tests

Repo-governance verification should include:

- append/fork/supersede decision tests
- canonical pointer tests
- archive chain tests
- duplicate/orphan cleanup plan tests

Kernel parity verification should also include:

- project-registry tests
- config precedence and effective-view tests
- provider/auth resolution tests
- alias-resolution and proxy-routing tests
- REPL/slash-command semantics tests
- interrupt/retry/resume semantics tests
- JSON output contract tests
- memory query/explain contract tests
- usage/cost accounting tests
- debate trace and disagreement-resolution tests

## 10. What Is Ready To Implement First

The first implementable slice is:

- workspace identity
- session store
- event log
- permission policy
- tool registry
- minimal TUI/CLI shell

The second slice is:

- agent runtime
- reviewer blinding
- working memory
- artifact family registry
- review packet and trace protocol

The third slice is:

- durable memory
- branch search
- research-stage orchestration
- conformance and migration hardening

That is the cleanest path to avoid building the advanced layers on top of an
unstable shell.
