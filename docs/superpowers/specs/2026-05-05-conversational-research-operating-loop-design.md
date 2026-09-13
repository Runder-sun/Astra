# Conversational Research Operating Loop Design

Date: 2026-05-05

## Purpose

This document defines how semi-automatic and fully automatic research behavior
lives inside the normal `research-cli` conversation runtime.

The important correction is that the user must always stay in one continuous
conversation with the agent. Automation is not a separate mode or a bolt-on
layer. It is a policy and execution strategy that can attach to a conversation,
a research thread, a stage, or a task, while chat remains the primary
interface.

Trellis is not a default system component. It is a skill/workflow that can
produce useful planning artifacts, but the product architecture must stay
native to `research-cli`:

- one runtime and session lane
- one project goal authority
- one research-thread state model
- one review/task/projectops governance model
- local TUI, web, and mobile as projections over kernel-owned truth

The feature should therefore not be designed as "add Trellis Kanban to the
product." It should be designed as conversation-native research orchestration
that turns existing research state into executable, inspectable, gated work.

## Current Default Runtime

The current default runtime is not a research dashboard and not a mode switch.

When a user starts the product without a command, `Runtime::run` opens the TUI
launch path unless `--json` or `--continue` changes the lane. Explicit commands
dispatch through the same runtime object. The primary user lane is:

```text
workspace resolution
  -> project registry / data dir
  -> session open or resume
  -> provider and permission preflight
  -> prompt / chat / resume / continue
  -> agent loop with tools
  -> transcript, event log, checkpoint, projections
```

The TUI default is a Claw-style inline REPL. It is chat/composer first, with
commands, permissions, structured output, and compact research context folded
into the conversation. Fullscreen/projection rendering exists, but it is not
the default product shape.

Research is currently an advanced command domain and a context projection, not
the outermost runtime. `TurnResult` can carry:

- `mission_frame`
- `doc_context`
- `research_context`
- `skill_outputs`
- `research_classification`

That means research context already rides inside ordinary prompt/resume/continue
turns. The design must preserve that: research state, automation policy, and
next actions belong inside the conversation lane, not beside it.

## Conversation Is The Primary Runtime

The user should always stay in one live conversation with the agent.

- The agent may propose research actions inline.
- The agent may launch background work between turns.
- The user may keep talking, interrupt, redirect, or take over at any time.
- Semi-auto and full-auto are policies on a conversation, thread, stage, or
  task, not separate product modes.

This means the UI should never force a choice between "chat mode" and
"automation mode". The conversational runtime stays primary; automation only
changes how much of the surrounding research loop can execute proactively.

## Existing State Primitives

The current system already has most of the raw material for a conversational
research loop, but these primitives are not yet unified into one pipeline UX.

### Project Goal Authority

`MissionFrame` stores the project-level priority rule:

```text
project_max_goal > milestone_goal > current_implementation_goal
```

This is the highest-level alignment anchor. The research loop may propose
MissionFrame refreshes through evidence, but it must not silently rewrite the
canonical frame.

### Research State

`ResearchThread` is the long-lived research object. It points to the active
`ResearchStageExecution` and active `DeliberationSpan`.

`ResearchStageExecution` stores:

- stage id and class
- parent/root execution relation
- operation such as `advance`, `repair`, `pivot`, `supersede`
- input and output artifacts
- decision record
- repair targets and superseded refs
- human gate
- status

`DeliberationSpan` stores:

- turn refs
- evidence refs
- open questions
- candidate and rejected options
- agreed decisions
- review findings
- pending operations
- next recommended action
- confidence

`ResearchStatusReport` exposes the active thread, active stage, active span,
pending operations, pending HITL gates, latest middleware packet, and next
recommended action.

### Research Middleware

The research middleware already models a lot of the desired semi-automatic
behavior:

- role templates with tool policy
- tool-selection traces
- stage reflection packets
- HITL pending gates
- tool-error recovery
- lazy runtime initialization

This is important: a future board should not invent its own gate model. It
should surface these pending gates and write decisions through the existing
research decision lane.

### Agent Execution

`TaskPacket` is the right unit for delegated work. It already contains:

- intent
- role profile
- retention and resume policy
- run class and IO mode
- replay seed
- budget
- path scope
- write authority
- success criteria
- output manifest requirement
- review gate requirement

The research loop should create or propose `TaskPacket`s for executable work.
It should not launch vague "agent cards" with only a title.

### Existing Main-Agent Orchestration Files

The main agent already writes orchestration files. This is not a minor output
detail; it is one of the existing ways the conversation becomes an inspectable
work protocol for the downstream agent team.

