# Executable Runtime And `.pmcli` Protocol Spec

This document turns the runtime side of the architecture into a concrete
protocol contract.

It is the missing operational layer between the design docs and code.

## 1. Scope

This spec defines:

- the executable state model
- stage ownership and transition rules
- failure and recovery behavior
- the `.pmcli/` file protocol
- atomic write and migration semantics
- conformance expectations

## 2. Reference-Derived Rules

The protocol is intentionally borrowed from the strongest concrete patterns in
the reference projects:

- `memoryOSS` gives the versioned contract + conformance-kit model
- `Memory-Palace` gives the write lane, snapshot-before-mutate, and recovery-first memory discipline
- `Memory-Palace-Openclaw` gives measurable write-lane metrics and flush tracking
- `mempalace` gives fast pointer-first retrieval over verbatim drawers
- `ARIS` gives reviewer independence and traceable review outputs
- `Claw Code` gives doctor/parity/session ergonomics

## 3. Executable State Model

The top-level runtime and checkpoint object is `KernelStateBundle`.

`ProjectState` is the project-scoped child payload inside that bundle. It is a
coordinator, not a transcript, but it is no longer a competing top-level truth.

The trigger labels used in the transition tables below are logical causes.
Serialized runtime events must use the canonical event envelope frozen in
`24-kernel-state-machine-contract.md`; terminality is carried by envelope
fields, not by table-specific suffix conventions.

### State Families

| Family | Owner | Persistence | Mutation rule |
|---|---|---|---|
| runtime | runtime | project | append events, checkpoint state |
| session | runtime | workspace-scoped | append transcript, compact on thresholds |
| agent | director/runtime | agent-local | isolated by default |
| review | reviewer | review-local | fresh-thread by default |
| branch | branch coordinator | batch-local | score, adjudicate, archive |
| memory | memory service | project | write lane + snapshot before destructive change |
| repo | synthesizer | project | canonicalize, archive, cleanup |

### Orthogonal Substates

The top-level stage is only the coordinator. Each family also has its own
substate machine so the system can handle concurrency without hiding it.

These are the canonical family states and must stay aligned with
`24-kernel-state-machine-contract.md`.

| Family | States |
|---|---|
| session | `opening`, `open`, `active`, `compacting`, `paused`, `archived`, `resume_blocked` |
| agent | `spawning`, `ready`, `running`, `blocked`, `awaiting_input`, `completed`, `failed`, `stopped` |
| review | `queued`, `packet_ready`, `running`, `awaiting_compare`, `passed`, `failed`, `cancelled`, `archived` |
| branch | `allocated`, `queued`, `running`, `needs_refresh`, `awaiting_evaluation`, `awaiting_review`, `promotable`, `won`, `lost`, `archived`, `gc_eligible` |
| memory | `idle`, `writing`, `indexing`, `consolidating`, `invalidating` |
| repo | `clean`, `dirty`, `superseding`, `cleaning`, `archived` |

These family-local states are not a second terminal-outcome vocabulary.

They describe submachine progress only.

Whenever a mutation lane actually terminates, the canonical terminal outcome
must still collapse to one of the bundle-wide labels frozen in
`24-kernel-state-machine-contract.md`:

- `succeeded`
- `failed`
- `cancelled`
- `blocked`
- `stale_conflict`

### Top-Level Stages

| Stage | Owner | Meaning |
|---|---|---|
| `booting` | runtime | load config, validate state, initialize services |
| `ready` | runtime | accept input and route work |
| `intake` | planner | classify the request |
| `planning` | planner | decompose work and budgets |
| `exploring` | branch coordinator | branch search / debate / grid search |
| `executing` | executor | make bounded changes or gather evidence |
| `debating` | branch coordinator | compare branches or hypotheses |
| `reviewing` | reviewer | independent judgment from primary artifacts |
| `consolidating` | synthesizer | merge evidence into memory and repo state |
| `promoting` | synthesizer/director | choose append/fork/supersede/reject |
| `compacting` | synthesizer/runtime | write summaries and reduce working state |
| `idle` | runtime | wait for next task |
| `recovering` | runtime | repair after failure or inconsistency |
| `shutting_down` | runtime | flush and close |

### Transition Rules

| Event | Guard | Action | Next |
|---|---|---|---|
| `turn_started` | state valid | load session, budgets, workspace | `intake` or `ready` |
| `plan_accepted` | permissions resolved | freeze plan and spawn work | `executing` |
| `branch_requested` | budget available | create batch + workspaces | `exploring` |
| `review_open` | fresh packet | emit blinded review trace | `reviewing` |
| `review_passed` | evidence complete | promote or archive | `promoting` |
| `artifact_promoted` | canonical slot free/replaceable | update family pointers | `consolidating` |
| `memory_written` | write lane acquired | snapshot then append | prior stage |
| `flush_ready` | compaction threshold met | write summary + checkpoint | `compacting` |
| `invariant_broken` | any | enter recovery path | `recovering` |
| `shutdown_requested` | drains complete | flush and close | `shutting_down` |

