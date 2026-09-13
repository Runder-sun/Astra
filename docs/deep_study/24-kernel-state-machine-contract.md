# Kernel State Machine Contract

This document turns the kernel into one executable composite state machine.

`14-executable-runtime-protocol.md` already defined the protocol line and
top-level runtime stages.

The missing piece was a single place that freezes:

- every kernel submachine
- the allowed transitions
- the cross-machine invariants
- the exact terminal outcomes conformance must test

That gap matters because operator-grade systems fail at transitions, not at the
existence of nouns.

## 1. Canonical Kernel State Bundle

Runtime truth is one `KernelStateBundle`.

```text
KernelStateBundle {
  project_state
  session_state
  turn_state
  agent_state
  review_state
  memory_state
  repo_state
  artifact_state
  projectops_state
  permission_state
  feature_state
  rollback_state
  branch_state
  remote_state
  seq_cursor
  checkpoint_epoch
}
```

Each submachine is independent in storage but not independent in authority.

Each family slot in the bundle is a canonical aggregate summary over that
family's detailed per-entity storage, not a duplicate full database.

`artifact_state` and `projectops_state` are reserved from the foundation line
even when their early contents are skeletal. That prevents later artifact
governance or proactive-project management features from becoming shadow
authorities outside the bundle.

The bundle is authoritative only when all submachine invariants hold.

Persistence mapping:

- `project_state.json` stores the serialized `KernelStateBundle` checkpoint
- `project_state` is one child payload inside that bundle
- restore/checkpoint code must never treat `ProjectState` as a separate
  top-level authority

## 2. Global Event Ordering Invariants

The entire kernel shares one canonical event envelope.

## `KernelEventEnvelope`

```text
KernelEventEnvelope {
  seq
  event_name
  phase
  terminal_outcome?
  object_kind
  object_id
  project_id
  session_id?
  timestamp
  payload
}
```

Allowed `phase` values:

- `start`
- `update`
- `terminal`

Allowed `terminal_outcome` values:

- `succeeded`
- `failed`
- `cancelled`
- `blocked`
- `stale_conflict`

The entire kernel shares these rules:

1. every mutating transition emits a `phase=start` event before side effects
2. terminality is expressed by `phase=terminal` plus `terminal_outcome`, not by
   suffix-only event naming
3. a later `seq` must never describe an earlier effective state
4. a terminal event must carry the same `object_id` as its start event
5. failed transitions may emit partial artifacts, but those artifacts must be
   marked non-canonical
6. duplicate terminal events for one logical outcome must reconcile into one
   canonical actionable truth even if audit history keeps the duplicates
7. transport failure, relay failure, or host disconnect must surface as typed
   uncertainty rather than being rewritten into synthetic success

### Canonical first-release `event_name` registry

These stems are authoritative for conformance and runtime output. Other docs
may use prose aliases, but fixtures and serialized events must use these
`event_name` values:

- `command`
- `runtime_preflight`
- `repl`
- `session_open`
- `session_resume`
- `session`
- `turn`
- `provider_resolution`
- `provider_auth`
- `provider_test`
- `provider_catalog`
- `permission`
- `permission_mode`
- `tool`
- `inspection`
- `session_compaction`
- `setup`
- `project`
- `config`
- `memory_query`
- `memory_record`
- `memory_promotion`
- `memory_invalidation`
- `projectops_tick`
- `digest_promotion`
- `digest_rejection`
- `cleanup_plan`
- `cleanup_apply`
- `experiment_supervision`
- `wake_event`
- `research_stage`
- `research_repair`
- `research_pivot`
- `research_refine`
- `agent_stop`
- `branch_cancel`
- `branch_promote`
- `review_open`
- `review_retry`
- `mcp_registry`
- `remote_pair`
- `remote_status`
- `remote_attach`
- `remote_handoff`
- `remote_takeover`
- `remote_control_owner`
- `remote_lease_revoked`
- `remote_binding_revoked`
- `remote_action_rejected`
- `remote_notify`
- `remote_terminal`

State-machine transition tables below use logical trigger labels. Those trigger
labels are not a second wire vocabulary and must not override the canonical
`event_name` registry above.

