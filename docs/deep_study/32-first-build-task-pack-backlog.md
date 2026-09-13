# First Build Task Pack Backlog

This document packages the first implementation wave into agent-sized task
packs.

Unlike `31-kernel-foundation-implementation-plan.md`, which is milestone and
flow oriented, this file is optimized for dispatching bounded workers.

Proof-gate note:

- `44-base-cli-superiority-proof-gate.md` defines the evidence threshold
- `45-base-cli-proof-execution-matrix.md` defines which packs move which proof
  lines
- packs that advance Claw-class or Hermes-class proof should be prioritized
  over broader reserved-surface scaffolding during M0-M2

Language transition note:

- the active implementation language is now Rust
- the file lists below still describe logical owners first
- canonical Rust path mapping is frozen in
  `46-rust-kernel-pivot-and-bootstrap.md`

## 1. Dispatch Rules

Each task pack must satisfy:

- one primary write surface
- one measurable acceptance target
- one obvious review path

Do not give one worker ownership of both kernel truth and protocol authority
unless the task is too small to split safely.

## 2. Pack A - Kernel Truth

Purpose:

- establish the checkpoint, event, and session floor

Files:

- `cmd/research-cli/main.go`
- `internal/app/app.go`
- `internal/runtime/runtime.go`
- `internal/runtime/checkpoint.go`
- `internal/events/events.go`
- `internal/events/writer.go`
- `internal/session/store.go`
- `internal/session/transcript.go`

Acceptance:

- `project_state.json` writes and reloads
- event log writes canonical envelope
- session create/load/list works

Reviewer checks:

- no duplicate top-level runtime truth
- no second event vocabulary

## 3. Pack B - Workspace And Project Scope

Purpose:

- make project resolution deterministic

Files:

- `internal/workspace/resolve.go`
- `internal/workspace/hash.go`
- project bootstrap helper under `internal/app/`

Acceptance:

- current project is resolved from cwd or explicit scope
- ambiguous scope fails structurally

Reviewer checks:

- workspace hash is stable
- `.pmcli/` bootstrap path is project-scoped

## 4. Pack C - Session UX Floor

Purpose:

- expose basic session operator behavior

Files:

- `cmd/research-cli/main.go`
- `internal/session/compact.go`
- `internal/session/store.go`

Acceptance:

- `prompt`, `chat`, `resume`, `continue`, `inspect`, `compact` baseline works

Reviewer checks:

- `chat` and `prompt` do not diverge into separate runtimes
- compact path preserves raw history and lineage

## 5. Pack D - Doctor, Smoke, Setup

Purpose:

- make startup and environment issues operator-visible early

Files:

- `internal/doctor/doctor.go`
- `internal/doctor/smoke.go`
- setup command layer in `cmd/research-cli/main.go`

Acceptance:

- ready/degraded/blocked states are emitted deterministically

Reviewer checks:

- read-only-safe behavior
- concrete repair hints

## 6. Pack E - Permission And Tool Policy

Purpose:

- make the kernel safe for real code actions

Files:

- `internal/permissions/policy.go`
- `internal/permissions/requests.go`
- `internal/tools/registry.go`
- `internal/tools/shell.go`
- `internal/tools/file.go`
- `internal/tools/web.go`

Acceptance:

- permission requests persist
- mutating/destructive tools are policy-gated

Reviewer checks:

- no out-of-workspace write bypass
- expired permission requests cannot be reused

## 7. Pack F - Provider And Config

Purpose:

- make model/provider/config behavior explainable

Files:

- `internal/providers/resolve.go`
- `internal/providers/auth.go`
- `internal/config/config.go`
- `internal/config/sources.go`

Acceptance:

- config precedence is deterministic
- provider auth and test surfaces explain conflicts

Reviewer checks:

- explicit provider beats ambient credentials
- auth-shape errors are specific

## 8. Pack G - Schema And Conformance

Purpose:

- turn protocol docs into enforceable machine contracts

Files:

- `schemas/kernel_state_bundle.schema.json`
- `schemas/project_state.schema.json`
- `schemas/event.schema.json`
- `schemas/session.schema.json`
- `schemas/task_packet.schema.json`
- `tests/conformance/schemas/`
- `tests/conformance/runtime/`

Acceptance:

- valid/invalid payload tests pass
- checkpoint round-trip test passes

Reviewer checks:

- schema owners match `28-schema-registry-and-conformance-plan.md`
- no duplicated canonical schema authority in docs/code

## 9. Pack H - Golden Fixture Harness

Purpose:

- make operator claims measurable from the first build wave

Files:

- `tests/golden/config/`
- `tests/golden/provider/`
- `tests/golden/sessions/`
- `tests/golden/permissions/`
- fixture runner scripts

Acceptance:

- first curated golden set passes in CI

Reviewer checks:

- fixtures assert canonical event stems, not prose aliases
- JSON envelope output is validated, not hand-inspected

## 10. Suggested Dispatch Order

Recommended order:

1. Pack A
2. Pack G
3. Pack B and Pack D in parallel
4. Pack C and Pack E in parallel
5. Pack F
6. Pack H

This order protects the true critical path while still allowing safe
parallelism.

## 11. Merge Gate

No pack should merge unless:

- its direct tests pass
- its schema/fixture ownership is still aligned with docs
- it does not introduce a second truth source

That is the simplest operational rule for preserving the architecture during
implementation.