### Invariants

- every transition is append-only in `transition_log`
- every destructive write is preceded by a snapshot
- every canonical artifact has exactly one latest pointer
- every review is independent unless explicit compare mode is requested
- every branch result records score provenance

## 4. Failure And Recovery

| Failure | Required behavior |
|---|---|
| permission denial | stay in stage, emit `event_name=permission` |
| lock contention | bounded retry, then recover or fail closed |
| schema mismatch | run setup/doctor path, do not guess |
| review timeout | fresh-thread rerun with the same packet shape |
| branch loss | archive the branch, never silently merge |
| checkpoint failure | block promotion until checkpoint succeeds |
| index worker crash | replay queued work from durable queue |
| workspace drift | re-resolve workspace identity before write |

Recovery must resume from the last verified checkpoint, not from an
in-memory assumption.

## 5. `.pmcli/` Protocol

`.pmcli/` is a versioned protocol surface, not an incidental cache directory.

### Canonical File Classes

| Class | Write mode | Examples |
|---|---|---|
| append-only log | append | `events/events.jsonl`, `sessions/*/transcript.jsonl`, `TRACE.jsonl` |
| snapshot file | replace atomically | `project_state.json`, `OUTPUT_MANIFEST.json` |
| manifest registry | replace atomically | `artifacts/families.json` |
| durable queue | append + ack | `memory/pending_index.jsonl`, `reviews/pending_retry.jsonl` |
| derived cache | rebuildable | `recall_cache.sqlite`, index files |
| archive | immutable once written | timestamped outputs, old review traces |

### Canonical Project-Local Layout

The project-local protocol root is singular and immutable once published:

```text
.pmcli/
├── project_state.json
├── project_meta.json
├── events/
│   └── events.jsonl
├── indexes/
│   └── session_search.sqlite
├── sessions/
│   └── <session_id>/
│       ├── transcript.jsonl
│       ├── summary.md
│       └── meta.json
├── agents/
├── memory/
├── artifacts/
├── branches/
├── reviews/
├── remote/
└── repo/
```

Milestone bootstrap may create only a subset of these directories, but it may
not introduce alternative canonical names for events or session storage.

### Global Operator Registry

Cross-project operator state is stored outside project-local `.pmcli/`.

Canonical location:

- `$XDG_STATE_HOME/research-cli/registry/`
- fallback: `~/.local/state/research-cli/registry/`

Canonical files:

- `projects.json`
- `current_project.json`

Rules:

- this registry is an operator index, not runtime authority
- it may point to `data_dir`, but the canonical project runtime truth remains
  the selected project's `.pmcli/`
- registry damage must not corrupt project-local runtime state

### Required Metadata

Every enveloped canonical object must carry:

- `schema_version`
- `runtime_version`
- `migration_version`
- `conformance_line`
- `canonical_path`
- `retention_policy`
- `atomic_write_policy`

### `CanonicalObjectEnvelope`

Persisted protocol objects use one universal wrapper:

```text
CanonicalObjectEnvelope {
  schema_version
  runtime_version
  migration_version
  conformance_line
  canonical_path
  retention_policy
  atomic_write_policy
  payload
}
```

Object shapes defined in other docs are payload schemas unless explicitly
marked as full persisted envelopes.

### Canonical Encoding Rules

The protocol must not leave envelope-vs-payload as an implementation choice.

Canonical rule by file class:

| File class | Encoding rule |
|---|---|
| snapshot JSON object | `CanonicalObjectEnvelope` wrapping one payload object |
| manifest/registry JSON object | `CanonicalObjectEnvelope` wrapping one payload object |
| append-only event log | line records, one `KernelEventEnvelope` per line |
| append-only transcript log | line records, one typed transcript record per line |
| durable queue log | line records, one queue record per line |
| derived markdown view | plain derived text, not an envelope |
| rebuildable cache/db | implementation-defined cache format, not canonical authority |

This means:

- `project_state.json` is an enveloped snapshot
- `project_meta.json` is an enveloped snapshot
- `projects.json` and `current_project.json` are enveloped registry objects
- `events/events.jsonl` is a line-record event log
- `sessions/<session_id>/transcript.jsonl` is a line-record transcript log
- `sessions/<session_id>/summary.md` is derived text only

### Commit And Privacy Policy

The protocol should explicitly classify `.pmcli/` content as:

- commit-safe metadata
- local cache only
- private trace data
- machine-local runtime state

Review traces, local caches, and sensitive provider/runtime artifacts should be
gitignored by default unless explicitly exported for audit.

### Atomic Write Rules

1. write to a temp file in the same directory
2. fsync the data
3. rename atomically over the target
4. fsync the parent directory
5. only then advance pointers or manifests

Append-only files still go through the write lane so that foreground writes and
background maintenance share one serialized path.

### Lock Order

The runtime must always acquire locks in this order:

1. session lock
2. family or review lock
3. global write lane

This avoids deadlocks when a session write triggers a memory or artifact update.

### Checkpoint Reconciliation Rule

Atomic writes prevent torn files, but they do not by themselves prevent
last-writer-wins corruption.

Therefore:

1. all mutating subsystems append canonical events before requesting checkpoint
   publication
2. only the checkpoint reducer may publish `project_state.json`
3. every reducer attempt declares a `base_seq_cursor` and `base_checkpoint_epoch`
4. reducer loads the latest committed bundle, verifies the base values, replays
   unapplied events from `events/events.jsonl`, and emits a new bundle
5. if the committed bundle has advanced, the reducer must reload and replay
   instead of overwriting
6. checkpoint publication advances both `seq_cursor` and `checkpoint_epoch`

Minimum bundle fields required for reconciliation:

- `seq_cursor`
- `checkpoint_epoch`

Replay rule:

- project-local events are the mutation journal
- the bundle is the current committed projection of that journal
- recovery resumes from the last committed bundle plus unapplied events

### Migration Rules

- additive fields are allowed within a published line
- unknown additive fields must be ignored by readers
- breaking changes require a new line or version
- writers default to the latest published line
- old readable fixtures must continue to pass conformance until explicitly retired

### Published Lines

The conformance line should be explicit and immutable once published:

| Line | Scope |
|---|---|
| `pmcli.runtime.v1alpha1` | runtime state, events, checkpoints |
| `pmcli.session.v1alpha1` | session transcripts and compaction |
| `pmcli.agent.v1alpha1` | agent task packets, traces, and manifests |
| `pmcli.memory.v1alpha1` | working and durable memory records |
| `pmcli.review.v1alpha1` | review packets and trace metadata |
| `pmcli.artifact.v1alpha1` | artifact families and promotion pointers |
| `pmcli.branch.v1alpha1` | branch batches and adjudication records |
| `pmcli.research.v1alpha1` | research workflow nodes and outputs |
| `pmcli.registry.v1alpha1` | cross-project registry and current-project pointer |
| `pmcli.remote-machine.v1alpha1` | machine-global remote pairing and capability authority |
| `pmcli.remote-project.v1alpha1` | project-local remote bindings, cursors, and projections |

### Remote Persistence Rule

Remote control state is part of the canonical protocol, not an out-of-band app
cache.

Remote persistence is split into two authority scopes.

Machine-global remote authority:

- `$XDG_STATE_HOME/research-cli/remote/machine_identity.json`
- `$XDG_STATE_HOME/research-cli/remote/lease.json`
- `$XDG_STATE_HOME/research-cli/remote/capabilities.json`

Project-local remote authority:

- `remote/BINDINGS.json`
- `remote/cursors/<session_id>.json`
- `remote/projections/<session_id>.json`

Rules:

- machine-global lease and capability persistence must validate before remote
  control resumes after restart
- project-local bindings/cursors/projections must validate before project-local
  follow or takeover resumes after restart
- remote cursors are durable replay checkpoints, not best-effort hints
- remote projections are derived from kernel truth but retained for reconnect
  continuity and audit

### Agent Task Packet Rule

Every launched agent must have one canonical task packet persisted before
execution begins.

Required protocol object:

```text
agents/<agent_id>/TASK_PACKET.json
```

Rules:

- validates against the published `pmcli.agent.v1alpha1` schema
- is the durable source of truth for allowed paths, budgets, and permission mode
- must exist before the agent lifecycle may enter `running` and emit canonical
  start/update/terminal events under the shared event-phase model
- is referenced by agent traces, review packets, and branch evidence when
  downstream promotion or audit needs provenance

### Retention Rules

- raw session logs are retained until compaction or cleanup
- review traces are retained as audit history
- derived caches may be dropped and rebuilt
- archives are immutable once promoted

### Durable Queue Rule

Any background work that must survive process death needs a durable queue entry
under `.pmcli/` before execution starts.

Minimum required queues:

- `memory/pending_index.jsonl`
- `memory/pending_rollup.jsonl`
- `memory/pending_summary_review.jsonl`
- `reviews/pending_retry.jsonl`
- `artifacts/pending_promotion.jsonl`

Queue consumers may mark items complete, but they must never silently delete
unfinished work from memory only.

## 6. Conformance Kit

The repository should ship:

```text
schemas/
tests/conformance/
tests/golden/
```

Optional exported `.pmcli/conformance_snapshot/` artifacts may be generated for
debugging or audit, but they are derived exports only and not schema authority.

Conformance must test:

- schema validation
- round-trip load/save
- migration from the previous line
- additive-field tolerance
- atomic write recovery
- lock contention behavior
- checkpoint resume behavior

## 7. Why This Matters

This is the floor that makes the rest of the system safe:

- the code-agent kernel becomes resumable
- memory becomes recoverable instead of magical
- multi-agent debate becomes auditable
- repo cleanup becomes deterministic
