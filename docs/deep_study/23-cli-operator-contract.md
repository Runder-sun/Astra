# CLI Operator Contract

This document freezes the exact first-release operator contract for
`research-cli`.

`16-kernel-parity-contract.md` defines the stable surface.

This document goes one level deeper and removes the remaining ambiguity that an
external reviewer called out:

- every shipped command family needs exact behavior
- machine consumers need one JSON/output contract
- operators need stable exit codes and side-effect expectations
- conformance needs fixture obligations, not only command names

The goal is to make the base CLI operator surface visibly stronger and more
executable than the current reference set.

## 1. Universal Operator Rules

All first-release commands must obey the same baseline rules.

### 1.1 Output modes

Every command supports:

- human-readable text output by default
- `--json` for machine-readable output
- `--quiet` for suppressing non-essential human chatter

If `--json` is requested, stdout must contain valid JSON only.

Diagnostics that do not belong in the JSON payload must go to stderr.

Interactive JSON rule:

- interactive entrypoints may not mix REPL text with JSON on stdout
- `research-cli --json` and `research-cli chat --json` must perform
  launch/preflight resolution, emit one `InteractiveLaunchResult`, and exit
- opening a long-lived framed interactive JSON protocol is reserved for a later
  graduated transport and is not part of the M0-M2 stdout contract

Machine-readable discoverability rule:

- help, slash help, and command-palette exports must resolve to one canonical
  help/palette surface rather than separate ad hoc JSON shapes
- resume ambiguity and browse/search discovery must return typed candidates or a
  typed ambiguity result instead of free-form lists

### 1.2 Standard result envelopes

All command JSON must use one of these envelopes.

## `CommandSuccess`

```json
{
  "ok": true,
  "command": "sessions list",
  "project_id": "proj_demo",
  "session_id": null,
  "data": {}
}
```

## `CommandFailure`

```json
{
  "ok": false,
  "command": "providers test",
  "project_id": "proj_demo",
  "session_id": null,
  "error": {
    "code": "auth_invalid_shape",
    "message": "ANTHROPIC_AUTH_TOKEN contains an API key shape",
    "hint": "Move the key to ANTHROPIC_API_KEY",
    "retryable": false
  },
  "data": {
    "overall_status": "blocked"
  }
}
```

Failure-context rule:

- non-zero exits may still carry schema-owned machine context in `data`
- blocked, ambiguous, or refused commands may not invent a second ad hoc JSON
  failure shape
- typical failure payloads include `ResumeAmbiguityResult`,
  `ProjectResolutionTrace`, `DoctorReport`, and `SmokeResult`

Canonical advanced failure payloads:

- `FeatureGateResult` for `not_graduated`, disabled, or unsupported command
  lanes
- `PolicyRefusalResult` for permission, ownership, feature-health, or safety
  refusals
- `RemoteActionRejection` for remote attach/handoff/takeover/notify refusals
- `FollowupRequiredResult` for exit `11` lanes where work is partially valid but
  operator follow-up is required

### 1.3 Frozen exit codes

The entire base CLI shares one exit-code table.

| Exit code | Meaning |
|---|---|
| `0` | success |
| `2` | usage or argument parse error |
| `3` | config or provider resolution failure |
| `4` | auth failure or preflight-blocked runtime |
| `5` | permission denied or approval unresolved |
| `6` | target not found or ambiguous target |
| `7` | workspace, project, or branch policy violation |
| `8` | feature unavailable, degraded beyond policy, or unsupported command lane |
| `9` | stale lease, ownership conflict, or concurrent mutation conflict |
| `10` | invariant, schema, or conformance failure |
| `11` | partial success with required manual follow-up |
| `12` | typed remote action rejection |

No command may invent a private exit code.

### 1.4 Event emission rule

Every command must emit canonical event envelopes, not ad hoc per-command
shapes.

Minimum requirements:

- every command emits a start-phase command event
- every terminal command event carries terminal outcome in the shared event
  envelope
- symbolic labels in this document are shorthand `event_name` values only; the
  wire format is frozen in `24-kernel-state-machine-contract.md`

Commands that mutate state must also emit family-specific events.

### 1.5 Text-mode rule

Text mode must always answer three operator questions without requiring logs:

- what happened
- what object it happened to
- what the user should do next if it failed

### 1.6 Scope rule

Unless a command explicitly supports cross-project queries, all commands are
scoped to the resolved current project/workspace.

Cross-project surfaces must name project scope in both text and JSON output.

## 2. Global Option Grammar

The stable top-level grammar is:

```text
research-cli [global-options] <command> [subcommand] [arguments]
```

Stable global options:

- `--project <project-id>`
- `--cwd <path>`
- `--json`
- `--quiet`
- `--no-color`
- `--trace`
- `--profile <profile-id>`

