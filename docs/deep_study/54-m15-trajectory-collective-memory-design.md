---
doc_frame:
  doc_id: docs.deep_study.54_m15_trajectory_collective_memory_design
  schema_version: "1"
  source_path: docs/deep_study/54-m15-trajectory-collective-memory-design.md
  title: M15 Trajectory Ingest And Collective Memory Design
  doc_type: milestone_design
  lifecycle: active
  scope: m15
  milestone: M15
  summary: Schema-first M15 design for ingesting session trajectories, segmenting them into task spans, deriving quality-gated memories, tracking adoption signals, and exposing explainable HOT/WARM/COLD tiers without bypassing research-cli governance.
  key_claims:
    - M15 turns real CLI, TUI, remote, agent, branch, and research-DAG runs into governed trajectory records rather than unstructured chat history.
    - ModelScope ULTRON's highest-value lesson is trajectory-to-memory collective learning: session jsonl ingest, LLM task segmentation, segment fingerprints, quality metrics, segment-tagged memory extraction, and tier/adoption signals.
    - Research-cli should exceed ULTRON by placing the same learning loop under project-local Git/canonicality, artifact-family latest gates, explicit invalidation, and M7/M8 memory explainability.
  decisions:
    - Add trajectory ingest as a background-capable but operator-visible lane; ingest records are not memory until a segment passes quality and promotion gates.
    - Use content fingerprints over normalized segment messages for idempotency and precise invalidation, following ULTRON's segment-level rule.
    - Keep LLM segmentation advisory and replayable: each segment stores boundaries, model/provider metadata, prompt digest, and fallback reason if deterministic segmentation is used.
    - Extract segment memories as candidates first, then promote through existing M8 durable-memory rules; M15 cannot directly create trusted injectable memory.
    - Track adoption events separately from memory content so HOT/WARM/COLD tier changes are explainable and reversible.
    - Do not add a supervised-training export lane; M15 may prepare quality-approved segments only for governed memory and later skill evolution.
  interfaces:
    - docs/deep_study/33-advanced-systems-implementation-plan.md
    - docs/deep_study/52-ultron-verifier-tailscale-reference-audit.md
    - docs/deep_study/05-exhaustive-memory-ecosystem.md
    - src/memory/mod.rs
    - schemas/memory_record.schema.json
  evidence_refs:
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/TrajectoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/docs/en/Components/MemoryHub.md
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/trajectory/segmenter.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/trajectory/trajectory_service.py
    - reference_repos/agent_teams/modelscope-ultron/ultron/services/memory/memory_service.py
  next_actions:
    - Treat M15 as complete for the deterministic, replayable release gate.
    - Continue with M16 memory-cluster skill evolution and skill catalog work.
    - Keep optional LLM segmentation for broad unmarked transcripts as a future enhancement after the deterministic governance contract remains stable.
    - Preserve the M15 rule that trajectory-derived records enter durable memory only through M8 promotion and invalidation gates.
  non_goals:
    - M15 does not implement M16 skill crystallization, M17 harness sharing, or any supervised-training export path.
    - M15 does not trust raw chat logs as canonical project truth.
    - M15 does not allow HOT memories to bypass M8 trust, invalidation, supersession, or artifact-family latest gates.
  generated_by: codex
  updated_at: 2026-04-29
---

# M15 Trajectory Ingest And Collective Memory Design

M15 is the bridge from project-local memory to collective learning. The unit of
learning is not a whole chat transcript and not a model summary. The unit is a
schema-owned task segment derived from an actual session trajectory.

## 1. Reference Lessons

ModelScope ULTRON is the controlling reference for this milestone.

`TrajectoryHub.md` defines the important contract:

- session `.jsonl` files are captured first as session metadata
- long sessions are split into independent task segments
- segment fingerprints are SHA-256-derived short content hashes over message
  role and content
