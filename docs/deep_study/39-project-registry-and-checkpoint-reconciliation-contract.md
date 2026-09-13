# Project Registry And Checkpoint Reconciliation Contract

This document closes two architecture gaps that proved too important to leave
implicit:

- cross-project registry ownership
- concurrent checkpoint publication semantics

These are foundation contracts, not optional implementation preferences.

## 1. Why This Document Exists

External review correctly identified that the architecture required:

- project-level active management across multiple workspaces
- one `KernelStateBundle` authority across multiple orthogonal submachines

but had not yet frozen:

- who owns the cross-project registry
- where that registry lives
- how concurrent runtime families safely publish one checkpoint bundle

This document resolves those gaps.

## 2. Two Authority Layers

The system has exactly two persistence authority layers, with different roles.

## Layer A: Project-local runtime authority

Location:

- `<workspace_root>/.pmcli/`

Owns:

- `project_state.json`
- project-local event log
- session transcripts and summaries
- advanced runtime state for agents, memory, reviews, branches, remote, repo

This is the only source of runtime truth for one project.

## Layer B: Global operator registry

Location:

- `$XDG_STATE_HOME/research-cli/registry/`
- fallback: `~/.local/state/research-cli/registry/`

Owns:

- project registry entries
- current-project pointer
- recency and init-state metadata

This is a global operator index only.

It may reference project-local state.

It may not override project-local runtime truth.

## 3. Global Registry Files

Canonical layout:

```text
registry/
├── projects.json
├── current_project.json
└── locks/
```

## `projects.json`

Stores a list or map of canonical `ProjectRegistryEntry` records.

Minimum fields:

```text
ProjectRegistryEntry {
  project_id
  workspace_root
  workspace_hash
  data_dir
  last_accessed_at
  init_state
  active_session_id?
  open_branch_count?
  repo_health?
}
```

Rules:

- `data_dir` defaults to `<workspace_root>/.pmcli`
- registry entries are keyed by `project_id`
- `workspace_root` uniqueness is enforced after path normalization
- dead entries may be pruned, but prune must default to dry-run
- file encoding is `CanonicalObjectEnvelope` with registry payload

## `current_project.json`

Stores the last successfully selected project pointer.

Minimum fields:

```text
CurrentProjectPointer {
  project_id
  workspace_root
  updated_at
}
```

Rules:

- it is a convenience pointer, not a substitute for scope resolution
- explicit CLI scope flags always outrank it
- a stale pointer must fail cleanly rather than silently redirect
- file encoding is `CanonicalObjectEnvelope` with pointer payload

## 4. Ownership

Canonical module owner:

- `internal/projects`

Required first-build files:

- `internal/projects/registry.go`
- `internal/projects/current.go`

Required first-build responsibilities:

- register/list/get/prune project entries
- persist current-project pointer
- resolve `data_dir`
- update recency on successful session open/resume

## 5. Current-Project Resolution Order

Canonical resolution order:

1. explicit `--project`
2. explicit `--cwd`
3. workspace detection from current directory
4. stored current-project pointer

Rules:

- ambiguity fails closed
- pointer fallback may only select one registered project
- registry lookup never rewrites project-local runtime state

## 6. Checkpoint Publication Problem

The runtime uses one `KernelStateBundle`, but multiple submachines may mutate
state:

- session
- turn
- permission
- feature
- agent
- review
- branch
- memory
- artifact
- projectops
- experiment supervision
- research stage
- repo
- remote

Atomic file replacement protects against torn writes.

It does not protect against concurrent last-writer-wins corruption.

So checkpoint publication must be a reducer contract.

## 7. Checkpoint Reducer Contract

Only the checkpoint reducer may publish `project_state.json`.

Every mutating subsystem follows this rule:

1. append canonical event(s) to `events/events.jsonl`
2. submit or trigger checkpoint publication
3. let the reducer compute the next committed bundle

Canonical reducer inputs:

```text
CheckpointPublishPlan {
  base_seq_cursor
  base_checkpoint_epoch
  touched_families[]
}
```

Allowed `touched_families[]` values for first release:

- `session`
- `turn`
- `permission`
- `feature`
- `agent`
- `review`
- `branch`
- `memory`
- `artifact`
- `projectops`
- `experiment_supervision`
- `research_stage`
- `repo`
- `remote`

Canonical bundle fields required for reconciliation:

```text
KernelStateBundle {
  ...
  seq_cursor
  checkpoint_epoch
}
```

## 8. Reducer Algorithm

The reducer must:

1. acquire the global write lane
2. load the latest committed `project_state.json`
3. verify `base_seq_cursor` and `base_checkpoint_epoch`
4. if stale, reload plan state from latest committed bundle and continue by
   replay, not overwrite
5. read unapplied events from `events/events.jsonl`
6. replay those events through canonical family reducers
7. emit a new bundle with advanced `seq_cursor` and `checkpoint_epoch`
8. publish atomically

Rules:

- reducers are deterministic over the same prior bundle + event range
- event replay is the source of mutation history
- bundle publication is the committed projection of that event history
- if replay fails, the runtime must enter recovery rather than guess
- any advanced-family reducer must remain replayable from canonical events; no
  memory digest, wake escalation, or research-stage projection may depend on an
  unlogged side channel

## 9. Failure Semantics

If a publisher discovers that its base cursor or epoch is stale:

- it must not overwrite the committed bundle
- it must reload and replay
- it may retry a bounded number of times
- repeated failure becomes a recovery event

If the event log contains invalid or unreadable lines:

- checkpoint publication fails closed
- `doctor` and `setup repair-hints` must expose the issue

## 10. M0-M2 Minimum Implementation Requirement

M0-M2 does not need every advanced family reducer.

But it does need:

- the reducer ownership model
- `seq_cursor`
- `checkpoint_epoch`
- stale publish detection
- replay from committed bundle plus unapplied events

Otherwise the foundation is not actually safe for later multi-agent and
background-runtime features.

## 11. Required Fixtures

Before the design is considered implementation-ready, the fixture plan must
include:

- project registry round-trip
- current-project pointer fallback
- stale pointer failure
- stale checkpoint publish retry
- concurrent session + permission update replay
- restart from bundle plus unapplied events
- memory + artifact invalidation replay after rollback
- projectops tick and digest replay from canonical event history
- wake escalation replay from supervisor lease plus wake events
- research-stage repair/pivot replay without hidden stage-local state

These fixtures are not advanced nice-to-haves.

They are the proof that the architecture can scale into the later milestones
without rewriting its foundation.