Profile rule:

- profile resolution must be machine-explainable, not only an implicit config
  side effect
- commands that consume a resolved profile must expose
  `ProfileResolutionTrace` in launch, inspect, or trace-friendly output lanes

Project resolution rule:

- current-project resolution must also be machine-explainable rather than a
  hidden cwd/current-pointer side effect
- `projects current --json` must return `ProjectResolutionTrace`
- startup, inspect, and other trace-friendly lanes may expose
  `ProjectResolutionTrace` when project scope resolution is central to the
  result

Resolution order:

1. explicit `--project`
2. explicit `--cwd`
3. workspace detection from current directory
4. stored current-project pointer

Ambiguous scope must fail with exit code `6`.

## 3. Core Runtime Commands

## `research-cli`

Purpose:

- open a new interactive session by default
- or continue only when explicit continue semantics are requested

Stable behavior:

- no positional prompt means interactive TUI/REPL session
- `--continue` resumes the latest session in current project scope
- blocked preflight returns exit `4`
- `--json` switches to launch-inspect-and-exit mode and must not open the REPL

Success payload:

- `InteractiveLaunchResult`
- JSON launch result must expose preflight plus resolved project/profile traces
  in one round-trip

Required events:

- `runtime_preflight`
- `session_open` or `session_resume`
- `repl` only when the command actually enters interactive text mode

## `research-cli prompt <prompt>`

Purpose:

- execute one bounded turn in one-shot mode

Stable flags:

- `--session <session-id>`
- `--new-session`
- `--model <model>`
- `--provider <provider>`
- `--permission-mode <mode>`
- `--output text|json`

Rules:

- exactly one prompt payload is required
- `--session` appends to an existing session
- `--new-session` forces a fresh session
- omitting both creates a fresh one-shot session

Success JSON:

- `TurnResult`

Trace rule:

- `TurnResult` must expose project, profile, and provider resolution in one
  typed payload rather than requiring separate follow-up commands

Failure lanes:

- bad arguments -> `2`
- provider/config resolution -> `3`
- auth/preflight block -> `4`
- permission unresolved -> `5`

Events:

- `turn`
- `provider_resolution`
- `permission` if needed

Recovery payloads used by foreground control surfaces:

- `InterruptResult`
- `RetryResult`
- `UndoRefusal`

## `research-cli chat`

Purpose:

- explicit synonym for interactive session mode

Contract:

- behavior matches bare `research-cli`
- must not drift into a separate runtime lane
- `chat --json` follows the same launch-inspect-and-exit rule as bare
  `research-cli --json`

## `research-cli resume <target>`

Purpose:

- resume an existing resumable session

Accepted targets:

- exact `session_id`
- stable alias
- `latest`

Rules:

- ambiguous alias fails with `6`
- child/internal/tool sessions fail with `7`
- workspace mismatch fails with `7`

Success JSON:

- `ResumeResult`

Events:

- `session_resume`

## `research-cli continue`

Purpose:

- resume the latest resumable session in the current project scope

Rules:

- no cross-project fallback
- if no resumable session exists, fail with `6`

## `research-cli inspect [session-id]`

Purpose:

- inspect current or specified session, recent turn state, provider, tools,
  permissions, feature degradation, lineage summary, runtime/source affinity,
  and durable operator log health

Stable flags:

- `--project`

Rules:

- `inspect --project` returns project-scoped operational state instead of one
  session inspection
- project inspection always returns a stable core payload
- advanced sections may appear only when their backing subsystem has graduated
- omitted advanced sections must be represented in `section_status` with
  `not_graduated`, `disabled`, or `degraded` instead of invented placeholder
  objects
- mixing a session target with `--project` fails with `2`
- session inspection should expose canonical runtime/source affinity and
  transcript-source information when available
- session inspection may expose durable operator log summary, but log artifacts
  remain non-authoritative debugging evidence

Success JSON:

- `SessionInspection`
- `ProjectInspection` when `--project` is selected

### `ProjectInspection`

```text
ProjectInspection {
  project_id
  registry_entry
  init_state
  active_session_id?
  degraded_features[]
  section_status
  sections?
}
```

Stable M0-M2 core:

```text
ProjectInspectionCore {
  project_id
  registry_entry
  init_state
  active_session_id?
  degraded_features[]
}
```

Optional graduated sections:

```text
ProjectInspectionSections {
  projectops?
  memory?
  branches?
  reviews?
  active_runs?
  cleanup?
}
```

### `section_status`

```text
ProjectInspectionSectionStatus {
  projectops
  memory
  branches
  reviews
  active_runs
  cleanup
}
```

Required rules:

- M0-M2 guarantees only `ProjectInspectionCore`
- advanced sections must not appear before their milestone-backed runtime exists
- a not-yet-built section reports `not_graduated`
- a disabled configured section reports `disabled`
- a temporarily unhealthy graduated section reports `degraded`

Events:

- `inspection`

## `research-cli compact [session-id]`

Purpose:

- deterministically compact a session transcript into recap state without
  silently archiving or deleting raw history

Rules:

- defaults to the current or latest resumable session in scope
- writes compaction lineage and summary refs before publishing the compacted
  view
- success must report which derived views were refreshed for resume, browse, or
  search continuity
- blocked lineage or persistence failure returns `10`
- partial derived-view refresh returns `11`

Success JSON:

- `CompactResult`

### `CompactResult`

```text
CompactResult {
  session_id
  compaction_record_id
  summary_ref
  resume_recap_ref?
  compacted_turn_count
  raw_log_retained
  lineage_ok
  derived_views_updated[]
  deferred_repairs[]
}
```

Events:

- `session_compaction`

## `research-cli doctor`

Purpose:

- run environment, config, provider, workspace, plugin, and MCP preflight

Rules:

- must be safe in read-only mode
- must distinguish `ready`, `degraded`, and `blocked`

Success JSON:

- `DoctorReport`

Failure:

- true blocked state still returns exit `4`, but JSON must be emitted when
  `--json` is present
- blocked or refused diagnostic lanes must return
  `CommandFailure.data = DoctorReport`

### `DoctorReport`

```text
DoctorReport {
  overall_status
  workspace
  project
  providers[]
  config
  binaries[]
  plugins[]
  hooks[]
  mcp[]
  remote
  repair_hints[]
}
```

## `research-cli smoke`

Purpose:

- execute a minimal end-to-end smoke for model resolution, tool registry,
  session persistence, and event log emission

Rules:

- must not mutate user source files
- may write only under `.pmcli/`

Success JSON:

- `SmokeResult`

Failure:

- blocked preflight returns `4`
- mutation-policy or persistence invariants return `10`
- when `--json` is present, `CommandFailure.data` must carry `SmokeResult`

### `SmokeResult`

```text
SmokeResult {
  overall_status
  preflight
  checks[]
  provider_trace?
  session_id?
  turn_id?
  event_log_path?
  transcript_path?
  source_mutation_free
  mutated_paths[]
}
```

## `research-cli conformance`

Purpose:

- run schema and fixture conformance

Rules:

- can target one fixture family or all families
- `--json` returns `ConformanceResult`
- any failed fixture returns exit `10`

## Setup command family contract

The setup family exists to absorb onboarding, migration, and repair logic
explicitly instead of burying it in ad hoc startup failures.

## `setup status`

- returns install routes, schema versions, data directories, and last migration
  status
- `--json` returns `SetupStatusReport`
- emits `event_name=setup`

## `setup migrate-check`

- reports pending schema or layout migrations without mutating anything
- `--json` returns `MigrateCheckReport`
- incompatible migration state returns `10`
- emits `event_name=setup`

## `setup repair-hints`

- returns deterministic remediation steps derived from doctor/preflight state
- `--json` returns `RepairHintsReport`
- must be machine-readable and grouped by component
- emits `event_name=setup`

## `setup install-routes`

- returns canonical install/config/data locations for providers, skills, MCP,
  plugins, hooks, remote host assets, and `.pmcli/`
- `--json` returns `InstallRoutesReport`
- supports platform-specific path variants
- emits `event_name=setup`

## 4. Project Commands

Project command JSON uses `ProjectRegistryEntry`, `ProjectRegistryList`,
`ProjectStatus`, or `ProjectResolutionTrace`.

## `projects list`

- no arguments
- lists known projects in recency order
- `--json` returns `ProjectRegistryList`
- emits `event_name=project`
- not-found is never an error; empty list is valid

## `projects current`

- shows the resolved project for the current scope and why that resolution won
- `--json` returns `ProjectResolutionTrace`
- unresolved or ambiguous project returns `6`
- when `--json` is present on failure, `CommandFailure.data` must carry
  `ProjectResolutionTrace`

## `projects register <path>`

- registers a workspace root
- success returns `ProjectRegistryEntry`
- writes project registry metadata
- emits `event_name=project`
- invalid path returns `6`

## `projects init [path]`

- initializes `.pmcli/` protocol skeleton for a workspace
- success returns `ProjectStatus`
- may imply register when project is new
- emits `event_name=project`
- blocked write policy returns `5` or `7`

## `projects status`

- reports init state, active session, open branches, repo health, and degraded
  features
- `--json` returns `ProjectStatus`
- emits `event_name=project`

## `projects prune`