- segment-level quality metrics gate memory extraction; ULTRON also uses them for training export, which research-cli intentionally does not adopt
- memories extracted from a segment are tagged with the segment id so later
  segment changes can archive or invalidate the old memories
- short chats may use a deterministic single-segment fallback
- LLM unavailability keeps sessions pending rather than silently accepting bad
  segmentation

`TrajectorySegmenter` proves the implementation shape: read the session file,
parse messages, run task segmentation, compute a fingerprint per segment, skip
matching fingerprints, and archive old segment memories when a segment at the
same index changes.

`MemoryHub.md` and `MemoryService` add the second half:

- memory upload sanitizes text before persistence
- near duplicates are merged instead of blindly appended
- search can boost HOT memories and demote COLD memories
- tier rebalance is driven by adoption signals, not by model preference alone
- memory clusters can later feed skill evolution, but that belongs to M16

## 2. Research-Cli Interpretation

Research-cli should not clone ULTRON's service boundary directly. Our runtime
already has stronger project truth and durable-memory governance, so M15 should
translate the useful ideas into local `.pmcli` records and CLI-visible gates.

The core rule:

```
raw trajectory -> task segment -> quality label -> memory candidate
  -> existing M8 promotion/invalidation/explain path -> tier/adoption surface
```

No step may jump directly from raw trajectory to trusted injectable memory.

## 3. Data Model

### 3.1 TrajectoryIngestRecord

One record per imported session source.

Required fields:

- `schema_version`: `trajectory.ingest_record.v1`
- `conformance_line`: `M15.trajectory_ingest`
- `ingest_id`
- `project_id`
- `source_kind`: `cli_session`, `tui_session`, `remote_session`,
  `agent_runtime`, `branch_run`, or `research_stage`
- `source_agent_id`
- `source_session_id`
- `source_path`
- `source_digest`
- `normalized_format`: `jsonl.v1`
- `ingest_status`: `pending_segmentation`, `segmented`,
  `segmentation_degraded`, `failed`, or `superseded`
- `segment_count`
- `degraded_reasons`
- `created_at`
- `updated_at`

Canonical path:

```
.pmcli/trajectory/ingests/<ingest_id>.json
```

Append log:

```
.pmcli/trajectory/ingests.jsonl
```

### 3.2 TaskSegmentRecord

One record per task segment.

Required fields:

- `schema_version`: `trajectory.task_segment.v1`
- `conformance_line`: `M15.task_segment`
- `segment_id`
- `ingest_id`
- `project_id`
- `segment_index`
- `start_line`
- `end_line`
- `message_count`
- `fingerprint`
- `topic`
- `summary`
- `segmentation_method`: `llm`, `deterministic_short_chat`,
  `deterministic_single_task`, or `manual_fixture`
- `segmentation_model`
- `segmentation_prompt_digest`
- `quality_status`: `pending`, `labeled`, `failed`, or `ineligible`
- `memory_extraction_status`: `not_started`, `candidate_created`,
  `skipped`, `invalidated`, or `failed`
- `segment_tag`: `segment:<segment_id-prefix>`
- `supersedes`
- `superseded_by`
- `created_at`
- `updated_at`

Canonical path:

```
.pmcli/trajectory/segments/<segment_id>.json
```

### 3.3 SegmentQualityRecord

Quality labels are records, not hidden fields, because the score and provider
may change over time.

Required fields:

- `schema_version`: `trajectory.segment_quality.v1`
- `conformance_line`: `M15.segment_quality`
- `quality_id`
- `segment_id`
- `provider`
- `model`
- `criteria`: named scores for task completion, evidence density, command/result
  grounding, artifact linkage, contradiction risk, privacy risk, and reuse value
- `overall_score`
- `memory_eligible`
- `failure_reason`
- `evidence_refs`
- `created_at`

Initial deterministic baseline:

- task completion from final assistant/tool outcome presence
- evidence density from command output, file refs, commit refs, and support refs
- privacy risk from secret/path/token detectors
- contradiction risk from links to superseded/invalidated memory

