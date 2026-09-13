# M15 Trajectory Collective Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build M15 trajectory ingest and collective memory so session `.jsonl` files become quality-gated, segment-tagged memory candidates with explainable adoption tiers.

**Architecture:** Add a focused trajectory module that writes schema-owned records under `.pmcli/trajectory/`, then hands eligible memory candidates to the existing M8 memory promotion queue. Keep segmentation and quality labeling deterministic first; optional LLM segmentation can be added after fixture contracts pass.

**Tech Stack:** Rust, serde/serde_json, JSON Schema files under `schemas/`, operator commands through `src/runtime/mod.rs`, memory integration through `src/memory/mod.rs`, conformance/operator tests in `tests/conformance/runtime/working_memory.rs` and `tests/operator/cli_surfaces.rs`.

---

## File Structure

- Create: `src/trajectory/mod.rs`
  - Owns M15 record structs, persistence helpers, deterministic segmentation,
    quality labeling, candidate extraction, adoption events, and tier
    explanations.
- Modify: `src/lib.rs`
  - Export the trajectory module.
- Modify: `src/runtime/mod.rs`
  - Add `memory trajectory *`, `memory adoption record`, and `memory tier *`
    command parsing and JSON-envelope output.
- Modify: `src/memory/mod.rs`
  - Accept M15 segment memory candidates in the existing promotion path and
    record adoption events from query/explain/injection surfaces where practical.
- Create: `schemas/trajectory_ingest_record.schema.json`
- Create: `schemas/task_segment_record.schema.json`
- Create: `schemas/segment_quality_record.schema.json`
- Create: `schemas/segment_memory_candidate.schema.json`
- Create: `schemas/memory_adoption_event.schema.json`
- Create: `schemas/memory_tier_explanation.schema.json`
- Modify: `tests/conformance/runtime/working_memory.rs`
  - Add schema and command tests for M15 memory/trajectory runtime behavior.
- Modify: `tests/operator/cli_surfaces.rs`
  - Add end-to-end operator fixture covering ingest, segment, label, extract,
    promote, adoption, tier explain, and invalidation.

## Task 1: Schemas And Record Types

**Files:**
- Create: `schemas/trajectory_ingest_record.schema.json`
- Create: `schemas/task_segment_record.schema.json`
- Create: `schemas/segment_quality_record.schema.json`
- Create: `schemas/segment_memory_candidate.schema.json`
- Create: `schemas/memory_adoption_event.schema.json`
- Create: `schemas/memory_tier_explanation.schema.json`
- Create: `src/trajectory/mod.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing schema conformance tests**

Add tests that construct minimal valid JSON values for each new schema and
validate them through the existing schema harness.

Run:

```bash
cargo test --test conformance m15_trajectory_collective_memory_schemas_lock_governed_learning_contracts -- --nocapture
```

Expected: FAIL because schemas and module do not exist yet.

- [ ] **Step 2: Add schemas**

Create strict draft-2020-12 schemas with `additionalProperties: false` and
`conformance_line` constants:

- `M15.trajectory_ingest`
- `M15.task_segment`
- `M15.segment_quality`
- `M15.segment_memory_candidate`
- `M15.memory_adoption`
- `M15.memory_tier`

- [ ] **Step 3: Add record structs**

In `src/trajectory/mod.rs`, add serde structs mirroring the schemas and helper
path functions:

```rust
pub fn trajectory_root(data_dir: &Path) -> PathBuf {
    data_dir.join("trajectory")
}
```

- [ ] **Step 4: Run schema tests**

Run:

```bash
cargo test --test conformance m15_trajectory_collective_memory_schemas_lock_governed_learning_contracts -- --nocapture
```

Expected: PASS.

## Task 2: Deterministic Ingest And Segmentation

**Files:**
- Modify: `src/trajectory/mod.rs`
- Modify: `src/runtime/mod.rs`
- Test: `tests/conformance/runtime/working_memory.rs`

- [ ] **Step 1: Write failing ingest/segment tests**

Create a temp project, write a session `.jsonl` with marker lines:

```jsonl
{"role":"user","content":"# task: build docs"}
{"role":"assistant","content":"Created docs/deep_study/example.md"}
{"role":"user","content":"# task: run tests"}
{"role":"assistant","content":"cargo test passed"}
```

Assert:

- `memory trajectory ingest <path> --source-kind cli_session --json` creates one
  ingest record
- `memory trajectory segment <ingest-id> --json` creates two segment records
- fingerprints are stable across repeated segmentation

- [ ] **Step 2: Implement ingest**

Persist `TrajectoryIngestRecord` to:

```text
.pmcli/trajectory/ingests/<ingest_id>.json
.pmcli/trajectory/ingests.jsonl
```

Compute `source_digest` from file bytes.

- [ ] **Step 3: Implement deterministic segmentation**

Support:

- marker-based fixture segmentation from `# task:` user messages
- short-chat fallback for two or fewer messages
- single-task fallback when no marker exists