The current implementation already has a real orchestration-file substrate:

- `OrchestrationRun` stores run objective, step status, progress, worker,
  gate, artifact, continuation, and control-command fields.
- `orchestration.json` is the typed persisted run record.
- `orchestration.md` is the human-readable checklist projection.
- `active_run` points the runtime back to the current run.
- markdown reconciliation can read checklist edits back into run state.
- context assembly injects active run progress into the main agent's prompt.
- host projection exposes active run progress to TUI, web, and mobile surfaces.
- `TaskPacket` is the schema-owned execution contract for delegated work.
- local agent runs write per-agent `DIRECTIVE.md` and `STATUS.md`.
- `AgentRuntimeRecord` links back to `task_packet.json`, `DIRECTIVE.md`,
  `STATUS.md`, workspace binding, traces, and output manifest.

So the issue is not that the system lacks orchestration files. The issue is
that the existing orchestration run is still mostly a generic run/checklist
artifact, while the research workflow needs it to carry or link richer research
semantics. Today, per-agent directive/status files explain what one worker did,
and `OrchestrationRun` explains run progress. They are not yet fully reconciled
into one research-team orchestration loop that binds:

- conversation goal and MissionFrame
- active ResearchThread / ResearchStageExecution
- ordered or parallel work packages
- agent-team roles and ownership
- TaskPacket refs
- review and claim gates
- ProjectOps leases and wakes
- promotion / repair / pivot decisions

That means the main agent's orchestration is already real, but the system should
extend the existing orchestration substrate instead of inventing a second one.
The research loop should make `OrchestrationRun` / `orchestration.md` more
research-aware and reconcile it with per-agent `DIRECTIVE.md` / `STATUS.md`,
not replace it with a separate board database.

### Review and Promotion

`ReviewPacket` and `ReviewTrace` provide the independent review layer. They
encode:

- target paths
- objective
- reviewer role and model
- fresh thread requirement
- blinding/redaction policy
- evidence requirements
- output schema
- trace retention

Research work that claims novelty, result support, or canonical promotion
should pass through this review layer instead of being marked done by a board
drag.

### Background Supervision

`ProjectOps` already has experiment supervisor leases and wake events:

- `ExperimentSupervisorLease`
- `WakeEvent`
- active/stale/pending/escalated status

This is adjacent to Hermes-style background automation, but it is not the same
as a full routine system. It can supervise known runs and escalate wakes. It
does not yet provide first-class schedule/webhook/API-triggered research runs.

### Host Surfaces

`HostSurfaceProjection` already includes sessions, permissions, memory,
branches, reviews, research summary, remote state, workbench projection, and
control lease status.

This makes it the correct projection substrate for TUI, web, and mobile. The
research loop should extend this projection with research pipeline summaries
rather than creating surface-specific truth in the mobile app or web client.

## Product Interpretation

The system should become a conversation-native research loop by turning the
current default lane into a research operating loop:

```text
conversation goal
  -> MissionFrame alignment
  -> ResearchThread selection or creation
  -> stage and span projection
  -> existing OrchestrationRun / orchestration.md update
  -> work proposal from the orchestration run
  -> gated TaskPacket execution by the agent team
  -> per-agent DIRECTIVE.md / STATUS.md reconciliation
  -> evidence and artifact writeback
  -> ReviewPacket / result-to-claim gate
  -> repair, pivot, supersede, or promote
  -> compact memory and next action
```

The workflow is the conversation-aware research state machine and its governed transitions. Any rendered view is only one projection of that workflow.

## Research Board Projection

The product can call this a research board. The important boundary is that the
board is a projection and action surface inside the same conversation runtime,
not a separate automation console or state authority.

Different surfaces may render the same projection as a compact brief, inbox,
drawer, or board. Mobile should reuse the existing mobile research-board shell
where it fits. The displayed entries must still be derived from kernel-owned
state. Board interactions may request actions, but they must write through the
existing authorities: orchestration commands, research decisions, `TaskPacket`
launches, review requests, ProjectOps wake/lease actions, and permission gates.

Its buckets should be research-native:

- `Questions`
- `Hypotheses`
- `Evidence Needed`
- `Ready To Run`
- `Running`
- `Needs Review`
- `Claim Judgment`
- `Repair / Pivot`
- `Paper Assets`
- `Blocked`
- `Promoted Memory`

These buckets are projections over typed objects, not free-form card states.
Manual bucket movement is never canonical truth.

Examples:

- a `Question` entry maps to a thread open question or candidate stage
- a `Ready To Run` entry maps to a validated `TaskPacket` proposal
- a `Running` entry maps to `AgentRuntimeRecord` or ProjectOps lease state
- a `Needs Review` entry maps to `ReviewPacket`
- a `Claim Judgment` entry maps to result-to-claim evidence and decision state
- a `Repair / Pivot` entry maps to `ResearchDecisionRecord.operation`
- a `Promoted Memory` entry maps to durable memory or DocFrame publication refs

## Semi-Automatic Policy

Semi-automatic policy should be the default for ordinary research threads.
It is not a separate user-facing mode. The user stays in chat; the policy only
changes which proactive actions the agent may take between turns.

The system may automatically:

- classify user turns into research relevance
- propose or select a research thread
- propose stage transitions
- propose TaskPackets
- prefetch literature or local evidence
- run read-only inspections
- prepare review packets
- summarize current evidence and gaps
- draft next actions
- stage repair/rerun plans

The user should approve:

- creating a new research thread when ambiguous
- changing MissionFrame priority
- heavy compute or external API spend
- mutation-bound code/doc/experiment actions
- branch promotion
- canonical artifact supersede
- paper/public output promotion
- full-auto escalation policy

This policy matches the current product shape: chat remains primary; the
research brief and board show what the system thinks should happen next.
The user can still discuss, redirect, or pause at any point.
This policy is a resolver over the existing permission preflight,
`TaskPacket.write_authority`, research HITL gates, and review gates. It does not
introduce a second ACL or task permission model.

## Fully Automatic Policy

Fully automatic policy should be opt-in and bounded. Product surfaces may
present this as an automation mode switch, but implementation should treat it
as a policy attached to the current conversation, research thread, stage, or
task. Switching the policy must not move the user into a second runtime.

It should require an explicit automation policy:

```text
AutomationPolicy {
  mode: full_auto
  scope: project | thread | stage
  budget: time / tool calls / cost / compute
  permission_ceiling
  allowed_stage_operations
  allowed_artifact_families
  review_required
  stop_conditions
  delivery_targets
}
```

The system may then, within that policy:

- decompose thread goals into stage tasks
- dispatch multiple TaskPackets
- monitor ProjectOps leases
- retry known recoverable failures
- request independent reviews
- execute repair/rerun loops within budget
- publish non-public internal summaries
- stop on gates, budget exhaustion, contradiction, or review failure

Even in full-auto mode, several transitions should remain hard gates unless
the user explicitly grants a narrower policy exception:

- changing `MissionFrame`
- spending heavy compute
- external publication
- destructive repo cleanup
- canonical supersede of public artifacts
- merging winning branches
- sending results to external recipients

## Pipeline Fit

The current architecture already describes R1-R9 research stages. The loop
should map them to current primitives this way:

| Research Stage | Current Primitive | Research Orchestration Role |
|---|---|---|
| R1 problem framing | MissionFrame + ResearchThread | create/select thread and frame alignment |
| R2 literature/evidence | ResearchStageExecution + evidence refs | evidence-needed and reading tasks |
| R3 idea/debate | DeliberationSpan + DebateTrace | hypotheses and candidate options |
| R4 method/hypothesis | ResearchDecisionRecord | approve plan or request revision |
| R5 implementation/env | TaskPacket + AgentRuntimeRecord | scoped implementation tasks |
| R6 experiment run | ProjectOps lease + artifacts | running/supervised experiment cards |
| R7 result/claim | ReviewPacket + result-to-claim | claim judgment and review gates |
| R8 repair/pivot/rerun | ResearchDecisionRecord.operation | repair/pivot loop |
| R9 report/paper | DocFrame + artifacts + review | paper asset and promotion cards |

This keeps the full research lifecycle visible without making the board the
source of truth.

## Interaction Design

### TUI

The TUI should keep the inline REPL as the default screen.

The research loop should appear through:

- compact research brief in the banner/context area
- `/research` scoped palette
- inline cards for gates, review requests, running tasks, and claim judgments
- optional inspector opened by `/research board`

The TUI should not become a persistent multi-column dashboard by default. The
board inspector is useful, but the inline conversation remains the primary
operator lane.

### Web

Web can support a richer research board because it has space, but chat should
still be the primary first screen. The board should be a drawer or secondary
route that is opened from the active research brief.

Best fit:

- conversation center
- compact project/session chrome
- right-side or full-page research board when requested
- claim/evidence/review detail drawer for selected cards

### Mobile

Mobile should not show a dense kanban by default.

It should show:

- active question
- current stage
- next action
- gates awaiting approval
- running/background task status
- result/claim alerts
- simple approve/reject/replan choices

