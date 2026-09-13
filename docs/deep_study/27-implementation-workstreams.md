# Implementation Workstreams And Ownership Map

This document turns the now-ready architecture into implementation workstreams.

`11-implementation-blueprint.md` defines the module map.

`12-milestone-roadmap.md` defines the milestone order.

This document sits between them and answers a more practical question:

- who owns what
- what gets built together
- what may run in parallel
- what must stay on the critical path

The goal is to prevent the implementation phase from degenerating into
"everyone touches everything."

## 1. Execution Rule

Implementation should follow two principles:

1. one runtime truth first
2. one subsystem owner per workstream

No workstream may invent a second protocol, second event vocabulary, second
session store, or second remote runtime.

## 2. Critical-Path Summary

The true critical path is:

1. runtime + persistence kernel
2. operator contract surfaces
3. permission/tool/provider lanes
4. remote operator plane
5. multi-agent runtime
6. memory + ProjectOps
7. research workflow runtime
8. release conformance

Parallelism is allowed only when the write surface is disjoint and the shared
protocol contracts are already frozen.

## 3. Workstream Inventory

## WS0: Bootstrap And Runtime Skeleton

Purpose:

- create the executable shell that everything else plugs into

Primary ownership:

- `cmd/research-cli/`
- `internal/app/`
- `internal/runtime/`
- `internal/events/`
- `internal/session/`
- `internal/workspace/`

Must implement first:

- process startup
- workspace detection
- project/session resolution
- runtime event writer
- bundle checkpoint load/save
- basic interactive and one-shot entrypoints

Required deliverables:

- `KernelStateBundle` load/save path
- `project_state.json` bundle checkpoint support
- append-only event log bootstrap
- transcript JSONL bootstrap
- `doctor` and `smoke` skeletons

Exit signal:

- app boots
- opens a project-scoped session
- writes deterministic session + event artifacts

## WS1: Operator Surface And TUI

Purpose:

- make the kernel operable like a real code-agent CLI

Primary ownership:

- `internal/tui/`
- `internal/session/`
- `internal/events/`
- `internal/doctor/`
- command parser layer under `cmd/research-cli/`

Builds:

- `prompt`, `chat`, `resume`, `continue`
- `inspect`, `compact`, `doctor`, `usage`, `cost`, `stats`
- `projects *`, `sessions *`
- slash/help/command-palette behavior

Dependencies:

- WS0 complete enough to create/read sessions and emit canonical events

Required deliverables:

- JSON envelope compliance
- exit-code compliance
- session browse/search/export/title behavior
- compact/resume recap path
- TUI panes wired to kernel truth, not shadow state

Exit signal:

- base CLI feels competitive with Claw/Hermes/Crush class workflows
- golden operator fixtures for core command families can run

## WS2: Protocol, Schema, And Conformance Substrate

Purpose:

- turn the contracts into machine-checked protocol artifacts

Primary ownership:

- `schemas/`
- `tests/conformance/`
- protocol helpers under `internal/events/`, `internal/session/`, `internal/agents/`, `internal/remotehost/`

Builds:

- schema files
- schema validation helpers
- fixture loaders
- migration-line compatibility checks
- canonical object envelope helpers

Dependencies:

- WS0 runtime object boundaries frozen

Required deliverables:

- schema set listed in `28-schema-registry-and-conformance-plan.md`
- conformance runner
- additive-field tolerance tests
- checkpoint recovery tests

Exit signal:

- every persisted canonical object validates
- migrations and replay rules are tested before feature expansion

## WS3: Permission, Tool, Provider, And Config Surface

Purpose:

- make the CLI safe and predictable for real coding work

Primary ownership:

- `internal/permissions/`
- `internal/tools/`
- `internal/mcp/`
- provider/config code under `internal/app/` or dedicated provider package if later split

Builds:

- permission mode logic
- request/approve/deny lifecycle
- tool registry and classification
- provider resolution and auth validation
- config precedence and sources view
- MCP registry, inspect, test, refresh

Dependencies:

- WS0 and WS1 must already expose event + JSON contract paths

Required deliverables:

- shell/file/web/MCP baseline
- provider auth-status/test/refresh-catalog
- config get/set/effective/sources
- operator-visible degraded feature state

Exit signal:

- permission and provider fixtures pass
- failures are explainable without log scraping

## WS4: Remote Operator Plane

Purpose:

- add mobile/web/desktop follow and control without splitting runtime truth

Primary ownership:

- `internal/remotehost/transport/`
- `internal/remotehost/projection/`
- `internal/remotehost/actions/`
- `internal/remotehost/capabilities/`
- `internal/remotehost/terminal/`
- `internal/remotehost/workbench/`

Builds:

- pair / status / attach / handoff / takeover / notify
- lease + cursor + binding persistence
- projection snapshots
- remote capability advertisement
- mock host harness