## 3. Session Lifecycle Machine

States:

- `absent`
- `opening`
- `open`
- `active`
- `compacting`
- `paused`
- `archived`
- `resume_blocked`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `absent` | `open_session` | preflight ready | create session identity | `opening` |
| `opening` | `session_opened` | persistence ok | append open event | `open` |
| `open` | `turn_begin` | no active turn | bind turn context | `active` |
| `active` | `turn_finished` | no compaction threshold | release foreground ownership | `open` |
| `active` | `turn_finished` | compaction threshold hit | schedule compaction | `compacting` |
| `compacting` | `compaction_succeeded` | lineage recorded | write summary | `paused` |
| `paused` | `resume_session` | resumable and scope-valid | restore foreground lane | `active` |
| `open` | `archive_session` | no active turn | mark archived | `archived` |
| any non-terminal | `workspace_mismatch` | detected | record rejection | `resume_blocked` |

### Session invariants

- only `open`, `active`, `paused`, `compacting` sessions may have `active=true`
- `archived` sessions are never default-resumable
- `resume_blocked` cannot transition directly to `active` without a fresh scope
  check

## 4. Turn Lifecycle Machine

States:

- `idle`
- `preflighting`
- `resolving_provider`
- `awaiting_permission`
- `running_tools`
- `streaming_model`
- `assembling_result`
- `interrupted`
- `failed`
- `succeeded`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `idle` | `turn_requested` | session active | bind input | `preflighting` |
| `preflighting` | `preflight_ok` | report ready | persist preflight | `resolving_provider` |
| `preflighting` | `preflight_blocked` | report blocked | emit failure | `failed` |
| `resolving_provider` | `provider_ok` | auth valid | freeze route | `running_tools` or `streaming_model` |
| `resolving_provider` | `provider_failed` | any | emit remediation hint | `failed` |
| `running_tools` | `permission_needed` | tool policy asks | create request | `awaiting_permission` |
| `awaiting_permission` | `permission_granted` | request live | resume tool lane | `running_tools` |
| `awaiting_permission` | `permission_denied` | request live | persist denial | `failed` |
| `running_tools` | `tool_lane_complete` | model call needed | open model stream | `streaming_model` |
| `running_tools` | `tool_lane_complete` | no model call needed | collect tool result | `assembling_result` |
| `streaming_model` | `interrupt_requested` | foreground owner valid | stop stream safely | `interrupted` |
| `streaming_model` | `model_stream_complete` | response valid | collect usage | `assembling_result` |
| `assembling_result` | `result_written` | persistence ok | finalize turn | `succeeded` |
| any non-terminal | `fatal_error` | any | persist partial state | `failed` |

### Turn invariants

- there is at most one foreground turn per session
- `awaiting_permission` freezes tool progression
- `interrupted` is resumable only when the provider/tool lane exposed a resume
  token or the kernel can safely replay from last checkpoint

## 5. Permission Request Machine

States:

- `none`
- `pending`
- `granted`
- `denied`
- `expired`
- `superseded`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `none` | `permission_requested` | policy asks | persist request | `pending` |
| `pending` | `approve_request` | requester still live | persist approval | `granted` |
| `pending` | `deny_request` | requester still live | persist denial | `denied` |
| `pending` | `turn_cancelled` | request no longer actionable | close request | `superseded` |
| `pending` | `ttl_elapsed` | no answer | mark timeout | `expired` |

### Permission invariants

- a request ID may resolve only once
- remote and local approvals go through the same machine
- expired requests cannot be revived; they must be reissued

## 6. Feature Health Machine

States:

- `ready`
- `pending`
- `degraded`
- `blocked`
- `disabled`

Tracked features:

- provider backends
- plugins
- MCP servers
- remote transport
- background project ops

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `ready` | `dependency_lost` | retry lane exists | attach degraded reason | `degraded` |
| `ready` | `policy_disabled` | explicit config | mark disabled | `disabled` |
| `pending` | `feature_ready` | checks passed | clear degraded reason | `ready` |
| `degraded` | `recovery_succeeded` | checks passed | clear degraded reason | `ready` |
| `degraded` | `hard_failure` | no safe lane | block feature | `blocked` |
| `blocked` | `repair_succeeded` | checks passed | restore feature | `ready` |

