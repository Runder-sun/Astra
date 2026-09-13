---
doc_frame:
  doc_id: docs.deep_study.55_m16_m18_skill_evolution_feedback_design
  schema_version: "1"
  source_path: docs/deep_study/55-m16-m18-skill-evolution-feedback-design.md
  title: M16-M18 Skill Evolution And Feedback Design
  doc_type: milestone_design
  lifecycle: active
  scope: m16_m18
  milestone: M16-M18
  summary: Schema-first design for implementing M16 skill evolution and M18 feedback calibration while deferring M17 harness sharing and excluding supervised-training export.
  key_claims:
    - M16 absorbs ModelScope ULTRON SkillHub's useful loop: cluster repeated memories, crystallize reusable workflows, verify provenance, and gate structure upgrades.
    - Research-cli exceeds the reference by making evolved skills governed candidates only; publication still belongs to M11 SkillOutputEnvelope and canonical public-latest gates.
    - M18 turns memory adoption and skill verification into advisory calibration signals without granting authority to publish truth, merge code, widen tools, or train models.
  decisions:
    - Keep M17 harness/profile sharing deferred; it is useful but not needed for the current learning loop.
    - Use deterministic local clustering and verification first; LLM/embedding providers may improve recall later but must preserve replayable records.
    - Treat feedback as a recommendation layer, not a governance layer.
  evidence_refs:
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/skill/skill_cluster.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/skill/skill_evolution.py
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/SkillHub.md
  generated_by: codex
  updated_at: 2026-04-29
---

# M16-M18 Skill Evolution And Feedback Design

M16 and M18 are the post-M15 collective-learning layer. M15 creates governed
trajectory-derived memory. M16 asks whether repeated memories imply a reusable
workflow. M18 asks whether later operator behavior confirms or weakens the
runtime's recommendations.

The controlling invariant is:

```
memory evidence -> skill candidate / feedback signal -> governed projection
```

No learning record may become project truth by itself.

## 1. Reference Lessons

ModelScope ULTRON's SkillHub has three valuable ideas:

- memory clustering by semantic similarity and a crystallization threshold
- crystallization/re-crystallization into reusable workflow skills
- provenance and structure-score gates before a new skill version can replace
  an older version

The stronger product lesson is not "generate more skills". It is to require an
evidence trail from repeated work to reusable process.

Research-cli should exceed this by binding every evolved skill to existing
governance:

- clusters are evidence records under `.pmcli/skills/evolution/`
- crystallized skills are candidates, not installed public skills
- verification labels grounding, hallucination, and contradiction
- structure score checks workflow clarity, specificity/reusability, and
  preservation of existing value
- publication must enter M11 `SkillOutputEnvelope`; M16 has no direct
  public-latest authority
- optional local install must enter the existing skill search path
  `.codex/skills/<skill-id>/SKILL.md` only after verification and explicit
  human approval, with fail-closed path scope and no silent overwrite of an
  existing local skill

## 2. M16 Contract

Records:

- `KnowledgeClusterRecord`: deterministic cluster of active injectable durable
  memories, with `similarity_threshold`, `crystallization_threshold`,
  `memory_ids`, support refs, and readiness state. Clustering is implemented as
  deterministic weighted token-overlap connected components over memory
  summaries/titles/bodies, not a first-token bucket and not a single all-memory
  fallback. This keeps word-order variants together while separating unrelated
  retrieval, publication, review, or experiment patterns into distinct
  crystallization candidates.
- `SkillEvolutionCandidate`: generated skill artifact plus source memory ids,
  support refs, verification status, and publication status.
- `SkillProvenanceVerification`: grounded, hallucinated, contradicted claim
  counts; grounded evidence ratio; verification decision.
- `SkillStructureScore`: ULTRON-style weighted score:
  `0.35 workflow_clarity + 0.35 specificity_and_reusability +
  0.30 preserves_existing_value`.
- `SkillInstallRecord`: project-local install evidence for a verified candidate
  copied into the existing skill registry path, with human-gate approval and
  support refs. The candidate artifact path must be a workspace-relative file
  that canonicalizes inside the workspace; absolute paths, `..` traversal,
  symlink escapes, and existing `.codex/skills/<skill-id>/SKILL.md` targets are
  rejected by default.

Command surface:

- `skills evolve cluster --min-members <n> --json`
- `skills evolve crystallize <cluster-id> --json`
- `skills evolve verify <candidate-id> --json`
- `skills evolve submit <candidate-id> --json`
- `skills evolve install <candidate-id> --approve-human-gate --json`
- existing `prompt status`, `resume`, and `compact` include verified evolution
  candidates in `skill_outputs.evolved_recommendations` as advisory context
  only

Publication rule:

```
skills evolve submit -> SkillOutputEnvelope(review_candidate)
```

The operator may later publish through `skills publish --execute`, subject to
the same canonicality and human-gate rules as any M11 output.

Prompt/resume/compact projection rule:

```
skill_output_context_projection.public_latest = only M11 canonical public-latest
skill_output_context_projection.evolved_recommendations = verified M16 candidates only
```

This is the key integration boundary. M16 improves the existing CLI context
plane by helping the operator notice reusable verified workflows, but it does
not install them, expose private candidates, or change the single public-latest
contract.

Install rule:

```
verified candidate + explicit human gate -> .codex/skills/<skill-id>/SKILL.md
```

Installed evolved skills are normal local skills. `skills list` and
`skills inspect` discover them through the existing registry; there is no second
catalog or side loader. Install is intentionally separate from M11 publication:
publication governs project truth, while install governs local operator
capability.

## 3. M18 Contract

M18 consumes already-governed signals:

- M15 memory adoption events
- M16 skill verification results
- future M12 verifier tournament outcomes

Records:

- `AgentFeedbackSignal`: normalized signal with target lane, polarity, weight,
  evidence refs, and `authority_boundary=advisory_only`.
- `FeedbackCalibrationRecord`: latest advisory projection containing routing
  adjustments, skill recommendation hints, verifier calibration hints, and
  hard governance limits.

Command surface:

- `feedback collect --json`
- `feedback calibrate --json`
- `feedback status --json`
- existing `memory query` consumes the latest advisory calibration through an
  explicit `feedback_calibration` retrieval lane when the calibration permits
  `memory_retrieval`
- `feedback calibrate` refreshes collected governed signals first, so direct
  adoption -> calibrate -> query flows work without requiring the operator to
  remember an intermediate `feedback collect`

Authority boundary:

- feedback can explain why a memory/skill should be recommended
- feedback can influence future ranking only through explicit projections
- feedback cannot publish canonical docs, promote branches, merge code, widen
  tools, bypass skill gates, or create a supervised-training export path

Memory-query integration rule:

```
feedback calibration -> memory query feedback_calibration lane -> matched_via / feedback_adjustments
```

The lane is signed and explainable. Positive adoption adds a
`feedback_calibration` ranking lane. Negative adoption records
`memory_adoption:negative_weight=<w>` and subtracts an explicit feedback penalty
after rank fusion, so rejected memories are actually downranked rather than
only explained. The query result route marks `_feedback_calibrated` only when
an advisory calibration is present, preserving replayability for uncalibrated
runs.

## 4. M17 Deferral

M17 harness/profile sharing remains useful but is deliberately deferred. It
needs a separate review because it imports or exports environment conventions,
tool presets, and product profiles. Those operations have a wider blast radius
than M16/M18 and must include allowlists, secret exclusion, dry-run diffs,
backups, and product-specific merge rules.

## 5. Completion Gate

M16/M18 are complete when:

- all six schema families compile and reject unknown top-level fields
- conformance proves memory clustering, crystallization, provenance
  verification, M11 candidate submission, feedback collection, calibration, and
  advisory status
- conformance proves clustering separates distinct workflow topics instead of
  collapsing all active memories into one skill candidate
- conformance proves verified evolved skills can be installed into the existing
  local skill registry only after verification and explicit human approval, and
  that install rejects unverified candidates, workspace-escaping artifact paths,
  and existing manifest overwrites
- conformance proves M16/M18 are wired into existing CLI surfaces:
  `memory query` for feedback-calibrated recall and `prompt/resume/compact`
  skill-output projection for verified evolved-skill recommendations
- conformance proves negative adoption can move an otherwise highly relevant
  memory below a comparable un-rejected memory while preserving an auditable
  `feedback_adjustments` explanation
- no command creates public-latest artifacts, durable memory, branch merges, or
  tool authority outside existing governance
- docs no longer describe supervised-training export as a roadmap requirement

This is enough to claim that research-cli has absorbed ULTRON's memory-to-skill
learning loop and exceeded it in project-local governance and publication
discipline. It is not a claim that M17 harness sharing or native mobile remote
control are complete.
