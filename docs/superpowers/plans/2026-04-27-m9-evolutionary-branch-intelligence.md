# M9 Evolutionary Branch Intelligence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the M9 proof surface for Git-native evolutionary branch intelligence with branch batches, candidate runs, evaluation, debate, promotion, and archive gates.

**Architecture:** Add a focused `src/branches` module that owns M9 persistence under `.pmcli/branches/`, then route CLI commands through `src/runtime/mod.rs`. M9 now proves scheduling, worktree-backed mutation, evaluation, debate, lineage, promotion gates, and gated automatic winner merge. LLM mutation enters through a command adapter so real provider/agent drivers can write inside candidate worktrees without weakening canonical governance.

**Tech Stack:** Rust, serde JSON records, existing `.pmcli` project data layout, existing CLI success/failure envelopes, operator integration tests, schema conformance fixtures.

---

### Task 1: Branch Search RED Tests

**Files:**
- Modify: `tests/operator/cli_surfaces.rs`

- [x] Add a test that `branches search --objective <text> --max-branches 3 --strategy evolutionary --json` creates a `SearchBatch` with three `BranchRun` candidates, variation operators, lineage records, real Git worktrees, and hidden candidate artifacts.
- [x] Add a test that `branches promote <branch-id> --json` fails before evaluation and debate with `branch_promotion_blocked`.
- [x] Run: `cargo test --test conformance m9_ -- --nocapture`.
- [x] Expected RED observed before implementation for missing real worktrees, real eval command execution, and canonicality gate integration.

### Task 2: Branch Module And Search Surface

**Files:**
- Create: `src/branches/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/commands/help.rs`

- [x] Implement `SearchBatchRecord`, `BranchRunRecord`, and `BranchListResult` structs.
- [x] Persist batch/run records under `.pmcli/branches/batches/`, `.pmcli/branches/runs/`, and append lineage under `.pmcli/branches/lineage.jsonl`.
- [x] Add `branches search/list/inspect` routing.
- [x] Add `branches` to help registry as a graduated advanced command.
- [x] Run the focused tests from Task 1.
- [x] Expected: search/list/inspect pass; promotion test remains gated until evaluation/debate.

### Task 3: Evaluation And Debate Gates

**Files:**
- Modify: `src/branches/mod.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `tests/operator/cli_surfaces.rs`

- [x] Add tests for `branches evaluate <branch-id> --json` producing an `EvaluationPacket` with stale-base, changed paths, claimed paths, objective metrics, real test command outcomes, canonicality gate, and reviewer gate fields.
- [x] Add tests for `branches debate <branch-id> --against <branch-id> --json` producing a `DebateTrace` with support/opposition/recommendation.
- [x] Implement evaluation and debate persistence.
- [x] Verify promotion remains blocked until both evaluation and debate exist.

### Task 4: Promotion, Archive, And Single-Winner Rule

**Files:**
- Modify: `src/branches/mod.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `tests/operator/cli_surfaces.rs`

- [x] Add tests that evaluated+debated candidate can promote.
- [x] Add tests that a second candidate in the same batch cannot also promote.
- [x] Add tests that `branches archive <branch-id> --reason <text> --json` moves the run to archived status without deleting audit records.
- [x] Implement `PromotionDecision` and `ArchiveResult`.
- [x] Verify loser cleanup keeps records hidden under `.pmcli/branches/`.
- [x] Implement `branches mutate <branch-id> --llm-command <cmd> --json` so LLM/agent mutation executes inside the candidate worktree, commits the changed hypothesis, and invalidates stale evaluation/debate/promotion refs.
- [x] Implement default automatic winner merge on `branches promote`, with `--no-merge` preserving director-decision-only mode.
- [x] Verify auto-merge excludes hidden candidate artifacts, commits only public winner diff, blocks dirty public source worktrees, and rolls back if post-merge canonicality fails.

### Task 5: Schema And Documentation Graduation

**Files:**
- Create: `schemas/search_batch_record.schema.json`
- Create: `schemas/branch_run_record.schema.json`
- Create: `schemas/evaluation_packet.schema.json`
- Create: `schemas/debate_trace.schema.json`
- Create: `schemas/promotion_decision.schema.json`
- Modify: `tests/conformance/schemas/batch10_enforcement.rs`
- Modify: `docs/deep_study/30-milestone-execution-handbook.md`
- Modify: `docs/deep_study/33-advanced-systems-implementation-plan.md`
- Modify: `docs/deep_study/34-advanced-build-task-pack-backlog.md`
- Modify: `docs/deep_study/51-m6-m10-reference-superiority-design-audit.md`

- [x] Add strict schemas for M9 records with top-level unknown-field rejection, enum/range gates, mutation result shape, promotion merge fields, and evaluation outcome shape.
- [x] Register schema names in conformance enforcement.
- [x] Update milestone docs to describe evolutionary branch intelligence and proof evidence.
- [x] Run verification: `cargo test --test conformance m9_ -- --nocapture`, schema compile tests, and full `cargo test --quiet`.
