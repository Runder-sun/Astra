# Goal-Driven Research Board Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first read-only goal-driven research board projection from existing orchestration, research, agent, review, ProjectOps, and host-surface state.

**Architecture:** Add a native projection type under `src/research/mod.rs`, then attach it to `HostResearchSummary` so TUI, web, and mobile receive one kernel-owned view. The first slice is read-only and derives entries from current authorities; it does not create a new board database, write path, scheduler, or permission model.

**Tech Stack:** Rust, serde, existing `research-cli` CLI/runtime contracts, existing mobile HTML/CSS/JS assets.

---

### Task 1: Add Read-Only Research Board Projection

**Files:**
- Modify: `src/research/mod.rs`
- Test: `src/research/mod.rs`

- [ ] **Step 1: Write the failing unit test**

Add a test that builds a `ResearchStatusReport` with an active thread, stage, deliberation span, pending operations, and HITL gate, then calls the new projection builder and expects typed board buckets:

```rust
#[test]
fn research_board_projection_derives_entries_from_existing_status() {
    let report = sample_research_status_report();
    let projection = ResearchBoardProjection::from_status(&report, None);

    assert_eq!(projection.schema_version, "research_board_projection.v1");
    assert_eq!(projection.authority_model, "derived_from_existing_research_authorities");
    assert!(projection.buckets.iter().any(|bucket| bucket.bucket_id == "questions"));
    assert!(projection.entries.iter().any(|entry| entry.bucket_id == "questions"));
    assert!(projection.entries.iter().any(|entry| entry.bucket_id == "ready_to_run"));
    assert!(projection.entries.iter().any(|entry| entry.bucket_id == "needs_approval"));
    assert!(projection.entries.iter().all(|entry| entry.write_authority == "read_only_projection"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test research::tests::research_board_projection_derives_entries_from_existing_status --lib`

Expected: FAIL because `ResearchBoardProjection` does not exist.

- [ ] **Step 3: Implement minimal projection types**

Add `ResearchBoardProjection`, `ResearchBoardBucket`, `ResearchBoardEntry`, and `ResearchBoardEntrySource`. Derive buckets from open questions, pending operations, middleware gates, active stage, and optional orchestration progress. Keep all entries read-only.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test research::tests::research_board_projection_derives_entries_from_existing_status --lib`

Expected: PASS.

### Task 2: Attach Projection To Host Surface

**Files:**
- Modify: `src/host_surface.rs`
- Test: `src/host_surface.rs`

- [ ] **Step 1: Write the failing unit test**

Extend the active-thread host research summary test to assert `summary.board` is present and contains the active question plus next-action entries.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test host_surface::tests::host_research_summary_active_thread_projects_brief_not_debug_ids --lib`

Expected: FAIL because `HostResearchSummary` has no `board` field.

- [ ] **Step 3: Add board field to HostResearchSummary**

Call the research projection builder from `host_research_summary_from_projection` or a companion helper. Preserve existing fields and defaults for backward compatibility.

- [ ] **Step 4: Run targeted tests**

Run: `cargo test host_surface::tests::host_research_summary_no_active_thread_uses_product_brief_copy host_surface::tests::host_research_summary_active_thread_projects_brief_not_debug_ids --lib`

Expected: PASS.

### Task 3: Expose Board Through CLI Host Surface Contract

**Files:**
- Modify: `tests/operator/cli_surfaces.rs`
- Modify if needed: `schemas/host_surface_projection.schema.json`

- [ ] **Step 1: Write the failing operator test**

Add assertions to the host surface status test that `data.projection.research.board` exists, has `schema_version == "research_board_projection.v1"`, and marks the board as derived/read-only.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test conformance host_surface_status_and_tui_projections_share_remote_workbench_contract -- --nocapture`

Expected: FAIL until the host surface serializes board data and schema accepts it.

- [ ] **Step 3: Update implementation/schema minimally**

If the host surface schema rejects the new board field, extend only the research summary portion of `schemas/host_surface_projection.schema.json`.

- [ ] **Step 4: Run targeted operator and schema tests**

Run:

```bash
cargo test --test conformance host_surface_status_and_tui_projections_share_remote_workbench_contract -- --nocapture
cargo test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture
```

Expected: PASS.

### Task 4: Reuse Mobile Research Board Shell

**Files:**
- Modify: `src/assets/mobile/app.js`
- Modify if needed: `src/assets/mobile/styles.css`

- [ ] **Step 1: Write the failing static contract test**

Add or extend an existing JS/static assertion only if the repo already has a suitable test harness. If no mobile static test exists, validate through grep-backed review and CLI host surface JSON after implementation.

- [ ] **Step 2: Render board entries from projection**

Update `renderKanbanPage()` to prefer `research.board.entries` over locally invented `claims` and `findings`. Keep the existing page shell and progress/stage sections.

- [ ] **Step 3: Keep existing fallback behavior**

If `research.board` is absent, the current fallback behavior must still render without errors.

### Task 5: Final Verification

**Files:**
- All touched files

- [ ] **Step 1: Format**

Run: `cargo fmt`

- [ ] **Step 2: Run targeted Rust tests**

Run:

```bash
cargo test research::tests::research_board_projection_derives_entries_from_existing_status --lib
cargo test host_surface::tests::host_research_summary_no_active_thread_uses_product_brief_copy host_surface::tests::host_research_summary_active_thread_projects_brief_not_debug_ids --lib
cargo test --test conformance host_surface_status_and_tui_projections_share_remote_workbench_contract -- --nocapture
```

- [ ] **Step 3: Run schema validation if schema changed**

Run:

```bash
cargo test --test conformance host_surface_and_terminal_payloads_validate_against_m14_schemas -- --nocapture
```

- [ ] **Step 4: Update trellis task notes**

Update `.trellis/tasks/05-06-goal-driven-research-automation/README.md` with implementation status and validation commands.
