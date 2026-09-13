# Autonomous Research Information Closure Implementation

Date: 2026-06-11

This document maps the 2026-05-27 PRD onto the Rust implementation that now
exists in this repository. It is intentionally implementation-first: it names
the real modules, the real persisted objects, and the real control flow that
already enforce the information-closure boundary.

The core design rule is unchanged:

- the main agent decides
- runtime persists, validates, projects, and blocks
- agent-team workers execute assigned TaskPackets only
- reviewer output must bind to concrete evidence
- canonical artifacts change only through explicit adoption records

## 1. What This Closure Layer Actually Does

The implemented loop is not a second research system. It is a communication
and authority layer that makes the existing research pipeline auditable.

The loop now has a concrete chain:

1. user intent becomes a mission frame and an autonomous research job
2. the main agent receives a continuity packet and the active stage contract
3. the main agent publishes board tasks with explicit evidence and review
   requirements
4. workers run in scoped role packages with assigned skills and tools
5. worker outputs are accepted into a stage-local accepted-evidence index
6. the main agent explicitly adopts selected evidence into canonical artifacts
7. canonical artifacts are recorded in the ledger and promoted to the current
   baseline only when the status chain allows it
8. review failures become blocking obligations that must be routed back to the
   main agent
9. continuity packets and stage closure ledgers preserve the current state for
   resume
10. cleanup and replacement retire stale artifacts out of the active baseline

## 2. Real Code Surfaces

The closure behavior is split across these modules:

- `src/session/context_pack.rs`
- `src/session/role_package.rs`
- `src/session/role_souls/*.md`
- `src/session/role_packages/*`
- `src/tools/mod.rs`
- `src/agents/mod.rs`
- `src/goals/mod.rs`
- `src/canonical_artifacts.rs`
- `src/runtime/mod.rs`

## 3. Main-Agent Authority Is Explicit

The main agent context is assembled from:

- the role soul text in `src/session/role_souls/main_agent.md`
- the stage/objective context in `src/runtime/mod.rs`
- the project mission frame and task pool in `src/goals/mod.rs`
- the scoped tool surface in `src/tools/mod.rs`

The main agent tool surface is intentionally narrow:

- read-only inspection tools are always available
- board and adoption tools are only available when the permission mode is not
  read-only
- worker-only tools are excluded from the main-agent surface

The exact main-agent control tools are:

- `publish_board_tasks`
- `update_board_task`
- `merge_board_tasks`
- `record_obligation_decision`
- `record_worker_artifact_decision`
- `adopt_stage_artifact`
- `record_canonical_artifact_integration_check`
- `request_review_rerun`
- `request_route_change`
- `request_cleanup_plan`

## 4. Stage Contracts Are Now First-Class

The stage contract is the key implementation bridge between the PRD and the
runtime.

`GoalStageTaskMetadata` in `src/goals/mod.rs` and `AgentStageTaskContract` in
`src/agents/mod.rs` now carry the same closure-critical fields:

- `input_artifact_refs`
- `required_canonical_artifacts`
- `required_output_artifact_type`
- `required_output_fields`
- `acceptance_checks`
- `failure_signals`
- `depends_on_task_ids`
- `review_findings_refs`
- `blocker_refs`
- `review_target_task_ids`
- `review_target_evidence_refs`
- `supersedes_task_ids`
- `replacement_of_task_ids`
- `current_evidence_set_id`
- `consumed_input_required`

This is the part that makes stage tasks precise enough to be replayed, reviewed,
and repaired instead of being vague prompts.

## 5. Stage Plans And Rubrics Are Materialized

`src/goals/mod.rs` now synthesizes the stage plan and the stage acceptance
rubric as DocFrame-bearing artifacts.

The runtime writes:

- the stage plan
- the stage acceptance rubric
- the stage artifact itself
- the accepted worker evidence section
- the paper source bundle manifest for the paper-write stage

The stage plan records:

- delegated worker task families
- required evidence before review
- review-readiness criteria
- rollback / pivot / fork / cleanup triggers
- the main-agent obligation to synthesize the active stage artifact from
  accepted evidence

The rubric records:

- non-negotiable bottom-line rules
- expert-level acceptance targets
- required fields
- pass criteria
- failure signals
- stage-local worker responsibilities

## 6. Worker Execution Uses Scoped Role Packages

Workers do not receive the main-agent control surface.

Instead, they are built from:

- a shared `TaskPacket`
- a role profile
- role-specific skill refs
- role-specific tool policy
- a worker-only tool set

Relevant worker authority lives in:

- `src/session/role_souls/*.md`
- `src/session/role_packages/*`
- `src/agents/mod.rs`