### Feature invariants

- `blocked` features cannot silently serve requests
- `degraded` features may serve only commands whose policy allows degraded mode
- every degraded or blocked feature must expose a machine-readable remediation
  hint

## 7. Rollback And Checkpoint Machine

States:

- `idle`
- `snapshotting`
- `ready_to_restore`
- `restoring`
- `restored`
- `restore_failed`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `idle` | `pre_mutation_checkpoint` | mutating action approved | write safety snapshot | `snapshotting` |
| `snapshotting` | `snapshot_complete` | durable | arm rollback lane | `ready_to_restore` |
| `ready_to_restore` | `rollback_requested` | snapshot exists | compute diff preview | `restoring` |
| `restoring` | `restore_complete` | state coherent | emit invalidation candidates | `restored` |
| `restoring` | `restore_failed` | any | preserve failed restore trace | `restore_failed` |
| `restored` | `resume_runtime` | project coherent | reopen runtime lane | `idle` |

### Rollback invariants

- no restore without a known snapshot ID
- successful rollback must emit artifact and memory invalidation candidates
- rollback never implies silent remote ownership transfer

## 8. Branch Lifecycle Machine

States:

- `unallocated`
- `allocated`
- `queued`
- `running`
- `needs_refresh`
- `awaiting_evaluation`
- `awaiting_review`
- `promotable`
- `won`
- `lost`
- `archived`
- `gc_eligible`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `unallocated` | `branch_created` | budget available | allocate workspace | `allocated` |
| `allocated` | `scheduler_enqueued` | concurrency full | queue branch | `queued` |
| `allocated` | `scheduler_started` | concurrency available | bind lease | `running` |
| `queued` | `scheduler_started` | lease granted | start execution | `running` |
| `allocated` or `queued` or `running` or `awaiting_evaluation` or `awaiting_review` or `promotable` | `base_revision_drifted` | stale-base policy triggered | mark refresh required | `needs_refresh` |
| `needs_refresh` | `refresh_completed` | base reconciled and conflicts resolved | rebind workspace/base revision | `allocated` |
| `needs_refresh` | `refresh_failed` | rebase or merge conflict persists | keep stale conflict visible | `needs_refresh` |
| `running` | `execution_complete` | artifacts written | lock outputs | `awaiting_evaluation` |
| `awaiting_evaluation` | `evaluation_complete` | packet valid | persist score | `awaiting_review` |
| `awaiting_review` | `review_passed` | promotion allowed | mark candidate | `promotable` |
| `promotable` | `winner_selected` | exclusive promotion | update family pointers | `won` |
| `promotable` | `winner_rejected` | another branch wins | archive outputs | `lost` |
| `won` | `archive_after_merge` | family stable | archive branch workspace | `archived` |
| `lost` | `gc_window_elapsed` | retention satisfied | mark gc eligible | `gc_eligible` |

### Branch invariants

- only one branch per batch may enter `won`
- `won` requires a valid evaluation packet and review outcome
- `needs_refresh` blocks promotion until refresh succeeds or the branch is
  explicitly archived
- `lost` branches may not mutate canonical artifact families

## 8.5 Agent Lifecycle Machine

States:

- `unspawned`
- `spawning`
- `ready`
- `running`
- `blocked`
- `awaiting_input`
- `completed`
- `failed`
- `stopped`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `unspawned` | `agent_spawn_requested` | budget available | allocate runtime scope | `spawning` |
| `spawning` | `agent_ready` | runtime healthy | publish agent record | `ready` |
| `ready` | `agent_task_bound` | task packet valid | start execution | `running` |
| `running` | `agent_blocked` | missing input or permission | persist blocker | `blocked` |
| `blocked` | `agent_unblocked` | blocker resolved | resume execution | `running` |
| `running` | `agent_waiting_input` | policy asks for operator response | park lane | `awaiting_input` |
| `awaiting_input` | `agent_input_received` | input accepted | continue execution | `running` |
| `running` | `agent_completed` | outputs written | persist manifest | `completed` |
| any non-terminal | `agent_failed` | any | persist failure class | `failed` |
| any non-terminal | `agent_stop_requested` | stop policy valid | persist stop intent | `stopped` |

