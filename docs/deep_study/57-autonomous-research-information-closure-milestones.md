# Autonomous Research Information Closure Milestones

Date: 2026-06-11

This milestone map is specific to the information-closure PRD. It sits on top
of the broader deep-study roadmap and answers a narrower question:

> when can we say that the research loop carries information through the system
> without loss of authority, evidence, or review binding?

The answer is not "when the prompt looks good". The answer is "when each stage
is represented by durable state, explicit review bindings, and reproducible
adoption records".

## Milestone Table

| Milestone | Goal | Main code anchors | Exit proof | Current status |
| --- | --- | --- | --- | --- |
| C0 | Freeze authority boundaries | `src/session/role_souls/*`, `src/session/role_package.rs`, `src/tools/mod.rs` | Main agent has the control surface; workers do not | Implemented structurally |
| C1 | Make stage tasks explicit | `src/goals/mod.rs`, `src/agents/mod.rs` | Board tasks carry stage, task, evidence, review, blocker, and replacement fields | Implemented structurally |
| C2 | Close the accepted-evidence loop | `src/runtime/mod.rs` | Worker evidence is indexed, reviewed, and bound to the current evidence set | Implemented structurally |
| C3 | Close canonical adoption | `src/runtime/mod.rs`, `src/canonical_artifacts.rs` | Main-agent adoption materializes canonical artifacts and retires stale ones explicitly | Implemented structurally |
| C4 | Close resume, recovery, and cleanup | `src/runtime/mod.rs`, `src/branches/mod.rs`, `src/research/mod.rs` | Continuity packets, obligations, provider faults, and cleanup refs survive restart | Implemented structurally |
| C5 | Prove unattended long-run execution | runtime loop plus integration harness | A real multi-hour run completes without human intervention | Still pending live proof |

## C0. Authority Boundaries

This milestone is about making the architecture honest.

Deliverables:

- role soul separation for main agent, reviewer, and worker
- role package resolution for skills and tool policy
- main-agent-only control tools
- worker-only execution tools

Exit proof:

- a worker cannot publish board tasks
- a worker cannot request cleanup or route change
- the main agent cannot use worker-only tools
- the role soul is injected as context, not as a hidden side channel

## C1. Stage Task Contract

This milestone is about making task intent durable.

Deliverables:

- `TaskPacket`
- `AgentStageTaskContract`
- `GoalStageTaskMetadata`
- board task publication and update paths
- explicit required canonical artifact dependencies

Exit proof:

- every task has an objective, required outputs, acceptance checks, failure
  signals, and dependency refs
- task contracts can carry review targets, blocker refs, and replacement refs
- the contract is rich enough to survive resume without reconstructing meaning

## C2. Accepted Evidence And Review Binding

This milestone is about making worker output replayable and reviewable.

Deliverables:

- accepted-worker-evidence index
- semantic review binding
- current evidence set ids
- supersede and replacement tracking
- failure-to-obligation projection

Exit proof:

- worker output is not just saved; it is indexed
- semantic review attaches to a concrete task and evidence set
- failed review becomes a blocking obligation
- review failure cannot disappear without a main-agent decision

## C3. Canonical Adoption And Replacement

This milestone is about making evidence become canonical only through explicit
main-agent choice.

Deliverables:

- stage artifact adoption records
- project-file adoption records
- directory candidate manifests
- canonical artifact ledger
- baseline overlay promotion
- explicit replacement_of_artifact_ids

Exit proof:

- adopting a replacement without naming the old artifact ids is blocked
- directory candidates require explicit manifests
- unsafe nested paths are rejected
- failed materialization does not leave a partial canonical target
- retired artifacts stop satisfying future dependencies

## C4. Continuity, Resume, Cleanup, And Recovery

This milestone is about preserving project memory across restarts and provider
changes.

Deliverables:

- `AutonomousResearchJobState`
- `AutonomousResearchContinuityPacket`
- `AutonomousResearchProjectContinuitySnapshot`
- `AutonomousResearchStageClosureLedger`
- provider fault state and backoff
- cleanup plan references

Exit proof:

- a new session can resume from persisted project state
- the current stage, open obligations, and active evidence set are visible
- provider handoff and model handoff are recorded explicitly
- cleanup requirements are projected instead of inferred

## C5. Unattended Long-Run Proof

This is the only milestone that is still proof-complete rather than
structurally complete.

Deliverables:

- a real full-auto run in an empty working directory
- at least one complete non-trivial research cycle
- provider recovery under real faults
- review-fail-repair loops that remain under main-agent control
- cleanup after route changes or stale evidence replacement

Exit proof:

- the run continues unattended for hours or longer
- the main agent, not runtime, makes the research decisions
- the system can recover from provider faults without converting them into
  hidden fallback research
- the loop produces a research artifact that survives review and resume

## Suggested Order Of Execution

If this closure layer is being extended again, the next work should follow this
order:

1. tighten task contract fields when a missing dependency appears
2. repair evidence binding before adding new worker types
3. extend canonical adoption only after the evidence chain is explicit
4. harden continuity and cleanup once the adoption path is stable
5. run the unattended long proof last, not first

## What Not To Reorder

Do not move the following earlier than they belong:

- do not let runtime author research tasks
- do not let workers change the research route
- do not let review failure bypass the obligation layer
- do not let canonical adoption happen without explicit replacement ids when a
  target already has active canonical content
- do not declare the closure layer complete from smoke tests alone