- prunes dead registry entries or unreachable data dirs
- default mode is dry-run
- `--json` returns `PruneResult`
- `--apply` performs mutation
- dry-run success still returns `0`
- apply conflict returns `9`
- emits `event_name=project`

## 5. Session Commands

Session command JSON uses `SessionIdentity`, `SessionSearchResult`,
`SessionExportResult`, and `SessionStats`.

## Session title, lineage, and recap rules

The first-release session UX must absorb the strongest Hermes-grade session
behaviors instead of stopping at raw IDs.

Required rules:

- sessions may have a human-readable unique title
- title uniqueness is enforced only for non-null titles
- resuming by title resolves to the latest lineage member unless ambiguity still
  remains
- resume text mode shows a compact recap panel unless minimal mode is selected
- recap must hide internal reasoning and raw tool payloads while preserving
  continuity for the next turn

### `SessionTitleRecord`

```text
SessionTitleRecord {
  session_id
  title?
  title_source
  lineage_root_id?
  lineage_index?
}
```

Allowed `title_source` values:

- `manual`
- `auto_generated`
- `lineage_derived`

### `ResumeRecap`

```text
ResumeRecap {
  session_id
  message_count
  shown_exchange_count
  truncated
  user_preview[]
  assistant_preview[]
  tool_summary[]
}
```

## `sessions list`

- supports `--limit`, `--kind`, `--project`, `--include-archived`
- returns resumable sessions first unless sorted explicitly
- text mode should prefer title + preview + relative activity time when titles
  exist
- emits `event_name=session`

## `sessions browse`

- interactive or paged browse surface for recent sessions
- `--json` returns `SessionBrowseResult`, not terminal UI markup
- ambiguous browse target does not mutate state

## `sessions search <query>`

- full-text and metadata search across project sessions
- accepts `--scope current|all`
- returns ranked hits with source tags and lineage hints
- search reads from a rebuildable derived index and must not become the source
  of session truth
- emits `event_name=session`

## `sessions export <session-id>`

- exports transcript, summaries, inspection metadata, and operator-log refs
- supports `--format jsonl|json|md`
- exported JSON must include lineage, title metadata, runtime/source affinity,
  transcript-source envelopes, and durable log manifest refs when available
- invalid session -> `6`
- write failure -> `7`
- emits `event_name=session`

## `sessions logs [session-id]`

- returns durable per-request operator log artifacts for one session
- supports `--request-seq <n>` and `--kind request|response_stream|response|tool_results`
- `--json` returns `SessionOperatorLogManifest` or a filtered subset
- missing or inconsistent log artifacts must be reported structurally instead of
  silently omitted
- these logs are debugging/export surfaces only and never change session
  authority
- invalid session -> `6`
- emits `event_name=session`

## `sessions rename <session-id> <title>`

- sets or replaces a session title
- duplicate live title returns `9`
- emits `event_name=session`

## `sessions delete <session-id>`

- deletes one ended session after confirmation
- active or resumable foreground session delete requires explicit force policy
- delete must preserve an audit tombstone in metadata
- emits `event_name=session`

## `sessions prune`

- default mode is dry-run
- may target archived, orphaned, or stale sessions only
- resumable active sessions cannot be pruned without explicit override
- emits `event_name=session`

## `sessions stats`

- reports counts, token usage, compaction counts, and age distribution
- emits `event_name=session`

## 6. Model, Provider, Auth, And Config Commands

These commands use `ModelCatalogList`, `ModelCurrentResult`,
`ProviderResolution`, `ProviderStatus`, `ProviderStatusList`,
`EffectiveConfigReport`, `ConfigSourceReport`, and `ConfigValueResult`.

## Provider catalog and installability rules

To match Crush-grade provider productization, provider status must be more than
ambient env detection.

### `ProviderCatalogState`

```text
ProviderCatalogState {
  provider_id
  catalog_source
  catalog_version
  embedded
  refreshable
  last_refreshed_at?
  model_count
}
```

## `model list`

- lists available model aliases and canonical model IDs
- degraded providers remain visible but flagged

## `model current`

- shows resolved default model for current scope
- output includes provider and config source

## `model set <model>`

- writes model config at requested scope
- requires explicit `--scope global|project|private`
- missing scope returns `2`
- emits `event_name=config`

## `providers list`

- lists configured providers, auth status, feature degradation, and catalog
  state

## `providers inspect <provider>`

- returns auth source, base URL source, supported models, catalog source, and
  degraded capabilities
- unknown provider -> `6`

## `providers auth-status`

- verifies auth material presence and shape without making a paid request
- emits `event_name=provider_auth`

## `providers test <provider?>`

- validates auth and a minimal live request path
- live transport failures return `4`
- provider mismatch or invalid model lane returns `3`
- emits `event_name=provider_test`

## `providers refresh-catalog [source]`