### Agent invariants

- one agent belongs to exactly one owner session or batch scope
- reviewer agents cannot publish directly to canonical artifact families
- stopped agents may emit final trace metadata but may not resume without a new
  task binding

## 8.6 Review Lifecycle Machine

States:

- `unopened`
- `queued`
- `packet_ready`
- `running`
- `awaiting_compare`
- `passed`
- `failed`
- `cancelled`
- `archived`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `unopened` | `review_open_requested` | target refs valid | build packet | `queued` |
| `queued` | `review_packet_built` | packet complete | persist packet | `packet_ready` |
| `packet_ready` | `review_started` | reviewer runtime ready | bind fresh thread | `running` |
| `running` | `review_compare_needed` | compare policy requires | park compare state | `awaiting_compare` |
| `awaiting_compare` | `review_compare_bound` | compare refs valid | resume review | `running` |
| `running` | `review_passed` | verdict emitted | persist trace | `passed` |
| `running` | `review_failed` | verdict emitted | persist trace | `failed` |
| any non-terminal | `review_cancelled` | cancel policy valid | persist cancellation | `cancelled` |
| `passed` or `failed` or `cancelled` | `review_archived` | retention policy satisfied | archive record | `archived` |

### Review invariants

- every non-cancelled review must have a `ReviewPacket`
- every terminal review except cancelled must have a `ReviewTrace`
- compare mode must preserve explicit linkage to the prior review or evidence set

## 8.7 Memory Lifecycle Machine

States:

- `candidate`
- `supported`
- `trusted`
- `contested`
- `superseded`
- `invalidated`
- `expired`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `candidate` | `memory_evidence_bound` | support refs resolvable | persist support linkage | `supported` |
| `supported` | `memory_promotion_approved` | promotion authority valid and no unresolved critical contest | publish durable memory | `trusted` |
| `candidate` or `supported` or `trusted` | `memory_contested` | review or operator challenge opened | narrow injection eligibility | `contested` |
| `contested` | `contest_resolved` | evidence still holds and no critical contest remains | clear contest marker | `supported` |
| `supported` or `trusted` or `contested` | `superseding_memory_promoted` | newer scope-compatible record wins | link lineage | `superseded` |
| `candidate` or `supported` or `trusted` or `contested` | `memory_invalidation_requested` | rollback, cleanup, review, or operator authority valid | mark non-injectable | `invalidated` |
| `candidate` or `superseded` or `invalidated` | `retention_window_elapsed` | retention policy satisfied | archive from active recall | `expired` |

### Memory invariants

- `trusted` memory requires resolvable support refs and zero unresolved critical
  contest
- `contested`, `superseded`, and `invalidated` memory may remain
  explain-visible, but may not auto-inject into turns
- invalidation changes trust and injection eligibility, not provenance lineage

## 8.8 ProjectOps Tick And Digest Lifecycle Machine

States:

- `idle`
- `tick_queued`
- `tick_running`
- `tick_completed`
- `tick_degraded`
- `digest_candidate_ready`
- `digest_promoted`
- `digest_rejected`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `idle` | `projectops_triggered` | dedupe window allows new work | allocate tick id | `tick_queued` |
| `tick_queued` | `projectops_worker_claimed` | lease granted | start tick execution | `tick_running` |
| `tick_running` | `digest_candidate_created` | supporting evidence gathered | persist candidate lineage | `digest_candidate_ready` |
| `tick_running` | `tick_actions_finished` | no digest candidate required | persist tick summary | `tick_completed` |
| `tick_running` | `tick_degraded` | dependency or support path degraded | persist degraded reasons | `tick_degraded` |
| `digest_candidate_ready` | `digest_promotion_approved` | support and policy checks pass | promote digest into canonical progress memory | `digest_promoted` |
| `digest_candidate_ready` | `digest_rejected` | support gap, duplicate scope, or policy block found | persist rejection trace | `digest_rejected` |
| `tick_completed` or `tick_degraded` or `digest_promoted` or `digest_rejected` | `projectops_tick_closed` | audit trail persisted | release queue slot | `idle` |

