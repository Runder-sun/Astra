# Proactive Project Ops And Personalized Governance

This document hardens the most user-specific part of the design:

- native project-level memory
- silent but auditable progress summarization
- active repo cleanup and anti-chaos governance
- multi-agent research operation with queue/supervisor semantics
- personalized query and explain surfaces over the whole project

The goal is to make these capabilities first-class runtime behavior rather
than optional scripts bolted onto a code agent.

## 1. Why This Document Exists

The earlier documents define:

- the runtime protocol (`14`)
- the research/review contracts (`15`)
- the kernel surface and parity contract (`16`)
- the exact golden cases (`17`)

What is still missing is one unified contract for the proactive project
services that turn those pieces into the product the user actually wants:

- remembers project progress without becoming noisy
- organizes the repository without becoming destructive
- supervises long-running research execution
- explains exactly why something was remembered, promoted, archived, or pruned

## 2. Reference-Derived Operational Lessons

### MemPalace

Contributes:

- pointer-first retrieval
- layered recall
- memory organization that can be traversed and explained

Rule we keep:

- recall should fetch references first and only hydrate verbatim evidence on demand

### Memory-Palace And Memory-Palace-Openclaw

Contributes:

- snapshot-before-mutate discipline
- rollback-minded write lanes
- runtime-state awareness around consolidation and governance

Rule we keep:

- silent project maintenance is allowed only when it is reversible or pre-snapshotted

### hippo-memory

Contributes:

- invalidation
- forgetting and decay
- persistence must be earned

Rule we keep:

- durable memory is not a write-only bucket; stale records should lose injection priority

### Sibyl / ARIS

Contributes:

- queue versus direct execution
- supervisor state and wake queues
- recovery-aware experiment tracking
- control-plane contracts verified by tests

Rule we keep:

- long-running research work must have leases, heartbeats, wake events, and resumable status

### autoresearch

Contributes:

- tiny mutation surface
- fixed-budget experiments
- clear keep/discard loop based on one comparable metric

Rule we keep:

- whenever possible, autonomous exploration should operate inside a bounded
  change envelope with explicit score provenance

### GPT Researcher

Contributes:

- planner / executor / publisher separation
- branching information gathering

Rule we keep:

- research work should separate evidence gathering, implementation, and synthesis

## 3. Core Service: `ProjectOps`

The system needs a dedicated proactive service spanning runtime, memory, repo,
and research layers.

`ProjectOps` is not a hidden agent. It is a governed runtime service.

Responsibilities:

- observe structured events
- decide whether a project-ops tick should run
- produce candidate summaries
- open repo cleanup proposals
- supervise queued research runs
- detect drift, stale branches, and orphan worktrees
- surface explainable project state to the operator

## 4. Core Objects

### `ProjectOpsTick`

```text
ProjectOpsTick {
  tick_id
  trigger
  project_id
  session_id?
  branch_id?
  review_id?
  run_id?
  started_at
  finished_at?
  actions_taken[]
  degraded_reasons[]
}
```

Triggers may include:

- `turn_end`
- `session_compacted`
- `branch_adjudicated`
- `review_resolved`
- `run_completed`
- `repo_drift_detected`
- `scheduled_maintenance`
- `manual_request`

### `ProgressDigestCandidate`

```text
ProgressDigestCandidate {
  candidate_id
  project_id
  scope
  summary_text
  supporting_event_ids[]
  supporting_artifact_ids[]
  supporting_session_spans[]
  promotion_status
  confidence
}
```

### `RepoCleanupProposal`

```text
RepoCleanupProposal {
  proposal_id
  project_id
  detected_duplicates[]
  stale_paths[]
  orphan_worktrees[]
  superseded_artifacts[]
  safe_actions[]
  risky_actions[]
  requires_human_gate
}
```

### `ExperimentSupervisorLease`

```text
ExperimentSupervisorLease {
  lease_id
  run_id
  owner_agent_id
  state
  claimed_at
  heartbeat_at
  stale_after_sec
  pending_wake_count
  last_summary
}
```

### `WakeEvent`

```text
WakeEvent {
  wake_id
  run_id
  owner_agent_id
  kind
  urgency
  requires_main_system
  summary
  details
}
```

### `ChangeEnvelope`

```text
ChangeEnvelope {
  envelope_id
  objective
  allowed_paths[]
  forbidden_paths[]
  evaluation_contract
  rollback_hint?
}
```

This is the generalized version of the bounded mutation surface suggested by
`autoresearch`.

## 5. Project-Ops Trigger Matrix