The important point is architectural:

- role souls define the worker identity and obligations
- role packages define the concrete skills and allowed tools
- the runtime injects the package into the prompt/context
- the worker still only executes the assigned TaskPacket

## 7. Accepted Evidence Is Now Indexed, Not Just Appended

The accepted-evidence layer is the main closure mechanism.

`src/runtime/mod.rs` now records a stage-local accepted-evidence index that
tracks:

- `agent_id`
- `task_id`
- `task_type`
- `worker_role`
- `required_output_artifact_type`
- `output_manifest_ref`
- `task_packet_ref`
- `evidence_refs`
- `matched_required_fields`
- `matched_acceptance_checks`
- `matched_quality_signals`
- `quality_profile`
- `semantic_review`
- `main_agent_acceptance`
- `acceptance_authority`
- `main_agent_decision_ref`
- `review_required`
- `active_status`
- `current_evidence_set_id`
- `superseded_by_task_id`
- `replacement_of_task_ids`
- `decision_reason`

That is what makes the evidence chain replayable.

The index is not just an audit log. It is also the mechanism used to:

- identify the current active evidence set
- carry semantic reviews forward
- supersede stale task outputs
- bind main-agent acceptance back into the same evidence chain
- decide whether later stages may consume the evidence

## 8. Canonical Artifacts Have A Real Lifecycle

`src/canonical_artifacts.rs` now manages the canonical artifact ledger and the
status transitions for real project files.

The ledger tracks a full lifecycle:

- `candidate_only`
- `accepted_evidence`
- `adoption_requested`
- `materialization_blocked`
- `materialized`
- `baseline_promotion_blocked`
- `baseline_visible`
- `integration_failed`
- `integration_verified`
- `active_stage_evidence`
- `retired_or_superseded`

The ledger entries carry the fields needed to prove the closure path:

- source agent and task
- target artifact path
- source and target checksums
- materialization timestamp
- baseline reference
- integration checks
- retirement and cleanup refs

The implementation also enforces explicit replacement:

- if a target already has an active canonical artifact, the main agent must
  name `replacement_of_artifact_ids`
- runtime refuses silent overwrite
- the old artifact is retired only after the replacement materializes

Directory candidates are also handled explicitly:

- a directory candidate must include a directory manifest
- nested paths must be safe and stay under the declared root
- failed copy cannot leave a partial canonical target

## 9. Review Failures Become Obligations

`src/runtime/mod.rs` turns review failures into structured obligations instead
of letting them disappear into prose.

The job state keeps:

- open obligations
- acknowledged obligations
- strategy-decided obligations
- converted board-task obligations
- satisfied obligations
- cleanup-related obligations

The closure ledger then projects:

- missing task types
- adoption-ready candidates
- latest failed review repair state
- canonical artifact blockers
- review rerun blockers
- next-stage action constraints

This is the piece that prevents the runtime from silently inventing the next
research action.

## 10. Resume Uses Persisted Continuity, Not LLM Memory

The resume path now centers on a persisted continuity packet:

- `AutonomousResearchJobState`
- `AutonomousResearchProjectContinuitySnapshot`
- `AutonomousResearchContinuityPacket`
- `AutonomousResearchStageClosureLedger`

The packet preserves:

- the active job and stage
- the current provider and model
- the previous provider and model
- open blocking obligations
- accepted evidence counts and task ids
- stage closure state
- cleanup plan ids
- current artifacts and warnings
- the next required action

That means model swaps and session reentry do not depend on the previous prompt
buffer. They depend on durable project state.

## 11. Current Implementation Status

Implemented in code:

- role-based main-agent / worker separation
- explicit stage contracts and board tasks
- accepted worker evidence indexing
- semantic-review binding
- main-agent adoption records
- canonical artifact lifecycle and replacement
- stage plan and stage rubric DocFrames
- continuity packet and resume projection
- provider-fault and obligation routing
- cleanup and retirement semantics

Still needing live, long-run proof:

- unattended full-auto runs over hours or longer
- real-provider recovery under repeated fault conditions
- empty-directory startup and resume across multiple stages
- end-to-end proof that no runtime research fallback is needed in the live loop

## 12. Source-Level Regression Anchors

The current tree already contains regression tests covering the hardest edge
cases, including:

- explicit replacement of canonical artifacts
- project-file replacement requiring explicit old artifact ids
- directory candidate safety and manifest enforcement
- no partial copy on directory materialization failure
- accepted-evidence projection into the canonical stage path
- retirement pruning from the current baseline overlay

The important point is not the test names themselves. It is that the code now
has a structural home for the information closure boundary instead of relying
on prompt discipline alone.