LLM quality labeling can be added later, but the deterministic fixture baseline
must pass first.

### 3.4 SegmentMemoryCandidate

M15 produces candidates for the M8 promotion path.

Required fields:

- `schema_version`: `trajectory.memory_candidate.v1`
- `conformance_line`: `M15.segment_memory_candidate`
- `candidate_id`
- `segment_id`
- `ingest_id`
- `project_id`
- `title`
- `summary`
- `body`
- `memory_kind`
- `support_refs`
- `source_artifacts`
- `segment_tag`
- `quality_id`
- `promotion_status`: `queued`, `promoted`, `rejected`, or `invalidated`
- `created_at`
- `updated_at`

Canonical path:

```
.pmcli/memory/promotion_queue/<candidate_id>.json
```

The candidate must include `segment_tag` and `quality_id`; promotion fails if
either is missing.

### 3.5 MemoryAdoptionEvent

Adoption is observed behavior, not model belief.

Event kinds:

- `retrieved`
- `inspected`
- `injected`
- `cited_in_output`
- `merged`
- `superseded`
- `invalidated`
- `rejected`

Canonical log:

```
.pmcli/memory/adoption_events.jsonl
```

Each event stores `memory_record_id`, optional `segment_id`, `event_kind`,
`weight`, `surface`, `actor`, `support_ref`, and `created_at`.

### 3.6 MemoryTierExplanation

Tier explanation is a projection derived from adoption events and M8 memory
status.

Fields:

- `schema_version`: `memory.tier_explanation.v1`
- `conformance_line`: `M15.memory_tier`
- `memory_record_id`
- `tier`: `HOT`, `WARM`, or `COLD`
- `adoption_score`
- `event_counts`
- `last_positive_event_at`
- `negative_event_count`
- `ranking_reason`
- `governance_limits`

`governance_limits` must explicitly say that tiering cannot override
invalidated, superseded, contested, expired, non-injectable, or non-latest
artifact constraints.

## 4. Processing Pipeline

### Step 1: Ingest

`memory trajectory ingest <path> --source-kind <kind>` normalizes session lines,
writes `TrajectoryIngestRecord`, and leaves the record pending segmentation.
The source path must be a workspace-relative existing file that canonicalizes
inside the current workspace. Absolute paths, `..` traversal, platform roots or
prefixes, and symlink escapes are rejected before ingest. The persisted
`source_path` is the normalized relative path so later re-ingest/supersession
logic cannot silently point at an external transcript.

### Step 2: Segment

`memory trajectory segment <ingest-id>` creates `TaskSegmentRecord` files.

The first graduated implementation should support:

- deterministic fixture segmentation from explicit marker comments
- short-chat fallback for two or fewer user/assistant turns
- single-task fallback when no marker exists and LLM segmentation is disabled

LLM segmentation is allowed after fixtures prove the record contract. When LLM
segmentation is unavailable, the ingest remains degraded or pending; it must not
silently create trusted segments.

### Step 3: Label

`memory trajectory label <segment-id>` writes `SegmentQualityRecord`.

The minimum gate for memory extraction:

- `overall_score >= 0.70`
- `privacy_risk <= 0.20`
- at least one support ref
- no known contradiction with a trusted latest artifact or invalidated memory

### Step 4: Extract

`memory trajectory extract <segment-id>` writes one or more memory candidates.
Extraction is rejected when quality is missing or ineligible.

### Step 5: Promote

Existing `memory promote` remains the only route into durable memory. M15 can
queue candidates but cannot directly write trusted records.

### Step 6: Tier

`memory tier rebalance` reads adoption events and durable memory state, computes
HOT/WARM/COLD explanations, and writes a projection. Tiering changes ranking
and explanation only; it does not change the durable memory's trust status.

## 5. Invalidation Rules

