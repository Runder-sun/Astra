# Research Review And Workflow Contracts

This document hardens the research side of the system into machine-readable
contracts.

It is the missing layer between the research workflow idea and a real
multi-agent engine.

## 1. Scope

This spec defines:

- reviewer independence
- review packet and trace formats
- branch evidence and adjudication
- research skill contracts
- workflow DAG and artifact contracts
- citation and recovery rules

## 2. Reference-Derived Rules

The design borrows the strongest concrete patterns from the reference repos:

- `ARIS` for reviewer independence, output versioning, and review tracing
- `GPT Researcher` and `Autonomous Researcher` for planner/executor/publisher flow
- `mempalace` for pointer-first retrieval before verbatim hydration
- `Memory-Palace` for governance, rollback, and consolidation discipline

## 3. `ReviewPacket`

Every review must be driven by a machine-readable packet.

```text
ReviewPacket {
  schema_version
  conformance_line
  canonical_path
  retention_policy
  atomic_write_policy
  review_id
  target_paths[]
  objective
  reviewer_role
  review_model
  fresh_thread
  blinded
  blinding_policy
  blind_context[]
  banned_context[]
  redaction {
    schema_version
    policy
    allowed_count
    redacted_count
    redacted_sources[]
  }
  compare_against?
  evidence_required[]
  output_schema
  trace_id
}
```

This is the canonical persisted review-packet envelope, not only an informal
payload sketch.

### Review Rules

- the reviewer reads primary files directly
- executor summaries are banned by default
- fresh thread is the default
- previous review rounds are only exposed through `compare_against`
- every review emits a trace artifact
- default reviewer input is built through `ReviewInputRedaction`; redacted
  executor-side sources are counted and named, but their content is not copied
  into the packet or prompt snapshot

### Banned Context

Do not pass:

- executor interpretations
- subjective conclusions
- pre-ranked findings
- "please confirm this is good" style leading prompts
- prior critique unless explicitly needed for compare mode

## 4. `ReviewTrace`

Every review run must leave an audit trail.

Required fields:

- `review_id`
- `thread_id`
- `model`
- `reasoning_effort`
- `prompt_snapshot`
- `file_list`
- `response_text`
- `verdict`
- `timestamp`
- `trace_path`

Trace retention follows the ARIS pattern:

- timestamped copies are permanent history
- a fixed-name latest copy may exist for downstream readers
- never overwrite the historical trace

## 5. Branch Evidence And Adjudication

`SearchBatch` and `BranchRun` need explicit evidence contracts.

### `BranchEvidencePack`

```text
BranchEvidencePack {
  batch_id
  branch_id
  hypothesis
  evidence_paths[]
  score_provenance
  reviewer_packet_id
  reviewer_verdict
  reproduction_steps[]
  promotion_decision
  superseded_by?
}
```

### `BranchAdjudicationRecord`

```text
BranchAdjudicationRecord {
  batch_id
  winning_branch_id?
  acceptance_rule
  archive_rule
  merge_rule
  rationale
}
```

### Adjudication Rules

- a branch can win only with score provenance
- the winner must pass review
- the winner must fit repo governance
- losing branches are archived, not merged silently

This is the multi-agent equivalent of a proof obligation.

## 5.5 Debate Protocol

Branch search is not the same thing as debate. Debate needs its own contract.

### `DebatePacket`

```text
DebatePacket {
  debate_id
  objective
  claim
  participating_branch_ids[]
  participating_agent_ids[]
  evidence_scope[]
  review_packet_id?
  adjudication_rule
}
```

### `DebateTurn`

```text
DebateTurn {
  debate_id
  turn_id
  speaker_branch_id
  turn_type
  claim_ref?
  evidence_refs[]
  rebuttal_to?
  verdict_delta?
}
```

### `DebateTrace`

```text
DebateTrace {
  debate_id
  turns[]
  accepted_claims[]
  rejected_claims[]
  contested_claims[]
  final_rationale
}
```