| Trigger | Default action | Human gate |
|---|---|---|
| turn end | consider working-memory rollup | no |
| session compaction | write digest candidate | no |
| branch adjudication | archive losers, refresh cleanup proposal | no |
| review resolved | update memory/artifact eligibility | no |
| run completed | write run digest + result-to-claim input | no |
| repo drift detected | open cleanup proposal | sometimes |
| destructive cleanup | never auto-apply without policy | yes |
| design/code/result contradiction | open repair task and mark memory contested | yes for promotion |

The system should feel proactive, not reckless.

## 6. Silent Progress Summarization Contract

Silent summarization is one of the main personalized requirements, but it
must be auditable.

Required stages:

1. collect supporting events and artifacts
2. produce `ProgressDigestCandidate`
3. attach confidence and degraded reasons
4. run promotion rule
5. either:
   - promote to durable memory
   - keep as candidate
   - reject
   - mark contested

Required rules:

- no silent summary may become trusted durable memory without support links
- summaries derived from unresolved debates are marked contested
- summaries that depend on superseded artifacts inherit that status
- digests are project-scoped, not just session-scoped

## 7. Active Memory Query And Personalized Explain

The operator must be able to ask:

- what do you remember about this project?
- why do you remember it?
- what changed recently?
- which memories are stale, contested, or superseded?

Required surfaces:

- `research-cli memory query`
- `research-cli memory explain`
- `research-cli memory status`
- `research-cli projects status`
- `research-cli inspect --project`

Required output dimensions:

- source path
- source span or artifact reference
- memory scope
- confidence
- contested/superseded status
- last validation time

This is the minimum needed to claim native active memory rather than hidden
prompt stuffing.

## 8. Active Repo Governance Contract

The repo manager should continuously watch for chaos, but apply actions
carefully.

Required detections:

- duplicate outputs for the same artifact family
- stale result directories
- orphan worktrees
- abandoned branch sandboxes
- multiple "latest" files for one logical output
- doc/code/result divergence

Required actions:

- produce cleanup proposals
- archive losers and superseded outputs
- refresh canonical/latest pointers
- open repair tasks when divergence is semantic rather than file-level

Forbidden actions:

- deleting canonical outputs without a snapshot
- hiding ambiguous cleanup under a silent auto-apply
- overwriting timestamped history

## 9. Research Run Supervision Contract

The research stack needs long-running operations that survive interruption.

Required behavior:

- queueable runs acquire an `ExperimentSupervisorLease`
- supervisor writes heartbeats
- unresolved problems emit `WakeEvent`s
- recovery loads durable run state before deciding next action
- results are written before claim judgment starts

This is the part where the design should clearly exceed simple
"spawn agents and hope" systems.

### Queue-versus-direct rule

Runs are `direct` only when all are true:

- small scope
- low cost
- no multi-seed scheduler need
- no long-running detached supervision needed

Otherwise they route through the queued supervisor contract.

### Keep/discard rule

Each exploratory run should define:

- metric to optimize
- minimum integrity bar
- promotion threshold
- discard/archive condition

This is the reusable lesson from `autoresearch`.

## 10. Personalized Project Management Surfaces

The CLI should make project management feel native, not bolted on.

Required user-facing capabilities:

- ask for recent project progress
- inspect current project health
- inspect pending cleanup proposals
- inspect active research runs and wake queue
- inspect what silent summaries were promoted recently
- ask why a file or result was archived
- do all of the above from remote mobile/web surfaces when connected through the host control plane

This implies first-class commands such as:

- `research-cli projects status`
- `research-cli repo cleanup-plan`
- `research-cli branches`
- `research-cli reviews`
- `research-cli memory explain`
- `research-cli inspect --project`

## 11. Implementation Guidance

`ProjectOps` should not be one giant file or loop.

Recommended ownership split:

- `internal/projectops/tick.go` - trigger evaluation and scheduling
- `internal/projectops/digest.go` - progress digest candidate generation
- `internal/projectops/cleanup.go` - repo cleanup proposal generation
- `internal/projectops/supervisor.go` - run lease, heartbeat, wake handling
- `internal/projectops/explain.go` - project-level explain/query assembly

This preserves kernel clarity while making the personalized layer concrete.

## 12. What "Clearly Better" Means Here

The system exceeds the reference projects on personalized capabilities only if:

1. project memory is queryable and explainable
2. silent summarization is support-linked and promotion-gated
3. repo cleanup is proactive but reviewable
4. long-running research work has supervisor leases and wake events
5. all of the above are visible through first-class CLI/TUI surfaces

If these behaviors stay implicit, the design is still not strong enough.