The mobile board should reuse the existing mobile research-board shell as a
focused research inbox, not a mini desktop dashboard.

## Architecture Additions

The design should introduce a thin native layer, not a new runtime:

```text
ResearchOrchestrationExtension
  schema_version
  run_id
  project_id
  session_id?
  mission_frame_ref?
  thread_id?
  stage_execution_id?
  mode
  research_stage_refs[]
  team_refs[]
  task_packet_refs[]
  review_packet_refs[]
  lease_refs[]
  claim_gate_refs[]
  last_reconciled_at?
```

```text
ResearchProjectionSnapshot
  reads:
    MissionFrame
    ResearchStatusReport
    ResearchThreadInspection
    OrchestrationRun / orchestration.md
    TaskPacket / AgentRuntimeRecord
    DIRECTIVE.md / STATUS.md
    ReviewPacket / ReviewTrace
    ProjectOpsStatusReport
    artifacts / memory / docs
  projects:
    board entries
    next actions
    HITL gates
    automation eligibility
```

The extension is the only persisted addition here. The snapshot is derived on
read from existing state and the run extension. It must not accept manual
section movement or column movement as canonical truth.

The research loop should be implemented as a projection and command layer under
the existing `research` family:

- `research status`
- `research board`
- `research propose`
- `research decide`
- `research policy`

Naming can change, but the important boundary is that this layer coordinates
existing authorities instead of duplicating them.

## Data Model Sketch

This should be an extension of the existing `OrchestrationRun` substrate, either
as new typed fields on the run or as a typed sidecar linked by `run_id`.
The board, mobile research view, and run progress should project from the
orchestration run plus the linked runtime records, rather than from free-form
state movement.

```text
ResearchAutomationPolicy {
  policy_id
  scope
  mode
  budget
  permission_ceiling
  allowed_operations[]
  blocked_operations[]
  review_required
  stop_conditions[]
  delivery_targets[]
  created_by
  status
}
```

The first implementation can derive board entries on demand from current state.
Persist only the extension, policy, and explicit operator decisions. Avoid
writing manually moved board sections or columns as canonical truth.

## What This Adds

This design adds value because it makes research work compositional:

- A vague research goal becomes a stage-linked thread instead of a chat blob.
- Tasks become `TaskPacket`s with scope, budget, success criteria, and review
  requirements.
- Existing main-agent orchestration files become the glue between conversation,
  research stages, agent-team execution, and operator projections.
- Background experiments become supervised leases with wakes.
- Results cannot silently become claims; they pass through review/claim gates.
- Repair and pivot become typed transitions instead of ad hoc follow-up chats.
- TUI, web, and mobile see the same state through host projections.
- Semi-auto and full-auto differ by policy, not by separate code paths.

The main benefit is not "a nicer board." The benefit is that the system can
accumulate work across research stages without losing authority, provenance, or
operator control.

## Non-Goals

- Do not make Trellis a product dependency.
- Do not replace the inline REPL as the default TUI.
- Do not create a second event, permission, or task state model.
- Do not let mobile/web become hidden runtime authorities.
- Do not claim Hermes-style routines parity until schedules/webhooks/API
  triggers and delivery targets exist.
- Do not auto-promote claims, papers, branches, or public artifacts without
  explicit policy and review gates.

## Open Questions

1. Should the first shipped command be read-only, such as
   `research brief --json`, before adding `propose` and `policy` writes?
2. Should full-auto policy live under `research` or under a broader
   `automation` command family?
3. Should the host projection cache the latest snapshot for mobile latency?
4. Which stages should be eligible for full-auto by default: R2/R3/R5/R6 only,
   or the whole R1-R9 chain with hard gates?
5. Should ProjectOps-based wake/resume continuation be part of this first slice,
   or should external schedule/webhook/API triggers wait until the routines gap
   is implemented?
6. Should research metadata be added directly to `OrchestrationRun`, or should
   it live in a typed sidecar linked to the existing run id?

## Recommended First Slice

The first implementation should be conservative:

1. Add read-only `ResearchProjectionSnapshot`.
2. Derive board entries from existing `ResearchStatusReport`, threads, stage
   executions, middleware gates, agents, reviews, ProjectOps leases, and
   artifacts.
3. Expose it through CLI JSON and host projection.
4. Show a compact board/brief in TUI/web/mobile without new write authority.
5. Add `propose` only after the read-only projection proves stable.
6. Add semi-auto task proposal before full-auto execution.

This sequence respects the current product: conversation first, kernel truth
first, projection before mutation, and human gates before automation. The user
can keep talking through every step.