### ProjectOps invariants

- every digest candidate must trace back to exactly one `ProjectOpsTick`
- promoted digests must cite supporting events, artifacts, or session spans
- rejected digests remain inspectable, but may not be silently retried inside
  the same tick

## 8.9 Experiment Supervision And Wake Machine

States:

- `lease_unclaimed`
- `lease_active`
- `lease_stale`
- `lease_reclaimed`
- `wake_queued`
- `wake_acknowledged`
- `wake_resolved`
- `wake_escalated`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `lease_unclaimed` | `supervisor_lease_claimed` | run is schedulable | bind owner agent | `lease_active` |
| `lease_active` | `wake_emitted` | anomaly, completion, or checkpoint requires attention | persist wake event | `wake_queued` |
| `wake_queued` | `wake_acknowledged` | owning agent or operator accepts responsibility | record ack actor | `wake_acknowledged` |
| `wake_acknowledged` | `wake_resolved` | remediation or follow-up completed | close wake with outcome | `wake_resolved` |
| `wake_queued` or `wake_acknowledged` | `wake_escalated` | TTL elapsed, lease stale, or main-system help required | emit escalation reason | `wake_escalated` |
| `lease_active` | `supervisor_heartbeat_missed` | stale-after threshold exceeded | freeze autonomous continuation | `lease_stale` |
| `lease_stale` | `supervisor_reclaimed` | reclaim authority valid | rebind supervisor ownership | `lease_reclaimed` |
| `lease_reclaimed` or `wake_resolved` or `wake_escalated` | `supervisor_lane_resumed` | reclaim or escalation handling complete | reopen supervision lane | `lease_active` |

### Supervision invariants

- every wake must reference a live or recently stale supervisor lease, or carry
  an explicit no-owner reason
- stale leases may not silently consume queued wakes
- escalated wakes must expose whether operator or main-system intervention is
  required before autonomous continuation

## 8.10 Research Stage Lifecycle Machine

States:

- `queued`
- `running`
- `awaiting_gate`
- `repairing`
- `pivoting`
- `succeeded`
- `failed`
- `cancelled`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `queued` | `research_stage_started` | stage map and task packet resolved | bind runtime stage | `running` |
| `running` | `research_stage_gate_required` | result-to-claim, review, or policy gate required | persist gate inputs | `awaiting_gate` |
| `running` | `research_stage_completed` | outputs persisted and no gate required | publish outputs | `succeeded` |
| `awaiting_gate` | `gate_passed` | gate verdict supports continuation | finalize outputs | `succeeded` |
| `awaiting_gate` | `repair_loop_requested` | evidence suggests a local fix is viable | bind repair inputs and envelope | `repairing` |
| `awaiting_gate` | `pivot_loop_requested` | current direction is invalidated or dominated | freeze prior path for comparison | `pivoting` |
| `repairing` | `repair_plan_bound` | repair packet valid | resume execution | `running` |
| `repairing` | `repair_exhausted` | retry budget or confidence exhausted | persist repair trace | `failed` |
| `pivoting` | `pivot_plan_adopted` | alternative path approved | enqueue successor stage | `queued` |
| any non-terminal | `research_stage_cancelled` | cancellation policy valid | persist cancellation trace | `cancelled` |
| `running` or `awaiting_gate` or `repairing` or `pivoting` | `research_stage_failed` | fatal runtime or contract error | persist failure trace | `failed` |

### Research-stage invariants

- every running stage must resolve through a `StageExecutionMap` entry and one
  `TaskPacket`
- repair and pivot transitions must preserve linkage to the originating stage,
  gate verdict, and change envelope or rollback snapshot
- a succeeded stage must emit output artifacts or an explicit null-result
  record; silent success is invalid

## 9. Remote Binding And Ownership Machine

States:

- `unpaired`
- `pairing`
- `paired`
- `binding_pending`
- `bound`
- `ownership_contended`
- `revoked`
- `stale`

### Transition table