- refreshes provider/model catalog metadata from `embedded`, `remote`, or a
  local file source
- refresh must never silently overwrite operator-pinned provider settings
- failed refresh returns `8` when the embedded catalog remains usable, otherwise
  `10`
- emits `event_name=provider_catalog`

## `config get <key>`

- returns effective value and source
- unknown key returns `6`

## `config set <key> <value>`

- requires `--scope`
- mutates exactly one config file
- emits `event_name=config`

## `config edit`

- opens config editor or prints editable path when `EDITOR` is unavailable
- must never edit multiple scopes at once

## `config effective`

- returns merged config view
- `--json` required for automation use

## `config sources`

- returns every source file and overridden field path
- emits `event_name=config`

## 7. Permission, Usage, Cost, And Stats Commands

## `permissions mode [mode]`

- without arg, returns current mode
- with arg, mutates current-session or configured default depending on
  `--scope`
- emits `event_name=permission_mode` on mutation

## `permissions pending`

- lists unresolved permission requests with request IDs
- `--json` returns `PermissionPendingList`
- empty list is valid

## `permissions history`

- returns recent `PermissionDecisionTrace` entries
- `--json` returns `PermissionHistoryResult`
- supports `--session`, `--limit`, `--tool`

## `usage`

- supports `--scope turn|session|project`
- returns `UsageSummary`
- degraded accounting must set `accounting_quality`

## `cost`

- reports monetary estimate or provider-reported cost
- same scope rules as `usage`

## `stats`

- top-level runtime stats surface
- includes session, tool, provider, branch, and memory counters that are safe
  to expose before advanced layers are enabled

## 8. Memory, Artifact, And Repo Commands

These commands are frozen as shipped surfaces, but only become available when
their backing milestones are complete.

If invoked before feature graduation, they must fail with exit `8` and a
machine-readable `FeatureGateResult`.

## `memory query <query>`

- returns `MemoryQueryResult`
- must include route, budget used, matched records, and degraded reasons
- emits `event_name=memory_query`

## `memory explain <record-id>`

- returns `MemoryExplainRecord`
- missing record -> `6`
- superseded/contested state must be explicit

## `memory status`

- returns working-memory, durable-memory, and promotion queue health
- `--json` returns `MemoryStatusReport`

## `memory invalidate <record-id>`

- marks memory as invalidated or superseded
- requires mutation permission
- emits `event_name=memory_invalidation`

## `artifacts list`

- lists artifact families and canonical pointers
- `--json` returns `ArtifactListResult`
- supports `--family`, `--status`, `--include-archive`

## `artifacts inspect <family-or-artifact>`

- returns canonical path, lineage, promotion history, and review links
- `--json` returns `ArtifactInspectionResult`

## `repo cleanup-plan`

- dry-run only
- returns proposed cleanup actions and review gates
- `--json` returns `RepoCleanupProposal`
- emits `event_name=cleanup_plan`

## `repo cleanup-apply <plan-id>`

- requires explicit approved plan ID
- no implicit recomputation
- `--json` returns `RepoCleanupProposal`
- approval or stale plan failure -> `9`
- emits `event_name=cleanup_apply`

## `projectops status`

- returns persisted supervisor leases and wake events for the current project
- `--json` returns `ProjectOpsStatusReport`

## `projectops lease acquire`

- acquires an `ExperimentSupervisorLease` for a supervised run
- requires `--run-id` and `--owner-agent`
- supports `--stale-after-sec`
- `--json` returns `ExperimentSupervisorLease`
- emits `event_name=projectops_lease`

## `projectops wake emit`

- records a typed `WakeEvent` linked to a supervised run and optional lease
- supports `--run-id`, `--lease-id`, `--owner-agent`, `--kind`, `--urgency`,
  `--summary`, and `--requires-main-system`
- `--json` returns `WakeEvent`
- emits `event_name=projectops_wake`

## 9. Agents, Branches, Reviews, Skills, MCP, And Setup Commands

These are operator surfaces over advanced subsystems and therefore share the
same feature-graduation rule as memory/artifact commands.

## `AgentRuntimeRecord`

```text
AgentRuntimeRecord {
  agent_id
  role
  owner_session_id
  workspace_binding
  status
  budget_state
  scope
  task_packet_ref
  last_state_change_at
  latest_output_ref?
}
```

## `agents list`

- lists active and recent agents
- includes role, session, workspace binding, lease, and status

- supports `--role`, `--status`, `--session`, and `--include-completed`

## `agents inspect <agent-id>`

- returns full `AgentRuntimeRecord`, output manifest, budget usage, and last
  typed failure
- must expose task-packet ref plus effective allowed-path scope
- unknown agent -> `6`

## `agents stop <agent-id>`