### Debate rules

- arguments must cite evidence refs, not free-form opinions
- rebuttals must point to a claim or evidence item
- unresolved disagreements remain contested, not silently collapsed
- debate outputs feed adjudication, not direct promotion

## 6. `ResearchSkillContract`

Each research skill must declare its contract explicitly.

```text
ResearchSkillContract {
  skill_id
  stage_id
  stage_class
  inputs[]
  outputs[]
  artifact_family
  preconditions[]
  evaluation_hooks[]
  citation_rules[]
  recovery_rules[]
  change_envelope_policy
  permission_mode
  tool_budget_policy
  rollback_policy
  success_edges[]
  failure_edges[]
  promotion_rule
  repair_targets[]
  required_agents[]
  reviewer_policy
}
```

Post-M10 generalization:

Research skills that produce externally visible outputs must also declare how
their outputs enter the `SkillOutputEnvelope` publication runtime:

- allowed `output_kind` values
- whether a `DocFrame` candidate is required
- artifact family and public-latest canonicality policy
- whether outputs are `private_only`, `candidate_only`, `requires_review`, or
  `winner_only_public_surface`
- whether human approval is required before publication
- whether the skill may propose code/doc/result/paper mutations, and the
  `ChangeEnvelope` policy that governs those mutations

The skill contract describes what a skill can produce. The publication runtime
decides what becomes public. No skill-specific metadata shape is allowed to
become the project-management source of truth.

### Mutation-bound stage rule

Any research skill that can modify code, docs, experiment configs, or result
artifacts must declare how it binds to `ChangeEnvelope` governance from
`18-proactive-project-ops.md`.

Minimum requirements:

- declare whether the stage is read-only or mutating
- declare the allowed path scope or the policy that computes it
- declare the permission mode expected for tool execution
- declare the maximum tool/token budget class
- declare whether rollback binding is mandatory before mutation begins

Stages that cannot satisfy these requirements are not eligible for autonomous
execution.

### Stage canonicalization rule

`stage_id` is the user-facing DAG node.

`stage_class` is the executable runtime class used by `ResearchStageExecution`.

Both must be declared. Runtime routing keys on `stage_class`; workflow
semantics and artifact expectations key on `stage_id`.

### Core Workflow DAG

The research engine is a DAG, not a checklist.

| Node | Output |
|---|---|
| literature | evidence map |
| idea | candidate hypotheses |
| novelty | validated novelty notes |
| refine | narrowed method plan |
| design-doc-sync | aligned design and experiment docs |
| experiment plan | runnable experiment protocol |
| implement-solution | code artifacts and tests |
| run | experiment artifacts |
| monitor | run health and metric traces |
| result-to-claim | claim support table |
| paper-plan | outline and section map |
| paper-write | draft artifacts |
| paper-compile | compiled submission artifact |
| research-review | critique and revision list |
| rebuttal | response packet |
| meta-optimize | harness improvements |

### Core Edges

| From | On success | On failure or insufficiency |
|---|---|---|
| literature | idea | literature refinement |
| idea | novelty | idea revision |
| novelty | refine | literature |
| refine | design-doc-sync | refine revision |
| design-doc-sync | experiment plan | refine |
| experiment plan | implement-solution | design-doc-sync |
| implement-solution | run | implement-solution revision |
| run | monitor | experiment plan or implement-solution |
| monitor | result-to-claim | run recovery |
| result-to-claim | paper-plan or refine | design-doc-sync or implement-solution or experiment plan |
| paper-plan | paper-write | paper-plan revision |
| paper-write | paper-compile | paper-write revision |
| paper-compile | research-review | paper-write |
| research-review | rebuttal or promote | design-doc-sync or paper-write |

The key point is that result failure routes back into the method graph instead
of just appending another stage.

### Nonlinear Execution Operations

The research runtime must model execution as a graph of stage attempts, not as a
single forward-only pipeline. A stage execution can create a new graph edge for
one of the following operation classes:

| Operation | Meaning | Typical trigger |
|---|---|---|
| `advance` | Move from a completed stage to its normal successor. | Evidence and gates pass. |
| `retry` | Re-run the same stage without changing the plan or hypothesis. | Transient tool, dependency, provider, or queue failure. |
| `repair` | Return to a nearby stage to fix an implementation, document, config, data, or run defect. | Tests fail, experiment crashes, review finds a concrete defect, integrity audit fails. |
| `pivot` | Return to an earlier hypothesis, method, claim, or plan stage because the current direction is not supported. | Result-to-claim is `no`, novelty fails, reviewer rejects the framing, or a baseline invalidates the claim. |
| `fork` | Explore multiple candidate plans, implementations, repairs, or narratives under a bounded search budget. | Several plausible directions exist and evidence is insufficient to choose one directly. |
| `supersede` | Mark prior artifacts or stage outputs as replaced by a newer canonical output while preserving history. | A new plan, code path, result table, or paper draft becomes the only public truth. |
| `abandon` | Stop a branch of the research graph without replacement. | Cost/risk is too high, objective is no longer relevant, or human director rejects the direction. |
| `human_override` | Record an explicit human decision that changes routing or authority. | User approves, blocks, narrows, expands, or redirects a research path. |

`retry`, `repair`, and `pivot` are not synonyms. Retry preserves the same
intent and inputs; repair changes a nearby artifact while preserving the
research direction; pivot changes the direction, claim, or hypothesis. The
runtime must store this distinction so downstream memory and project governance
can learn from failures instead of treating every loop as another attempt.

`fork` is the operation that binds M10 to M9. When a stage needs competing
candidate artifacts or executable hypotheses, M10 should create or reference an
M9 `SearchBatch`. M9 owns candidate branch/worktree creation, evaluation,
debate, and winner promotion. M10 owns the stage graph edge that requested the
search and the stage execution that consumes the promoted winner.

### `ResearchStageExecution`

Each stage attempt must be persisted as a graph node.

```text
ResearchStageExecution {
  schema_version
  execution_id
  stage_id
  stage_class
  parent_execution_id?
  root_execution_id
  operation
  trigger_reason
  input_artifacts[]
  output_artifacts[]
  decision_record?
  repair_targets[]
  supersedes[]
  m9_batch_id?
  human_gate?
  status
  started_at
  completed_at?
}
```

Required invariants:

- `stage_id` must resolve through `StageExecutionMap` before execution starts.
- mutating executions must satisfy the stage's `change_envelope_policy`.
- `fork` executions that produce competing code/doc/experiment artifacts must
  reference an M9 `SearchBatch` or explicitly record why branch search was not
  used.
- `supersede` must update artifact-family latest pointers and refresh DocFrames
  so public docs and code expose one latest truth.
- `abandon` preserves evidence and failure lessons but cannot leave active
  public artifacts behind.
- `human_override` must record who/what made the decision, what evidence was
  shown, and which normal gate was overridden or confirmed.

### Deliberation Layer

Long scientific work often spends many turns discussing a single stage before
any document, code, experiment, or paper artifact should be changed. This is
not limited to early ideation. Literature review, method design,
implementation debugging, result interpretation, paper writing, rebuttal, and
meta-optimization can all enter long deliberation loops.

Therefore M10 needs a stage-agnostic deliberation layer:

```text
ResearchThread {
  thread_id
  project_id
  title
  mission_frame_ref
  active_stage_execution_id?
  active_deliberation_span_id?
  status
  created_at
  updated_at
}

DeliberationSpan {
  span_id
  thread_id
  stage_execution_id
  mode
  turn_refs[]
  evidence_refs[]
  open_questions[]
  candidate_options[]
  rejected_options[]
  agreed_decisions[]
  review_findings[]
  pending_operations[]
  next_recommended_action
  confidence
  updated_at
}
```

`DeliberationSpan.mode` should be one of:

- `exploring`
- `comparing`
- `reviewing`
- `debugging`
- `interpreting_results`
- `drafting`
- `awaiting_human_gate`
- `ready_to_record`
- `ready_to_execute`

The deliberation layer is not a transcript summary. It is the runtime's compact
answer to:

- which research thread is active?
- which stage execution is the current discussion attached to?
- what is the user trying to decide or change?
- which questions remain open?
- which decisions are already agreed?
- does the latest turn require an operation edge?
- what should the CLI recommend next?

The runtime should update the active `DeliberationSpan` after every interactive
or one-shot turn that is classified as research-relevant. A turn normally
updates deliberation state only; it creates a new `ResearchStageExecution` edge
only when the conversation crosses an explicit boundary such as `advance`,
`retry`, `repair`, `pivot`, `fork`, `supersede`, `abandon`, or
`human_override`.

### CLI Integration

The user should not need to know M10 stage names. The CLI owns stage inference
and exposes correction surfaces.

Required behavior:

1. On each research-relevant turn, resolve or create a `ResearchThread`.
2. Resolve the active `ResearchStageExecution` through the thread, session,
   command intent, recent DocFrames, and MissionFrame.
3. Append the turn reference to the active `DeliberationSpan`.
4. Update open questions, evidence refs, candidate options, agreed decisions,
   and next recommended action.
5. Detect whether the latest turn proposes or requires a nonlinear operation.
6. If an operation is high-impact, mutating, costly, or public-surface
   changing, pause at a human gate instead of silently executing it.

### Turn Classification Safety

Natural-language research classification is advisory. It must not be the final
authority for public project changes.

`ResearchTurnClassification` should be a schema-owned object:

```text
ResearchTurnClassification {
  schema_version
  classification_id
  thread_id?
  stage_execution_id?
  span_id?
  research_relevant
  inferred_stage_id?
  inferred_mode?
  candidate_operation?
  confidence
  evidence_refs[]
  alternative_interpretations[]
  human_gate_required
  natural_language_summary
  next_recommended_action
  dry_run
}
```

Classification rules:

- deterministic command intent wins over LLM classification
- classification reads active thread, active stage, active deliberation span,
  MissionFrame, recent turns, DocFrames, and agreed decisions
- a normal turn may update deliberation fields, but it must not directly
  create irreversible DAG edges
- high-impact operations (`pivot`, `fork`, `supersede`, `abandon`, costly run
  launch, public artifact mutation) must become pending operations and require
  a human gate
- low-confidence or multi-interpretation results must set
  `human_gate_required=true`
- the CLI must ask for confirmation in natural language, not require the user
  to know internal DAG terms
- user correction can also be natural language; the runtime translates it into
  a revised classification or explicit `human_override`

The classifier may be backed by deterministic rules, the main runtime model, or
a slower reviewer/subagent path. The default hot path should not spawn a
subagent every turn. Subagents are appropriate for low-confidence, high-risk,
or evidence-heavy classification, and their output still enters the same
pending-gate mechanism.

Recommended operator surfaces:

- `research status --json` returns the active thread, stage execution,
  deliberation span, pending operations, and next recommended action.
- `research threads list --json` lists active and archived research threads.
- `research thread inspect <thread-id> --json` returns thread, stage graph,
  deliberation spans, evidence refs, and canonical artifact links.
- `research stage inspect <execution-id> --json` returns one stage execution
  with its deliberation history and operation edges.
- `research decide --operation <op> --json` records an explicit human decision
  for a pending operation.
- `research classify --text <turn> --dry-run --json` returns
  `ResearchTurnClassification` without mutating stage graph state.
- `research record --kind design|plan|result|paper|handoff --json` promotes
  agreed deliberation decisions into governed artifacts and refreshed
  DocFrames.

Prompt, resume, continue, and compact should inject a bounded projection of the
active `ResearchThread` and `DeliberationSpan` after `MissionFrame` and before
ordinary memory recall. This lets the CLI resume a long scientific discussion
by knowing the active research position before reading lower-priority memories.