| From | Trigger | Guard | Action | To |
|---|---|---|---|---|
| `unpaired` | `pair_requested` | host reachable | open pair flow | `pairing` |
| `pairing` | `pair_completed` | machine identity valid | store lease | `paired` |
| `paired` | `attach_requested` | eligibility ok | create session binding | `binding_pending` |
| `binding_pending` | `attach_confirmed` | relay or provider attach active | publish owner | `bound` |
| `bound` | `takeover_requested` | lease valid | compare ownership token | `ownership_contended` |
| `ownership_contended` | `takeover_committed` | token current | update owner | `bound` |
| `bound` | `lease_revoked` | any | clear binding | `revoked` |
| `bound` | `cursor_stale` | heartbeat missed | mark stale | `stale` |
| `stale` | `reconnect_succeeded` | cursor valid | restore binding | `bound` |

### Remote invariants

- exactly one foreground control owner exists per bound session
- unpaired machines cannot send mutating remote actions
- `revoked` bindings must refuse action replay

## 10. Cross-Machine Invariants

These rules connect the submachines into one operator-grade kernel.

### 10.1 Session and turn

- a turn may enter `preflighting` only if session state is `active`
- session cannot enter `archived` while turn state is non-terminal

### 10.2 Turn and permission

- turn state `awaiting_permission` requires permission state `pending`
- permission terminal states must force turn re-entry into either
  `running_tools` or `failed`

### 10.3 Feature health and commands

- a command may execute on a degraded feature only if its contract explicitly
  allows degraded mode
- blocked features must fail fast before mutation begins

### 10.4 Rollback and artifacts

- rollback `restored` requires at least one emitted invalidation or explicit
  `no_invalidation_needed`

### 10.5 Branches and remote

- remote follow is allowed for any running branch
- remote takeover for branch mutation is allowed only when branch state is
  `running` and no review gate is pending

### 10.6 Agents and reviews

- branch promotion cannot occur unless agent state for the winning executor is
  terminal and review state is `passed`
- review retry cannot start while the prior review is still `running`

### 10.7 Memory, rollback, and artifacts

- rollback restore and canonical artifact-pointer change must invalidate or
  contest dependent trusted memory before the next auto-inject cycle
- superseded or invalidated memory may not regain trusted status without a new
  promotion event

### 10.8 ProjectOps and memory

- a `ProjectOpsTick` may promote a digest only when supporting events,
  artifacts, or session spans are already durable
- every digest promotion must create or reference one promoted `MemoryRecord`
  lineage entry

### 10.9 Supervision, wake, and permissions

- wake escalation that requires operator or main-system action must surface as a
  blocker or follow-up record before the supervised run may continue
- stale supervisor leases block autonomous repair or continuation until reclaim
  or explicit override

### 10.10 Research stages, branches, and reviews

- repair and pivot transitions must cite the gate or evidence packet that
  caused them; they may not be inferred post hoc
- pivoted successors may not inherit winner or promotion status from the prior
  path without fresh evaluation and review

## 11. Canonical Terminal Outcomes

Every machine must end transitions in one of these terminal outcomes:

- succeeded
- failed
- cancelled
- blocked
- stale_conflict

Conformance must assert that no mutation path terminates without one of those
labels.

## 12. Fixture Obligations

At minimum, conformance must include:

- `session_resume_workspace_mismatch`
- `turn_permission_timeout`
- `turn_interrupt_and_resume`
- `feature_degraded_then_blocked`
- `rollback_restores_and_invalidates`
- `branch_winner_unique`
- `remote_takeover_stale_lease`
- `memory_promotion_then_invalidation`
- `projectops_digest_rejected_with_trace`
- `wake_escalates_after_stale_lease`
- `research_stage_repair_then_pivot`

These fixtures extend the protocol fixtures in `14` and the operator fixtures
in `17`.

## 13. Why This Fixes The Prior Gap

The kernel is no longer frozen only as prose structs and top-level stages.

It is now frozen as one composite runtime with:

- named submachines
- explicit transitions
- explicit invariants
- explicit terminal outcomes

That is the minimum detail needed before implementation can safely begin.