- requests graceful stop first, then hard stop if policy and timeout permit
- reviewer agents with an active verdict submission window cannot be hard-killed
  silently
- emits `event_name=agent_stop`

## `agents traces <agent-id>`

- returns trace metadata and export locations, not raw multiline logs by default
- supports `--format refs|jsonl|summary`

## `BranchBatchRecord`

```text
BranchBatchRecord {
  batch_id
  objective
  owner_session_id
  status
  max_parallel
  max_total
  winner_branch_id?
  review_gate
  stale_base_detected
}
```

## `BranchRunRecord`

```text
BranchRunRecord {
  branch_id
  batch_id
  hypothesis
  workspace_binding
  status
  retry_count
  evaluation_packet_id?
  review_id?
  score?
}
```

## `branches list`

- lists active and recent branch batches and branch runs
- includes scheduler state and winner/loser status

- supports `--batch`, `--status`, `--include-archived`

## `branches inspect <batch-or-branch-id>`

- returns `BranchBatchRecord` or `BranchRunRecord`
- includes stale-base detection, evaluation packet refs, review gate, and
  promotion eligibility
- unknown target -> `6`

## `branches cancel <batch-or-branch-id>`

- default behavior is graceful cancellation for queued or running branches
- already promotable or won branches require director-scoped force policy
- emits `event_name=branch_cancel`

## `branches promote <branch-id>`

- director-only operator surface
- requires completed evaluation packet and passing review gate
- missing review gate or stale base conflict returns `9`
- emits `event_name=branch_promote`

## `ReviewRuntimeRecord`

```text
ReviewRuntimeRecord {
  review_id
  target_kind
  target_refs[]
  reviewer_role
  fresh_thread
  blinded
  status
  verdict?
  compare_against?
  trace_ref?
}
```

## `reviews list`

- lists review jobs, packets, outcomes, and unresolved gates

- supports `--status`, `--target-kind`, and `--reviewer-role`

## `reviews inspect <review-id>`

- returns `ReviewRuntimeRecord`, packet metadata, evidence requirements, and
  trace refs
- unknown review -> `6`

## `reviews open <target>`

- creates a new review job from explicit target refs and reviewer policy
- requires declared target kind and objective
- accepts `--context <text>` as blinded reviewer context
- accepts `--executor-summary <text>` only as an explicitly redacted source; it
  must not appear in the persisted review packet or prompt snapshot
- emits `event_name=review_open`

## `reviews retry <review-id>`

- allowed only when the prior review failed for transport, timeout, or explicit
  rerun policy
- compare mode must preserve `compare_against` linkage
- emits `event_name=review_retry`

## `research status`

- reports the active research thread, active stage execution, active
  deliberation span, pending operation candidates, and next recommended action
- `--json` returns `ResearchStatusReport`
- if no research thread is active, returns an empty-but-valid report rather than
  inventing a stage from ordinary session text
- emits `event_name=research_status`

## `research threads list`

- lists active, awaiting-gate, archived, and abandoned research threads in the
  current project
- `--json` returns `ResearchThreadListResult`
- supports `--status`, `--stage`, and `--include-archived`
- emits `event_name=research_thread`

## `research thread inspect <thread-id>`

- returns the thread record, active stage execution, stage graph summary,
  deliberation spans, evidence refs, DocFrame refs, and canonical artifact links
- unknown thread -> `6`
- `--json` returns `ResearchThreadInspection`
- emits `event_name=research_thread`

## `research stage inspect <execution-id>`

- returns one `ResearchStageExecution`, its operation edge, parent/root refs,
  deliberation spans, M9 batch ref if any, input/output artifacts, and gate
  status
- unknown execution -> `6`
- `--json` returns `ResearchStageInspection`
- emits `event_name=research_stage`

## `research decide`

- records an explicit human decision for a pending research operation
- stable form:
  `research decide --thread <thread-id> --operation <advance|retry|repair|pivot|fork|supersede|abandon|human_override> --decision <approve|reject|revise> [--reason <text>]`
- high-impact, public-surface-changing, costly, or authority-changing
  operations must pass through this command or an equivalent interactive gate
- `--json` returns `ResearchDecisionRecord`
- rejected or revised decisions leave the current deliberation active
- emits `event_name=research_decision`

## `research classify`

- classifies a natural-language turn against the active research runtime state
  without changing public docs, code, experiments, or irreversible DAG edges
- stable form:
  `research classify --text <turn> [--thread <thread-id>] --dry-run --json`
- `--dry-run` is required until automatic prompt/resume integration graduates
- `--json` returns `ResearchTurnClassification`
- high-impact inferred operations must set `human_gate_required=true` and remain
  pending until `research decide` or an equivalent interactive confirmation