Dependencies:

- WS0 bundle/event truth
- WS2 schema/conformance helpers
- WS1 operator command grammar

Required deliverables:

- `.pmcli/remote/*` persistence
- `SessionRuntimeDescriptor`
- `SessionEnvelopeProjectionV1`
- `ControlLease`
- deterministic remote harness

Exit signal:

- one live session is safely controllable from remote follow/control surfaces
- remote conformance fixtures pass

## WS5: Multi-Agent, Review, Branch, And Debate Runtime

Purpose:

- add bounded multi-agent execution with auditability

Primary ownership:

- `internal/agents/`
- `internal/reviews/`
- `internal/branches/`

Builds:

- agent spawn/list/inspect/stop/traces
- task-packet persistence
- reviewer blinding
- branch scheduler
- evaluation packets
- review packets and traces
- debate packet/trace handling

Dependencies:

- WS2 schema authority
- WS3 permission/tool budget enforcement

Required deliverables:

- `.pmcli/agents/<agent_id>/TASK_PACKET.json`
- `.pmcli/agents/<agent_id>/TRACE.jsonl`
- `.pmcli/reviews/`
- `.pmcli/branches/`
- branch/worktree refresh rules

Exit signal:

- reviewer and executor isolation is real
- promotion cannot happen without evaluation + review gate

## WS6: Memory And ProjectOps

Purpose:

- implement the personalized differentiator without compromising safety

Primary ownership:

- `internal/memory/`
- `internal/projectops/`
- selected support in `internal/artifacts/`

Builds:

- working-memory store
- durable memory store
- retrieval routing and explainability
- invalidation/decay
- summary candidate/promotion pipeline
- cleanup proposal engine
- wake queue and supervised run hooks

Dependencies:

- WS0, WS2, and WS5

Required deliverables:

- `memory query`, `memory explain`, `memory status`, `memory invalidate`
- progress-digest candidate lane
- cleanup-plan / cleanup-apply governance
- project inspection state

Exit signal:

- project memory is useful, bounded, and explain-backed
- repo/project maintenance is proactive but reversible

## WS7: Research Runtime And Skill Orchestration

Purpose:

- express the full research lifecycle using the bounded runtime already built

Primary ownership:

- `internal/research/`
- `internal/reviews/`
- `internal/branches/`
- registry support in `internal/mcp/` or skill package area as needed

Builds:

- `ResearchSkillContract`
- `SkillManifest`
- `StageExecutionMap`
- stage execution runtime
- result-to-claim repair routing
- experiment supervision integration

Dependencies:

- WS5 multi-agent runtime
- WS6 memory and ProjectOps

Required deliverables:

- research DAG runner
- stage-to-stage repair edges
- skill registry compatibility checks
- stage/runtime guardrail enforcement

Exit signal:

- research workflow is runtime-governed, not shell-script stitched

## WS8: Release Conformance And Hardening

Purpose:

- prove the product is actually shippable

Primary ownership:

- `tests/golden/`
- `tests/conformance/`
- `tests/remote_harness/`
- release scripts / CI configs

Builds:

- full fixture population
- PR/merge/nightly CI matrix
- deterministic replay checks
- degraded-state and recovery tests
- release gate script

Dependencies:

- all earlier workstreams at least minimally complete

Required deliverables:

- fixture and conformance dashboards
- release-blocking gate definitions
- failure triage guide

Exit signal:

- implementation status is measurable by fixtures, not by optimism

## 4. Parallelization Rules

The following pairs are safe to run mostly in parallel after contracts freeze:

- WS1 operator surface and WS2 schema/conformance substrate
- WS4 remote plane and WS5 multi-agent runtime
- WS6 memory/ProjectOps and WS7 research runtime

The following must not diverge:

- WS0 vs any other workstream on bundle/event/session truth
- WS2 vs any other workstream on schema ownership
- WS4 vs any other workstream on remote event/control vocabulary

## 5. Recommended Team Or Agent Ownership

If this is executed via multiple agent workers, ownership should be split by
write surface:

- Kernel owner: WS0 + shared protocol glue
- Operator owner: WS1 + command parser/TUI
- Protocol owner: WS2
- Safety/runtime owner: WS3
- Remote owner: WS4
- Multi-agent owner: WS5
- Memory/ProjectOps owner: WS6
- Research runtime owner: WS7
- Verification owner: WS8

The protocol owner has veto rights on schema drift.

The kernel owner has veto rights on bundle/event/session truth drift.

## 6. Exit Rule

Implementation may start immediately, but only if:

- WS0 + WS2 begin first
- every new persisted object is assigned one schema owner before coding
- fixtures are added alongside features instead of after-the-fact

That is how this design keeps its current architectural advantage during the
actual build phase.
