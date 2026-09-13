# Rust Kernel Pivot Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the active `research-cli` kernel implementation from the temporary Go-style bootstrap to a Rust bootstrap that preserves the same authority boundaries and starts the first proof-bearing Batch 0 work.

**Architecture:** Keep the design authority unchanged: one kernel truth, one `.pmcli/` protocol surface, one remote adaptation line, one project-native memory and ProjectOps line. Replace only the local bootstrap substrate with Rust under `Cargo.toml`, `src/`, `tests/`, and `schemas/`, while treating old Go-style file paths as legacy ownership aliases via `46-rust-kernel-pivot-and-bootstrap.md`.

**Tech Stack:** Rust (`cargo`, `rustc`, standard library first), TypeScript/Node remote adaptation, Python research sidecars, JSON schemas under `schemas/`, conformance tests under `tests/`.

---

### Task 1: Freeze The Rust Pivot In Docs

**Files:**
- Modify: `README.md`
- Modify: `docs/deep_study/README.md`
- Modify: `docs/deep_study/11-implementation-blueprint.md`
- Modify: `docs/deep_study/31-kernel-foundation-implementation-plan.md`
- Modify: `docs/deep_study/32-first-build-task-pack-backlog.md`
- Modify: `docs/deep_study/33-advanced-systems-implementation-plan.md`
- Modify: `docs/deep_study/36-repo-bootstrap-and-source-tree-manifest.md`
- Modify: `docs/deep_study/37-kernel-type-and-interface-manifest.md`
- Modify: `docs/deep_study/38-m0-m2-pr-batch-and-review-plan.md`
- Create: `docs/deep_study/46-rust-kernel-pivot-and-bootstrap.md`

- [ ] Add the Rust language decision and the Rust/TS/Python split.
- [ ] Freeze the legacy-path-to-Rust-path mapping in `46`.
- [ ] Mark the active implementation docs as Rust-authoritative.

### Task 2: Write The First Failing Rust Bootstrap Test

**Files:**
- Create: `Cargo.toml`
- Create: `tests/conformance/runtime/bootstrap.rs`

- [ ] Add a minimal Rust package manifest for `research-cli`.
- [ ] Write a failing integration test asserting that the no-op app run exits cleanly.
- [ ] Run the targeted test and confirm it fails because the Rust implementation does not exist yet.

### Task 3: Implement The Minimal Rust Bootstrap

**Files:**
- Create: `src/lib.rs`
- Create: `src/bin/research-cli.rs`
- Create: `src/app/mod.rs`
- Create: `src/runtime/mod.rs`
- Delete: `go.mod`
- Delete: `cmd/research-cli/main.go`
- Delete: `internal/app/app.go`
- Delete: `internal/runtime/runtime.go`
- Delete: `tests/conformance/runtime/bootstrap_test.go`

- [ ] Add the Rust library root.
- [ ] Add the Rust binary entrypoint.
- [ ] Add the minimal app boot path.
- [ ] Add the minimal runtime constructor.
- [ ] Remove the stale Go bootstrap files so the repo has one active kernel substrate.

### Task 4: Install Toolchain And Verify

**Files:**
- Modify: `.gitignore`

- [ ] Install Rust using `rustup` with a minimal profile.
- [ ] Verify `rustc --version` and `cargo --version`.
- [ ] Run the targeted bootstrap test and make it pass.
- [ ] Run `cargo test` for the current repo state.
- [ ] Add Rust build output ignores to `.gitignore`.

### Task 5: Prepare The Next Batch Entry

**Files:**
- Modify: `docs/deep_study/31-kernel-foundation-implementation-plan.md`
- Modify: `docs/deep_study/38-m0-m2-pr-batch-and-review-plan.md`

- [ ] Confirm that Batch 1 now starts from the Rust bootstrap floor.
- [ ] Record the next immediate implementation entry as bundle/event floor work.