Canonicality rule:

- deliberation state may be updated continuously
- public docs, code, experiment records, and paper artifacts change only through
  governed stage operations such as `research record`, `supersede`, M9
  promotion, or explicit human-approved mutation

## 6.5 Research Ops Semantics

### `StageExecutionMap`

The workflow DAG and runtime stage classes must map explicitly.

| stage_id | stage_class |
|---|---|
| `literature` | `survey` |
| `idea` | `idea_form` |
| `novelty` | `idea_refine` |
| `refine` | `idea_refine` |
| `design-doc-sync` | `document` |
| `experiment plan` | `experiment_design` |
| `implement-solution` | `implement` |
| `run` | `experiment_run` |
| `monitor` | `experiment_run` |
| `result-to-claim` | `result_to_claim` |
| `paper-plan` | `document` |
| `paper-write` | `document` |
| `paper-compile` | `publish` |
| `research-review` | `publish` |
| `rebuttal` | `publish` |
| `meta-optimize` | `repair` |

No autonomous routing is valid without a resolved `StageExecutionMap` entry.

The workflow DAG needs operational semantics, not only edges.

### Experiment routing rules

- sanity-stage runs before large deployments by default
- small milestone batches may run directly
- large sweeps, dependency chains, or multi-seed grids should route through a queue/scheduler contract
- result collection must write structured run logs before claim evaluation

### Required run artifacts

- run manifest
- experiment tracker
- experiment log
- integrity or audit status
- result-to-claim verdict

### Human checkpoint rules

- idea selection is a gate
- writing/submission transition is a gate
- integrity failure downgrades confidence even if metrics look positive

### Result-to-claim rules

- claims are judged after metrics are assembled, not while runs are still ambiguous
- `partial` verdicts route into supplementary experiment or narrower claim
- unsupported claims become explicit failures, not buried notes
- long-running queued runs should expose lease, heartbeat, and wake artifacts
- result-to-claim should consume finalized run artifacts, not transient console state

For the proactive runtime semantics that sit above these workflow rules, see
`docs/deep_study/18-proactive-project-ops.md`.

## 7. Citation And Source Contract

Every research claim must carry a source contract:

- source path or URL
- claim ID
- evidence span
- extraction method
- confidence level
- whether the claim is direct evidence or synthesis

Rules:

- synthesis claims must point to supporting evidence
- direct quotes must be traceable to a source span
- unsupported claims remain open issues, not hidden facts

The same source-span discipline should also apply to durable project-memory
summaries when those summaries are later used to guide implementation.

## 8. Artifact Table Contract

Every research workflow stage writes to an artifact family.

Required fields:

- `artifact_family`
- `canonical_id`
- `latest_id`
- `archive_ids`
- `supersession_chain`
- `source_branch_ids`

Promotion rules:

- write timestamped history first
- update latest pointer second
- update the family manifest third

This keeps version sprawl out of the repo.

## 9. Skill Manifest And Discovery

Research skills must be discoverable through manifests, not only named in docs.

```text
SkillManifest {
  skill_id
  version
  stage_id
  stage_class
  inputs[]
  outputs[]
  artifact_family
  required_agents[]
  reviewer_policy
  citation_rules[]
  recovery_rules[]
}
```

Required rules:

- manifests are schema-validated
- discovery reads manifests rather than free-form names
- routing uses `stage_id`, `stage_class`, and contract metadata
- upgrades are versioned
- manifests should also declare queue-vs-direct execution preference when relevant

## 10. Recovery And Rollback

- failed runs keep their evidence
- revised runs write new outputs instead of mutating old history
- review failures trigger a fresh packet or a revision wave
- branch winners are promoted only after review and repo checks

## 11. Why This Matters

This is what makes the research system actually usable:

- reviewers are independent
- evidence is auditable
- workflows are repeatable
- multi-agent debate becomes structured rather than noisy
- research outputs can be safely fed back into project memory
