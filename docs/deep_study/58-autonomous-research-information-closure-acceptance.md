# Autonomous Research Information Closure Acceptance

Date: 2026-06-11

This document is the acceptance gate for the information-closure PRD.

It answers one question:

> can a strict reviewer verify that the system now preserves information,
> authority, and evidence through the full research loop?

If the answer is not yes, the work is not accepted.

## 1. Acceptance Principle

The closure layer is accepted only when each of the following is true:

- the main agent is the sole research decision-maker
- runtime only persists, validates, projects, and blocks
- workers only execute assigned TaskPackets
- review results are bound to concrete evidence
- canonical artifacts change only through explicit adoption
- resume restores persisted project state, not hidden LLM memory
- cleanup and replacement remove stale content from the active line
- a real unattended long run has been demonstrated

## 2. Gate Matrix

| Gate | Pass condition | Required evidence | Failure mode |
| --- | --- | --- | --- |
| Authority | Main-agent tools and worker tools are disjoint | `src/tools/mod.rs`, role package files, role soul files | Runtime or worker can take over research decisions |
| Task contract | Board tasks carry all closure fields | `TaskPacket`, `GoalStageTaskMetadata`, published board task JSON | Task intent must be reconstructed from prose |
| Evidence intake | Worker output is indexed and review-bound | Accepted evidence index + semantic review refs | Output is accepted without a traceable evidence set |
| Adoption | Main agent chooses canonical targets explicitly | Adoption record + canonical ledger + baseline overlay | Runtime silently overwrites project files |
| Replacement | Existing active content is retired by explicit ids | `replacement_of_artifact_ids` and retirement refs | A replacement lands without naming what it replaces |
| Safety | Directory candidates are manifest-backed and safe | Directory manifest + safe-path checks | Nested path or partial copy corruption |
| Review routing | Failed review becomes a blocking obligation | Obligation records + review packet/trace | Review failure is hidden or auto-resolved by runtime |
| Resume | Continuity packet restores the current research state | Continuity packet + stage closure ledger | Resume depends on stale prompt context |
| Cleanup | Route changes retire stale canonical content | Cleanup refs + retired artifact refs | Mixed old and new direction stays active together |
| Long run | Unattended research continues for hours or longer | End-to-end job log, state, and artifacts | Smoke test is mistaken for completion |

## 3. Structural Acceptance Criteria

### 3.1 Main-Agent Authority

Pass when:

- the main agent has the board, adoption, obligation, route-change, review-rerun,
  and cleanup tools
- workers do not have those tools
- the main-agent prompt says the main agent must not do worker research

Reject when:

- runtime generates research tasks on its own
- a worker can publish or close board state
- a fallback path quietly replaces main-agent decision making

### 3.2 Task And Evidence Closure

Pass when:

- stage tasks explicitly carry input refs, evidence requirements, blocker refs,
  review targets, and replacement refs
- worker output is recorded in the accepted-evidence index
- semantic review is attached to the concrete accepted evidence
- the current evidence set is explicit

Reject when:

- accepted evidence is only a note, not an index
- review results cannot be traced back to a task and evidence set
- the system must guess which evidence is active

### 3.3 Canonical Adoption

Pass when:

- a stage artifact adoption record exists for the selected target
- the canonical artifact ledger records adoption_requested and then a successful
  materialization/baseline transition
- old artifacts are retired when a replacement is selected
- directory targets are protected by a manifest and safe-path checks

Reject when:

- a replacement lands without explicit artifact ids
- a directory candidate can partially overwrite the active tree
- stale artifacts remain active in the baseline after replacement

### 3.4 Review And Repair

Pass when:

- failed review produces a blocking obligation
- the main agent can acknowledge, convert, or route the obligation
- the next action is visible in the stage closure ledger
- repair requires real evidence, not a prose-only patch

Reject when:

- review failure disappears after a resume
- runtime invents the repair task
- the system treats acknowledgement as satisfaction

### 3.5 Resume And Cleanup

Pass when:

- the continuity packet includes the current job, stage, evidence, provider,
  and next required action
- the stage closure ledger includes task-type statuses, adoption blockers, and
  review rerun blockers
- cleanup is linked to actual replacement or route change

Reject when:

- the system resumes only because the previous model still remembers context
- stale artifacts remain in the active line after a pivot
- cleanup is a comment instead of a state transition

## 4. Current Regression Anchors

The codebase already contains regression coverage for the hardest closure
cases. The important anchors are:

- `adopt_stage_artifact_persists_replacement_of_artifact_ids`
- `project_file_replacement_requires_explicit_main_agent_artifact_ids`
- `directory_candidate_explicit_replacement_retires_old_artifact_and_updates_baseline`
- `overlay_baseline_replaces_same_target_with_latest_artifact_entry`
- `retiring_artifact_removes_it_from_current_overlay_baseline`
- `directory_candidate_copy_failure_does_not_leave_partial_target`
- `publish_board_tasks_persists_required_canonical_artifacts`
- `worker_stage_task_contract_includes_required_canonical_artifacts`
- `main_agent_prompt_projects_canonical_artifact_blocker_details`
- `baseline_promotion_failure_is_recorded_after_materialized_state`
- `retired_artifact_no_longer_satisfies_file_or_runnable_dependencies`
- `unreadable_stage_artifact_adoption_does_not_retire_worker_evidence`
- `materialization_blocked_is_recorded_when_adopted_candidate_is_missing`
- `directory_candidate_requires_explicit_manifest`
- `directory_candidate_manifest_rejects_unsafe_nested_paths`

These tests are the right kind of evidence because they exercise the real
authority boundaries and the real artifact lifecycle.

## 5. Proof That Is Still Required

This gate is not satisfied by unit tests alone.

The final proof must include:

- a real full-auto research run
- a real provider or provider failover path
- unattended runtime over hours or longer
- at least one complete stage-to-stage closure cycle
- review failure, repair, and cleanup handled by the main agent
- resume from persisted state without losing the active evidence chain

## 6. Final Acceptance Rule

Accept the closure layer only when a reviewer can inspect:

- the task contract
- the accepted evidence index
- the review packet and trace
- the adoption record
- the canonical artifact ledger
- the stage closure ledger
- the continuity packet
- the cleanup summary
- the unattended run log

and confirm that every transition is explicit, auditable, and still under main
agent authority.