- low-confidence classifications must include `alternative_interpretations`
  and a natural-language clarification prompt in `next_recommended_action`
- emits `event_name=research_classification`

## `research record`

- promotes agreed deliberation decisions into governed artifacts and refreshes
  DocFrames
- stable form:
  `research record --thread <thread-id> --kind <design|plan|result|paper|handoff> [--from-span <span-id>]`
- mutating record operations must obey artifact family and canonical surface
  rules; they may not expose competing public docs or code paths
- `--json` returns `ResearchRecordResult`
- emits `event_name=research_record`

## `skills list`

- lists discovered skill manifests, stage compatibility, and degraded skills
- `--json` returns `SkillListResult`
- supports `--stage`, `--include-disabled`, and `--source`

## `skills inspect <skill-id>`

- returns manifest, stage graph compatibility, install origin, disabled reason,
  and dependency health
- `--json` returns `SkillInspectResult`
- unknown skill -> `6`

## `skills paths`

- returns every searched skill path and the winning manifest for each discovered
  skill ID
- `--json` returns `SkillPathsResult`

## `skills validate <skill-id?>`

- validates one skill or the whole registry against manifest schema and runtime
  dependency availability
- clean `--json` success returns `SkillValidationResult`
- degraded-but-loadable skills return `11`
- exit `11` with `--json` must return
  `CommandFailure.data = FollowupRequiredResult`

## `skills outputs list`

- lists governed skill output envelopes under `.pmcli/skills/outputs`
- `--json` returns `SkillOutputListResult`
- public state is derived from envelope visibility; private and review
  candidates remain explicit, non-canonical records

## `skills output submit`

- submits one or more skill artifacts as a governed `SkillOutputEnvelope`
- validates workspace-relative artifact refs, optional DocFrame candidates,
  output kind, publication policy, human-gate requirements, and the discovered
  `ResearchSkillContract`
- `--json` returns `SkillOutputSubmitResult`

## `skills output inspect <envelope-id>`

- inspects a specific envelope without publishing it
- `--json` returns `SkillOutputInspectionResult`
- unknown envelope -> `6`

## `skills run`

- runs a skill through a runtime adapter while preserving the skill's native
  directory and command conventions
- current graduated adapter: `--adapter local-command`
- sets `RESEARCH_CLI_SKILL_DIR` for the command, records file-level side
  effects, enforces contract `allowed_write_scopes`, then submits declared
  artifacts through the same governed envelope path as `skills output submit`
- `--json` returns `SkillRunResult`
- contract mismatch -> `7`; runner failure -> `13`

## `skills publish`

- `--inspect <envelope-id>` records a publication-gate inspection decision
  without changing canonical project truth
- `--execute <envelope-id>` promotes a permitted envelope to public latest,
  supersedes the previous public envelope for the artifact family, updates
  `.pmcli/artifacts/families/<family>.json`, and refreshes
  `.pmcli/canonical_surface.json.skill_outputs`
- human-gated outputs require `--approve-human-gate`
- `--json` returns `SkillOutputPublishResult`

## `plugins list`

- lists discovered plugins, enabled state, hook count, and degraded plugins
- `--json` returns `PluginListResult`

## `plugins inspect <plugin-id>`

- returns manifest origin, hook surface, tool additions, config source, and
  last-known degraded reason
- `--json` returns `PluginInspectionResult`
- unknown plugin -> `6`

## `plugins validate <plugin-id?>`

- validates one plugin or the effective plugin registry against manifest and
  runtime dependency policy
- clean `--json` success returns `PluginValidationResult`
- degraded-but-loadable plugins return `11`
- exit `11` with `--json` must return
  `CommandFailure.data = FollowupRequiredResult`

## `hooks list`

- lists discovered hooks, owner plugin or built-in owner, trigger phase, and
  effective enablement state
- `--json` returns `HookListResult`

## `hooks inspect <hook-id>`

- returns trigger, source plugin, timeout policy, failure policy, and last
  health record
- `--json` returns `HookInspectionResult`
- unknown hook -> `6`

## `hooks test <hook-id?>`

- runs dry-run or sandboxed validation of one hook or all hooks at a named
  trigger point
- `--json` returns `HookTestResult`
- destructive hooks must refuse unsafe test mode with exit `5` and
  `CommandFailure.data = PolicyRefusalResult`, or exit `8` with
  `CommandFailure.data = FeatureGateResult` when sandboxed hook testing is not
  graduated

## `mcp list`

- lists MCP servers, tool counts, auth state, and degraded status
- `--json` returns `MCPListResult`

## `mcp inspect <server>`

- returns transport type, tool surface, disabled tools, auth source, timeout,
  and last-known failure
- `--json` returns `MCPInspectionResult`
- unknown server -> `6`

