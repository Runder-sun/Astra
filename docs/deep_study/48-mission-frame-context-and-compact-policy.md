# MissionFrame Context And Compact Policy

This document adds a persistent goal-alignment layer to the project-memory
architecture. It formalizes the "memory palace" idea that every active context
should carry a small, stable hierarchy of goals:

```text
project_max_goal > milestone_goal > current_implementation_goal
```

The purpose is not to add more prompt stuffing. The purpose is to preserve
long-horizon intent through turns, compaction, resume, subagent launches, and
reviews.

## 1. Design Position

`MissionFrame` is a project-scoped, schema-owned context anchor. It is not a
transcript summary and not an ordinary memory record.

It answers:

- what is the largest project objective?
- what milestone is currently being advanced?
- what minimal implementation goal is active now?
- what non-goals prevent local overreach?
- what evidence proves alignment or drift?

It should live beside project/session state, not inside a model-generated
summary only.

Canonical storage target:

```text
.pmcli/project_goals/mission_frame.json
```

Canonical schema target:

```text
schemas/mission_frame.schema.json
```

## 2. Priority Semantics

The goal hierarchy is a project-internal conflict rule:

```text
project_max_goal outranks milestone_goal
milestone_goal outranks current_implementation_goal
current_implementation_goal outranks local implementation convenience
```

It never outranks:

1. system, developer, or user instructions
2. safety policy
3. permission and workspace-boundary policy
4. explicit rollback or stop requests

This distinction is critical. `MissionFrame` guides project coherence; it does
not grant authority to ignore the operator.

## 3. Data Shape

Minimum object:

```text
MissionFrame {
  frame_id
  version
  project_id
  project_max_goal
  milestone_goal
  current_implementation_goal
  priority_rule
  non_goals[]
  success_criteria[]
  evidence_refs[]
  risk_notes[]
  updated_at
}
```

Required constraints:

- `project_max_goal`, `milestone_goal`, and `current_implementation_goal` are
  concise enough for automatic injection.
- `evidence_refs` point to docs, schemas, tests, artifacts, or review packets.
- `non_goals` are first-class; they stop the model from using the max goal to
  justify scope creep.
- changes create a new version or event, not silent mutation.

## 4. Injection Policy

`MissionFrame` is part of fixed-budget wake-up context.

Recommended lanes:

- prompt / one-shot turn: inject compact frame before task-specific context
- resume / continue: inject frame before recap
- compact: preserve the frame verbatim or by canonical reference
- inspect: expose frame status and provenance
- subagent launch: copy frame into the task packet
- review: include a goal-alignment section

Recommended L0 budget:

```text
max_mission_frame_tokens = 200..500
```

If the frame is too large, the runtime must fail closed into an inspectable
degraded state or inject a canonical short projection. A summarizer may not
freely rewrite the authoritative frame.

## 5. Compact Semantics

Compaction must not erase or reinterpret the project goal hierarchy.

Required behavior:

1. load the current `MissionFrame`
2. include `mission_frame_ref` in the compaction result
3. include a short immutable projection in the compacted markdown
4. preserve the canonical frame outside the summary artifact
5. emit a goal-alignment trace when compacted work appears to drift

The summary may say what changed during the session. It must not silently change
the max goal, milestone goal, or current implementation goal.

## 6. Alignment Trace

Every non-trivial implementation or review lane should be able to emit:

```text
GoalAlignmentTrace {
  mission_frame_ref
  command
  alignment_status
  alignment_method
  mission_frame_present
  project_goal_supported
  milestone_goal_supported
  implementation_goal_supported
  support_scores
  drift_risks[]
  drift_reasons[]
  evidence_refs[]
  recommended_next_action
}
```

This trace makes the first-principles rule testable:

- if a local implementation passes tests but harms the milestone, it is not
  considered aligned.
- if a milestone action conflicts with the project max goal, the runtime should
  surface a follow-up or policy refusal instead of continuing silently.

Current implementation note:

- `compact` emits a schema-backed `goal_alignment_trace` event.
- The runtime reads the generated summary artifact, scores only the
  `Recent Highlights` section against the three `MissionFrame` goals, and keeps
  the immutable `MissionFrame Projection` out of the support score so goal
  injection does not make every summary look aligned.
- `alignment_status` is one of `aligned`, `drift_risk`,
  `mission_frame_missing`, or `needs_review`. A drift-risk trace carries
  `drift_reasons`, `drift_risks`, numeric `support_scores`, and a
  `review_compaction_against_mission_frame` next action.

## 7. Relationship To Memory

`MissionFrame` is not a replacement for durable memory.

It is a privileged memory plane:

- lower churn than working memory
- more active than cold durable memory
- stronger than summary-only recap
- weaker than user/system instructions

It should be queryable by memory surfaces, but ordinary memory promotion,
decay, and compaction must not invalidate it. Only explicit operator action or
governed project policy may update it.

## 8. Relationship To DocFrame

`DocFrame` is the document-scoped companion to `MissionFrame`.

The canonical design is
`49-document-info-block-and-docframe-policy.md`.

`DocFrame` objects may summarize project documents, skill-generated outputs,
milestone plans, review packets, and handoffs. They can be used to derive
candidate project, milestone, and implementation summaries. They must not
silently rewrite the canonical `MissionFrame`.

Required behavior:

1. `DocFrame` may propose a `MissionFrame` refresh with evidence refs.
2. `MissionFrame` remains the project-scoped goal authority.
3. compact/resume may use `DocFrame` projections to rebuild context packets.
4. conflicts between `MissionFrame` and document-derived candidates must emit a
   goal-alignment or stale-doc warning, not an automatic overwrite.

This gives document management a structured context layer while preserving the
first-principles goal hierarchy.

## 9. First Implementation Slice

Minimum useful slice:

1. create `schemas/mission_frame.schema.json` - implemented
2. add `goals status` and `goals set` or equivalent project command -
   implemented
3. store `.pmcli/project_goals/mission_frame.json` - implemented
4. inject a short frame projection into prompt/resume/continue - implemented
5. make compact preserve `mission_frame_ref` and bounded markdown projection -
   implemented
6. add conformance tests for compact/resume retention - implemented

This slice is intentionally small. It adds project coherence without requiring
the full durable memory subsystem to be graduated first.

Current implementation status:

- canonical storage: `.pmcli/project_goals/mission_frame.json`
- canonical schemas: `schemas/mission_frame.schema.json` and
  `schemas/mission_frame_status.schema.json`
- operator surfaces: `goals status --json` and `goals set --json`
- runtime lanes: prompt, resume, continue, and compact expose the active
  MissionFrame projection when present
- compact artifacts: compact JSON preserves `mission_frame_ref`; compacted
  markdown includes a bounded immutable MissionFrame projection
- guardrail: stale copied frames from another resolved project are rejected
