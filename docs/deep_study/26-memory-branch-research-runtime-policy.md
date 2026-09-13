# Memory, Branch, And Research Runtime Policy

This document freezes the runtime policy layer that sits behind the advanced
surfaces introduced in:

- `10-revised-final-architecture.md`
- `15-research-review-and-contracts.md`
- `16-kernel-parity-contract.md`
- `18-proactive-project-ops.md`

The earlier documents defined the nouns well.

The missing piece was policy:

- how retrieval is ranked and budgeted
- who may promote or invalidate memory
- how branch scheduling and retries work
- how research skills execute stage by stage

This document makes those advanced layers concrete enough to implement without
guessing.

## 1. Governing Rule

Advanced layers are not allowed to bypass the code-agent kernel.

That means:

- memory retrieval is a service behind turns and inspections
- branch search is a scheduler over workspace-bound executors
- research skills compile into stage graphs that use kernel sessions,
  artifacts, reviews, and remote controls

No advanced lane may invent a second event model or second permission model.

The same rule applies to project goals. The runtime may inject a persistent
goal hierarchy, but it must come from schema-owned project state, not from an
ad hoc prompt snippet.

Project-internal goal priority is:

```text
project_max_goal > milestone_goal > current_implementation_goal
```

This hierarchy guides implementation choices, compaction, resume, subagent
launches, and review. It does not outrank system/developer/user instructions,
safety policy, permission policy, or explicit operator stop/rollback requests.

The canonical design is `48-mission-frame-context-and-compact-policy.md`.

## 2. Memory Retrieval Pipeline

Memory retrieval must be deterministic enough to explain and test.

### Retrieval phases

1. scope resolution
2. candidate generation
3. evidence hydration
4. ranking
5. budget cutting
6. explain record generation
7. injection or inspect-only return

### Scope resolution

The retrieval scope is resolved in this order:

1. current turn/session scope
2. current project durable memory
3. branch-local memory if a branch session is active
4. cross-project memory only when explicitly requested

### Candidate generation lanes

- lexical search over titles, tags, and summaries
- embedding or semantic recall over durable records
- pointer-first artifact and transcript references
- mission-frame projection from project goal state
- active-task pins from working memory
- operator-pinned memories

### Ranking policy

Each candidate gets a composite score:

```text
retrieval_score =
  scope_weight +
  recency_weight +
  evidence_weight +
  relevance_weight +
  trust_weight -
  superseded_penalty -
  contest_penalty -
  staleness_penalty
```

Required rules:

- mission-frame projection is fixed-budget L0 context and is not ranked below
  ordinary recalled memory
- pointer-backed verbatim evidence outranks summary-only evidence
- project-local durable memory outranks cross-project memory by default
- contested or superseded items may appear in explain mode, but lose auto-inject
  priority
- records without support links cannot outrank records with support links

### Budget policy

## `RecallBudgetPolicy`

```text
RecallBudgetPolicy {
  max_records_auto_inject
  max_records_explain_only
  max_tokens_hydrated
  max_cross_project_records
  max_contested_records
}
```

First-release defaults:

- `max_records_auto_inject = 5`
- `max_records_explain_only = 20`
- `max_tokens_hydrated = 4000`
- `max_cross_project_records = 2`
- `max_contested_records = 1`

## 3. Memory Promotion And Invalidation Authority

Memory trust is role-sensitive.

### Allowed writers

- session summarizer may create `candidate` memory only
- branch winners may propose memory
- reviewers may mark evidence gaps or contest prior memory
- director or promotion policy may promote to durable trusted memory
- cleanup or rollback lanes may invalidate memory

### Promotion states

- `candidate`
- `supported`
- `trusted`
- `contested`
- `superseded`
- `invalidated`
- `expired`

### Promotion rules

- candidate -> supported requires at least one source artifact or transcript
  span
- supported -> trusted requires promotion authority and zero unresolved critical
  contest
- contested items are never auto-injected unless explicitly requested
- superseded or invalidated items are explain-visible only

### Invalidation authority

Allowed invalidators:

- rollback engine after restore
- cleanup engine when canonical artifact pointer changes
- review outcome that proves a memory is wrong
- operator explicit invalidation

No summarizer is allowed to invalidate durable memory on its own.

## 4. Working Memory Policy

Working memory is bounded and session-local by default.

Required policy:

- append only during a turn
- prune only at turn end or compaction points
- active task pins survive interruption
- unpinned scratch notes are evictable by recency and relevance

Eviction preference:

1. unpinned scratch
2. stale tool chatter
3. already-captured summaries
4. low-confidence inferred notes

## 4.5 Agent Runtime Policy

Multi-agent execution must stay operator-bounded rather than becoming hidden
background fanout.

## `TaskPacket` Policy View

