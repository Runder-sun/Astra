# Advanced Build Task Pack Backlog

This document is the advanced-systems companion to
`32-first-build-task-pack-backlog.md`.

It groups M3-M10 implementation into dispatchable packs for agentic workers or
small engineering subteams.

## 1. Dispatch Rule

These packs should be started only after the kernel foundation packs are
passing.

Never mix remote control, memory authority, and branch promotion logic inside
one worker unless the task is purely glue code.

## 2. Pack I - Artifact And Review Governance

Purpose:

- make repo outputs and review traces governed before advanced generation ramps up

Files:

- `internal/artifacts/*`
- `internal/reviews/store.go`
- `internal/reviews/trace.go`
- `schemas/artifact_family.schema.json`
- `schemas/review_packet.schema.json`
- `schemas/review_trace.schema.json`

Acceptance:

- artifact families resolve canonical/latest/archive
- review packet + trace persistence exists

## 3. Pack J - Remote Persistence Core

Purpose:

- implement remote lease/binding/control state safely

Files:

- `internal/remotehost/transport/*`
- `schemas/remote_lease.schema.json`
- `schemas/remote_binding.schema.json`
- `schemas/control_lease.schema.json`
- `schemas/remote_cursor.schema.json`

Acceptance:

- remote control survives restart and replay checks

## 4. Pack K - Remote Command Surface

Purpose:

- expose pair/status/attach/handoff/takeover/notify

Files:

- `internal/remotehost/actions/*`
- `internal/remotehost/capabilities/*`
- `internal/remotehost/workbench/*`
- CLI remote command layer
- `tests/golden/remote/*`

Acceptance:

- remote command fixtures pass

## 5. Pack L - Agent Runtime

Purpose:

- add inspectable packet-bound agents

Files:

- `internal/agents/*`
- `schemas/task_packet.schema.json`
- `schemas/agent_trace.schema.json`
- `schemas/agent_output_manifest.schema.json`

Acceptance:

- every launched agent has packet + trace + output manifest

## 6. Pack M - Reviewer Isolation

Purpose:

- make independent review non-optional

Files:

- `internal/reviews/blinding.go`
- review packet builders
- agent/review glue code

Acceptance:

- reviewer packets exclude executor interpretation by default

## 7. Pack N - Evolutionary Branch Intelligence

Purpose:

- implement budgeted executable hypothesis optimization instead of swarm chaos
- use Git-native branch/worktree lineage, variation operators, evaluation,
  debate, and director-only promotion

Files:

- `internal/branches/scheduler.go`
- `internal/branches/evaluation.go`
- `schemas/branch_batch.schema.json`
- `schemas/branch_run.schema.json`
- `schemas/evaluation_packet.schema.json`

Acceptance:

- branches cannot promote without complete evaluation and debate/review gate
- only one winner per search batch can become the canonical project surface
- loser branches archive without exposing stale public docs/code
- implemented proof surface uses real detached Git worktrees, command-adapter
  LLM/agent mutation, real eval commands, canonicality gates, strict schemas,
  and gated automatic winner merge with rollback

## 8. Pack O - Working Memory And Digests

Purpose:

- add bounded session/project context retention

Files:

- `internal/memory/working.go`
- `internal/memory/eviction.go`
- `internal/projectops/digest.go`

Acceptance:

- eviction, pinning, and digest-candidate flow work

## 9. Pack P - Durable Memory And Explain

Purpose:

- make project memory queryable and explainable

Files:

- `internal/memory/durable.go`
- `internal/memory/retrieval.go`
- `internal/memory/invalidation.go`
- memory command layer

Acceptance:

- `memory query/explain/status/invalidate` is operator-grade

## 10. Pack Q - ProjectOps Governance

Purpose:

- implement cleanup proposals, supervision, and project inspection

Files:

- `internal/projectops/projectops.go`
- `internal/projectops/cleanup.go`
- `internal/projectops/supervisor.go`
- project inspection command layer

Acceptance:

- cleanup proposals and project inspection are live

## 11. Pack R - Debate And Research Runtime

Purpose:

- turn the research skill pack into native runtime behavior

Files:

- `internal/branches/debate.go`
- `internal/research/*`
- research-related schemas

Acceptance:

- stage execution routing and repair edges are implemented

## 12. Recommended Parallel Wave Plan

Wave 1:

- Pack I
- Pack J
- Pack L

Wave 2:

- Pack K
- Pack M
- Pack N

Wave 3:

- Pack O
- Pack P
- Pack Q

Wave 4:

- Pack R
- full release conformance and hardening

This sequencing lets the protocol substrate mature before the most complex
autonomous behavior lands.