Re-ingesting the same source path with changed content creates a new digest. The
segmenter recomputes fingerprints.

- matching fingerprint: skip
- changed fingerprint at same segment index: supersede the old segment
- old segment has candidates only: mark candidates invalidated
- old segment has promoted memories: call the existing memory invalidation path
  with reason `trajectory_segment_superseded:<new_segment_id>`
- tier/adoption events remain append-only evidence

This is the key upgrade over ordinary transcript memory: stale experience is
not just lower-ranked; it is traceably invalidated.

## 6. Operator Surfaces

Minimum M15 commands:

- `memory trajectory ingest <path> --source-kind <kind> --json`
- `memory trajectory segment <ingest-id> --json`
- `memory trajectory label <segment-id> --json`
- `memory trajectory extract <segment-id> --json`
- `memory trajectory status --json`
- `memory trajectory explain <segment-id> --json`
- `memory adoption record <memory-id> --event <kind> --json`
- `memory tier rebalance --json`
- `memory tier explain <memory-id> --json`

Every command returns a typed JSON envelope and persists under `.pmcli`.
Ingest usage errors, including workspace-external sources, return typed
`usage_invalid` failures rather than internal errors.

## 7. Experiment Plan

The first experiment must be deterministic and local.

Fixture input:

- one `.jsonl` with two independent tasks
- one re-ingest where the second task changes
- one segment with low quality or missing support refs
- one segment containing a fake secret/token to trigger privacy rejection

Expected proof:

- ingest creates one `TrajectoryIngestRecord`
- segmentation creates two `TaskSegmentRecord` files
- re-ingest skips unchanged segment A and supersedes segment B by fingerprint
- memory extraction refuses low-quality/private segments
- eligible segment creates a promotion candidate with `segment_tag`
- promoting through M8 creates durable memory
- adoption events move the memory from WARM toward HOT in the explanation only
- invalidating/superseding the segment disables injection through existing M8
  invalidation, regardless of tier

## 8. Superiority Claim Gate

We may claim M15 exceeds ULTRON's trajectory memory layer only after all of
these are true:

- segment ingest/segmentation/fingerprint/idempotency are fixture-backed
- quality gating blocks bad/private/unsupported segments
- segment-tagged invalidation reaches promoted durable memory
- HOT/WARM/COLD tier explanations are visible and do not override governance
- M7/M8 query/explain surfaces show trajectory-derived provenance
- all behavior is project-local, Git/canonicality aware, and schema-valid

The current branch satisfies this gate for deterministic, replayable
project-local trajectory ingest. Optional LLM segmentation for broad unmarked
transcripts remains a future enhancement, not a release blocker for M15,
because the governance and invalidation contract is already fixture-proven.

## 9. Implementation Status

2026-04-29 implementation on branch
`feature/m15-post-product-plane`:

- Implemented six strict M15 schemas.
- Added `src/trajectory/mod.rs` as the deterministic M15 foundation.
- Added CLI surfaces for `memory trajectory ingest/segment/label/extract/status/explain`,
  `memory adoption record`, and `memory tier rebalance/explain`.
- Verified the first local trajectory-to-memory experiment:
  `.jsonl` ingest -> two marker-based task segments -> quality label ->
  segment-tagged memory candidate -> M8 durable promotion -> adoption events ->
  HOT tier explanation with explicit governance limits.
- Verified changed-span re-ingest supersession:
  unchanged segment fingerprints are reused, changed segment fingerprints create
  a new segment that supersedes the old segment, and old promoted memory becomes
  M8-invalidated/explain-only.
- Verified negative gates:
  unsupported segments and privacy-risk segments are marked ineligible and
  cannot create memory candidates.

M15 is therefore complete for the deterministic release gate. The remaining
ULTRON-derived collective-intelligence work is intentionally split into M16
skill evolution and M17 harness/profile sharing. Supervised-training export is
explicitly out of scope for this product line.