## `mcp test <server?>`

- runs handshake and minimal tool-surface validation
- clean `--json` success returns `MCPTestResult`
- partial startup must report degraded state rather than pass/fail only
- degraded-but-usable MCP startup returns `11` with
  `CommandFailure.data = FollowupRequiredResult`

## `mcp refresh`

- re-resolves configured MCP servers and recomputes effective tool exposure
- `--json` returns `MCPRefreshResult`
- emits `event_name=mcp_registry`

## Runtime registry productization rule

To clearly exceed the current baseline, `skills`, `mcp`, `providers`, and
`setup` may not remain generic placeholders.

The same productization rule applies to `plugins` and `hooks`.

They must behave like real registry surfaces with:

- explicit discovery paths
- explicit health or degradation state
- explicit winning config or manifest source
- explicit validation or repair lanes

The same rule now applies to `agents`, `branches`, and `reviews`:

- explicit operator-visible records
- explicit stop/cancel/retry/promotion lanes
- explicit evidence and trace references
- explicit policy-gated refusal modes

## 10. Remote Commands

Remote command behavior is layered further in
`25-remote-transport-auth-contract.md`.

## `remote pair`

- pairs or refreshes a machine identity
- `--json` returns `RemotePairResult`
- emits `event_name=remote_pair`

## `remote status`

- returns current machine pairing, bindings, ownership, revocation state, and
  degraded remote features
- `--json` returns `RemoteStatusReport`
- emits `event_name=remote_status`

## `remote attach`

- default mode is inspect-only; inspect returns `AttachEligibility`
- execute mode is explicit via `--execute` and returns `AttachExecutionResult`
- frozen grammar:
  `remote attach --session <session-id> [--strategy <provider_attach|terminal_host>] [--inspect|--execute]`
- stale lease, workspace mismatch, or disabled feature returns exit `9`, `7`,
  or `8` with `CommandFailure.data = RemoteActionRejection`
- exit `11` with `--json` must return
  `CommandFailure.data = FollowupRequiredResult` when local handback or remote
  acknowledgment is still required before control mutates
- emits `event_name=remote_attach`

## `remote handoff`

- default mode is inspect-only; inspect returns `HandoffEligibility`
- execute mode is explicit via `--execute` and returns
  `HandoffExecutionResult`
- frozen grammar:
  `remote handoff --session <session-id> --target-client <client-id> [--inspect|--execute]`
- stale lease epoch, unsupported target surface, or disabled feature returns `9`
  or `8` with `CommandFailure.data = RemoteActionRejection`
- exit `11` with `--json` must return
  `CommandFailure.data = FollowupRequiredResult` when the target surface must
  acknowledge focus transfer before ownership changes
- emits `event_name=remote_handoff` and, on ownership change,
  `event_name=remote_control_owner`

## `remote takeover`

- transfers control ownership via explicit lease handoff
- `--json` returns `RemoteTakeoverResult`
- stale owner or lease conflict returns `9`
- refusal with `--json` must return
  `CommandFailure.data = RemoteActionRejection`
- emits `event_name=remote_takeover` and `event_name=remote_control_owner`

## `remote notify`

- sends a typed notification into the remote host substrate
- `--json` returns `RemoteNotifyResult`
- `RemoteNotifyResult.notification` must serialize the canonical
  `RemoteNotificationPayload` object, not only delivery receipts
- emits `event_name=remote_notify`

## 11. Feature Graduation Rule

The command surface is intentionally broader than Milestone 4.

To stop shell-first overclaiming, every advanced command must expose one of two
states:

- `graduated` - full contract is active and conformance-backed
- `not_graduated` - command name exists, but returns exit `8` with
  `CommandFailure.data = FeatureGateResult`

This keeps the operator surface stable without pretending unfinished subsystems
already exist.

## 12. Fixture Obligations

Every shipped command family must have at least one golden fixture that proves:

- success path
- target-not-found or ambiguity path
- policy or degraded-state failure path
- JSON envelope compliance
- emitted event ordering

Minimum fixture ownership:

- base CLI and project/session/config/provider surfaces:
  `17-operator-golden-fixtures.md`
- kernel blocked-start and degraded-state surfaces:
  `21-code-agent-kernel-hardening.md`
- remote operator surfaces:
  `22-remote-command-and-fixture-contract.md`
- state-machine transitions and invariants:
  `24-kernel-state-machine-contract.md`

## 13. Why This Now Exceeds The Baselines

The operator surface is now stronger than a command inventory because it pins:

- exact output envelopes
- exact exit codes
- exact mutation and event expectations
- exact graduation rules for unfinished advanced layers
- exact session-title/recap behavior and runtime-registry productization

That is the missing operational detail the design still lacked in the previous
review round.