Compute fingerprint as SHA-256 over normalized `role`, NUL separator, `content`,
and record separator, truncated to 16 hex chars, matching the ULTRON rule.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test conformance m15_trajectory_ingest_segments_extracts_and_tiers_memory_candidates -- --nocapture
```

Expected: PASS.

## Task 3: Re-Ingest Supersession And Segment-Tagged Invalidation

**Files:**
- Modify: `src/trajectory/mod.rs`
- Modify: `src/memory/mod.rs`
- Test: `tests/operator/cli_surfaces.rs`

- [ ] **Step 1: Write failing re-ingest test**

Use the same file path. First ingest has tasks A and B. Second ingest keeps A
unchanged but changes B. Assert:

- A fingerprint matches and is skipped
- B old segment is marked `superseded`
- B new segment records `supersedes`
- candidates or promoted memories tagged with the old segment become
  invalidated

- [ ] **Step 2: Implement supersession lookup**

For same `project_id + source_path + segment_index`, compare fingerprints.
Matching means no-op. Mismatch means old segment is superseded and any
segment-tagged candidate/promoted durable memory is invalidated through the
existing M8 path.

- [ ] **Step 3: Run focused operator test**

Run:

```bash
cargo test --test conformance m15_trajectory_reingest_supersedes_changed_segment -- --nocapture
```

Expected: PASS.

## Task 4: Quality Labeling And Candidate Extraction

**Files:**
- Modify: `src/trajectory/mod.rs`
- Modify: `src/runtime/mod.rs`
- Test: `tests/conformance/runtime/working_memory.rs`

- [ ] **Step 1: Write failing quality tests**

Assert low-quality and private segments do not create candidates:

- missing support refs -> `memory_eligible=false`
- fake token such as `sk-TESTSECRET` -> privacy risk blocks extraction
- normal command/result segment -> eligible candidate

- [ ] **Step 2: Implement deterministic quality labeler**

Calculate criteria:

- task completion
- evidence density
- artifact linkage
- contradiction risk
- privacy risk
- reuse value

Require `overall_score >= 0.70`, low privacy risk, and at least one support ref.

- [ ] **Step 3: Implement candidate extraction**

`memory trajectory extract <segment-id> --json` writes an M15 candidate to
`.pmcli/memory/promotion_queue/<candidate_id>.json` with `segment_id`,
`segment_tag`, and `quality_id`.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test conformance m15_trajectory_quality_and_extract -- --nocapture
```

Expected: PASS.

## Task 5: Adoption Events And Tier Explanation

**Files:**
- Modify: `src/trajectory/mod.rs`
- Modify: `src/memory/mod.rs`
- Modify: `src/runtime/mod.rs`
- Test: `tests/conformance/runtime/working_memory.rs`

- [ ] **Step 1: Write failing tier tests**

Create a promoted memory, record adoption events, run tier rebalance, and
assert:

- tier explanation exists
- positive events increase adoption score
- invalidated memory remains non-injectable even if tier is HOT

- [ ] **Step 2: Implement adoption event append log**

Persist events to:

```text
.pmcli/memory/adoption_events.jsonl
```

Support explicit operator command first:

```bash
research-cli memory adoption record <memory-id> --event inspected --json
```

- [ ] **Step 3: Implement tier rebalance and explain**

Compute an adoption score from event weights and write:

```text
.pmcli/memory/tiers/<memory_id>.json
```

The explanation must include governance limits showing that tier cannot override
invalidated, superseded, contested, expired, non-injectable, or non-latest
artifact constraints.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test --test conformance m15_memory_adoption_tier_explain -- --nocapture
```

Expected: PASS.

## Task 6: End-To-End Experiment

**Files:**
- Modify: `tests/operator/cli_surfaces.rs`
- Use fixture generated inside temp project test

- [ ] **Step 1: Write the full operator experiment**

The test should cover:

1. ingest a two-task `.jsonl`
2. segment it
3. label both segments
4. extract an eligible candidate
5. promote through M8
6. record adoption events
7. rebalance tier
8. re-ingest changed second task
9. verify old segment and memory are invalidated

- [ ] **Step 2: Run the full experiment**

Run:

```bash
cargo test --test conformance m15_trajectory_collective_memory_end_to_end -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run wider verification**

Run:

```bash
cargo fmt --check
cargo check
cargo test --test conformance m15_trajectory
```

Expected: PASS.

## Completion Gate

M15 is complete only when:

- [x] all six schemas are present and validated
- [x] all operator commands return typed JSON envelopes
- [x] re-ingest is idempotent by fingerprint
- [x] changed segments invalidate old candidates/promoted memory
- [x] low-quality/private segments cannot create candidates
- [x] durable memory still goes through M8 promotion
- [x] HOT/WARM/COLD explains adoption without bypassing governance
- [x] docs and tests support the superiority claim against ULTRON's trajectory and
  memory hubs