```text
TaskPacket {
  runtime_version
  migration_version
  schema_version
  conformance_line
  canonical_path
  retention_policy
  atomic_write_policy
  mission_frame_ref?
  goal_alignment_policy
  task_packet_id
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

This section is informative policy projection only.

The canonical persisted schema owner is:

- `schemas/task_packet.schema.json`

This policy view must stay field-compatible with that schema and the universal
envelope rule in `14-executable-runtime-protocol.md`.

Frozen policy:

- every agent launch materializes one persisted task packet
- task packet persistence path is `.pmcli/agents/<agent_id>/TASK_PACKET.json`
- `allowed_paths` and `forbidden_paths` are authoritative for autonomous writes
- `change_envelope_ref` is required for mutating research or repo-governance stages
- `rollback_snapshot_required = true` for any stage that can mutate canonical
  artifacts, docs, or experiment configs
- a packet that omits scope, budget, or success criteria is invalid

## `AgentRuntimePolicy`

```text
AgentRuntimePolicy {
  max_live_agents_per_session
  max_live_reviewers_per_batch
  stop_grace_seconds
  require_task_packet
  reviewer_blinding_required
}
```

First-release defaults:

- `max_live_agents_per_session = 6`
- `max_live_reviewers_per_batch = 2`
- `stop_grace_seconds = 15`
- `require_task_packet = true`
- `reviewer_blinding_required = true`

Required rules:

- every agent launch carries the current mission-frame projection when a
  `MissionFrame` exists
- every agent launch carries an explicit task packet with scope, objective, and
  output expectations
- reviewer agents run on blinded evidence when reviewer policy requires
- operator-visible stop requests classify result as graceful stop, forced stop,
  or stop refused by policy

## 5. Branch Scheduler Contract

Branch search must feel like controlled search, not swarm chaos.

## `BranchSchedulerPolicy`

```text
BranchSchedulerPolicy {
  max_parallel_branches
  max_total_branches
  max_retries_per_branch
  evaluation_required
  review_required_for_promotion
  winner_count
}
```

First-release defaults:

- `max_parallel_branches = 3`
- `max_total_branches = 8`
- `max_retries_per_branch = 1`
- `evaluation_required = true`
- `review_required_for_promotion = true`
- `winner_count = 1`

### Scheduler semantics

- branches enter FIFO order within one priority band
- branches with explicit hypothesis diversity tags should be scheduled before
  near-duplicates
- one branch retry is allowed only for infrastructure or flaky-tool failure
- semantic failure or poor score does not earn automatic retry

### Quota allocator

Per batch, the scheduler must reserve:

- token budget
- tool budget
- workspace slots
- review slots
- remote visibility slots

If any quota is unavailable, the branch remains queued.

## 6. Evaluation Packet Contract

Every branch result must compile into one evaluation packet.

## `EvaluationPacket`

```text
EvaluationPacket {
  batch_id
  branch_id
  hypothesis
  diff_summary
  artifact_refs[]
  metric_results[]
  reviewer_inputs[]
  failure_flags[]
  budget_used
  final_score
  score_breakdown[]
}
```

Rules:

- no branch may become promotable without an `EvaluationPacket`
- packet must separate measured metrics from evaluator commentary
- missing required metrics marks the packet incomplete, not successful

## 7. Debate And Promotion Policy

Debate is a review-backed comparison mode over branch evidence.

Required policy:

- debate packets compare branch claims, not raw prose only
- a winner may be selected only after evaluation packets exist for all compared
  branches or the skipped branches are explicitly marked incomplete
- disagreement must be recorded as `accepted`, `rejected`, or `contested`

Promotion authority:

- director selects promotion target
- reviewer may veto promotion
- executor cannot self-promote to canonical

## 7.5 Review Runtime Policy

## `ReviewRuntimePolicy`

```text
ReviewRuntimePolicy {
  fresh_thread_default
  max_retries_per_review
  allow_compare_mode
  require_trace_for_terminal_verdict
}
```

First-release defaults:

- `fresh_thread_default = true`
- `max_retries_per_review = 1`
- `allow_compare_mode = true`
- `require_trace_for_terminal_verdict = true`

Required rules:

- retry is allowed only for timeout, transport failure, or explicit rerun policy
- a passed or failed review without trace is invalid
- compare mode is opt-in and must cite what prior review or evidence set is
  being compared

## 8. Research Skill Execution Contract

Research skills compile into stage graphs, not ad hoc scripts.

## `ResearchStageExecution`

```text
ResearchStageExecution {
  run_id
  stage_id
  stage_class
  skill_id
  input_artifacts[]
  output_artifacts[]
  assigned_agent_role
  task_packet_ref
  change_envelope_ref?
  permission_mode
  tool_budget
  token_budget
  rollback_snapshot_ref?
  retry_policy
  success_criteria[]
  repair_edges[]
}
```

### Required stage classes

- `survey`
- `idea_form`
- `idea_refine`
- `plan`
- `document`
- `implement`
- `experiment_design`
- `experiment_run`
- `result_to_claim`
- `repair`
- `publish`

Rules:

- `stage_id` must resolve through `StageExecutionMap` in
  `15-research-review-and-contracts.md`
- automatic stage routing keys on `stage_class`, not on free-form stage names

## 8.1 General Skill Output Publication Runtime

M10 implements the research-runtime hot path for governed `supersede`
operations. The most general post-M10 design is stricter: every skill output
that may influence public docs, code, experiments, paper artifacts, or
prompt/resume context must enter through one `SkillOutputEnvelope`.

```text
SkillOutputEnvelope {
  schema_version
  envelope_id
  skill_id
  skill_version
  thread_id?
  stage_execution_id?
  operation?
  output_kind
  visibility
  artifact_refs[]
  doc_frame_candidate?
  canonicality_policy
  human_gate_required
  review_packet_ref?
  change_envelope_ref?
  supersedes[]
  created_at
}
```

Required policies:

- skills write private candidates and envelopes, not public-latest docs
  directly
- public publication requires schema validation, path-scope validation,
  stage-compatibility validation, and DocFrame validation
- the runtime owns the transition from private candidate to review candidate
  to public-latest
- only one artifact per governed family can be public-latest
- private candidates remain addressable but are not injected into
  prompt/resume/compact unless explicitly selected by the operator
- superseded outputs remain auditable and must preserve their evidence refs
- public documents, code patches, experiment claims, and paper sections require
  a human gate unless the skill contract proves a narrower safe autopublish
  policy

This separates skill execution from publication. A skill may be creative,
domain-specific, or LLM-backed, but publication is a uniform project-management
operation with one canonical public surface.

## 8.5 Skill Registry Availability Policy

To absorb Crush-style skill productization without weakening runtime rigor, the
research layer needs a policy for when a discovered skill is actually runnable.

## `SkillAvailabilityRecord`

```text
SkillAvailabilityRecord {
  skill_id
  manifest_version
  install_origin
  enabled
  disabled_reason?
  dependency_status[]
  stage_compatibility[]
  validation_status
}
```

Rules:

- only `enabled` and schema-valid skills may be selected automatically
- degraded skills may remain inspectable but cannot be auto-routed into a stage
  that requires missing dependencies
- stage routing must consider `stage_compatibility`, not only skill name
- operator-disabled skills stay visible in registry output with explicit reason

### Stage rules

- every stage declares required input artifact families
- every stage emits at least one output artifact or an explicit null result
- implement and experiment stages are never allowed to write undocumented files
- mutating stages must either embed or reference a `ChangeEnvelope`
- mutating stages must publish allowed paths, permission mode, tool budget, and
  token budget before tool execution begins
- rollback binding is mandatory before any stage mutates canonical docs, code,
  experiment plans, or promoted result artifacts
- result-to-claim is a gate, not a passive report
- repair edges must name which artifact families are eligible to change

## 9. Retry And Repair Policy

Retries must be policy-bounded.

### Allowed automatic retries

- transient provider failure
- transient MCP or remote transport failure
- flaky test or experiment infra failure when flake evidence exists

### Disallowed automatic retries

- low-quality research judgment
- weak evaluation score
- failed review due to evidence gap

Those cases must route through repair:

- update docs
- update code
- update experiment plan
- rerun bounded validation

## 10. Research Runtime Roles

Role authority must stay crisp.

### `planner`

- decomposes problem
- proposes stage graph and budgets
- cannot promote outputs to canonical alone

### `executor`

- writes code, docs, or experiment assets within assigned scope
- cannot approve own work

### `reviewer`

- sees blinded evidence packet when policy requires
- may approve, reject, or request revision

### `publisher`

- packages approved outputs
- updates canonical pointers

### `supervisor`

- manages queued runs, leases, wake events, and remote notifications

## 11. Remote Visibility Policy For Advanced Layers

Remote/mobile/web surfaces may inspect:

- branch queue state
- evaluation packet summaries
- memory explain results
- project-ops cleanup proposals

Remote/mobile/web surfaces may act on:

- permission responses
- takeover requests
- wake queue actions
- review acknowledgments

Remote/mobile/web surfaces may not:

- promote branch winners directly
- invalidate durable memory without kernel approval
- bypass review gates

## 12. Required Advanced Fixtures

At minimum:

- `memory_pointer_backed_record_outranks_summary_only`
- `memory_contested_record_explain_visible_but_not_auto_injected`
- `memory_rollback_invalidates_trusted_record`
- `branch_retry_only_for_infra_failure`
- `branch_single_winner_after_review`
- `evaluation_packet_incomplete_blocks_promotion`
- `research_result_to_claim_forces_repair_edge`
- `reviewer_veto_blocks_executor_self_promotion`

## 13. Why This Now Becomes Implementable

The design no longer says only "we have memory, branches, debate, and a
research skill pack."

It now freezes:

- retrieval ranking and budgets
- who may promote or invalidate memory
- branch scheduling, quotas, retries, and evaluation packets
- research stage authority, repair edges, and gating

That is the runtime-policy layer the previous review said was still missing.
