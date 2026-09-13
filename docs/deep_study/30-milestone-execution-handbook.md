# Milestone Execution Handbook

This document is the implementation handoff for the now-ready architecture.

`12-milestone-roadmap.md` tells us what order the milestones should happen in.

This handbook tells the implementation team how to execute them without losing
the contract discipline we just established.

## 1. Global Rules

Before any milestone starts:

- confirm the upstream contract docs are frozen enough for that milestone
- assign a single workstream owner
- create schema files before ad hoc JSON
- create fixture stubs before feature sprawl

During a milestone:

- no feature-specific private protocol
- no second event vocabulary
- no second runtime or second remote truth
- no "we will add tests later" on persisted objects
- new milestone, policy, review, handoff, and skill-generated documents should
  expose or refresh a schema-valid `DocFrame` once the DocFrame slice is live

After a milestone:

- update docs, schemas, fixtures, and CI together
- update affected `DocFrame` blocks and rebuild the document index when the
  milestone meaning, scope, lifecycle, or evidence refs changed
- do not mark complete until exit criteria are objectively met

## 2. Milestone-by-Milestone Execution

## M0: Bootstrap, Protocols, Guardrails

Start with:

- repository skeleton
- schema harness
- bundle/event/session core protocol helpers
- doctor/smoke bootstrap

Do not start:

- remote plane
- multi-agent runtime
- memory/projectops

Completion proof:

- app boots
- bundle checkpoint validates
- event log validates
- schema harness is live in CI

## M1: Kernel Session Runtime

Build in this exact order:

1. project resolution
2. session create/load/list/delete
3. transcript persistence
4. one-shot turn execution
5. interactive loop
6. continue/resume
7. compaction baseline

Completion proof:

- continue/resume works project-scoped
- compaction and recap are deterministic
- interruption + resume path is test-covered

## M2: Permission, Tools, Providers, Config

Build in this order:

1. permission state machine
2. tool registry classification
3. shell/file/web adapters
4. provider resolution
5. auth-status / provider test
6. config precedence and sources
7. MCP baseline

Completion proof:

- out-of-scope writes are blocked or explained
- provider/auth ambiguity is operator-visible
- config precedence fixtures pass

## M3: Repo Governance And Review

Build in this order:

1. git-native canonical surface audit
2. artifact families
2. cleanup-plan engine
3. review packet + trace persistence
4. cleanup-apply review gate
5. summary candidate/promotion queue

Completion proof:

- canonicality audit exposes exactly one active public doc/code surface
- promotion requires an explicit canonical surface and a clean canonicality
  audit
- all mutable outputs land in governed families
- review trace exists for every non-cancelled review
- cleanup is reversible or pre-snapshotted

## M4: Base CLI Parity

Build in this order:

1. inspect + project/session visibility
2. doctor/setup surfaces
3. stats/usage/cost
4. title/lineage/recap semantics
5. slash/help/command palette
6. MCP/skills/setup registry surfaces
7. golden base CLI fixtures

Completion proof:

- day-to-day workflow is competitive before custom layers are added
- all shipped base commands have fixture coverage
- project inspection reports M3/M4 repo-governance surfaces as available or
  guarded, not ambiguous partial states
- terminal CLI parity is the active product priority: every public command in
  the canonical help registry must have stable text output, stable JSON output
  where automation depends on it, typed failures, and conformance/operator
  coverage before mobile full-CLI parity is attempted

## M5: Remote Operator Plane

Build in this order:

1. remote lease/binding persistence
2. pair/status
3. attach
4. handoff/takeover
5. projection snapshots
6. notify
7. Tailscale-only daemon PWA
8. workbench, session list, message steering, and permission response
9. replay-ticket, revocation, lease-expiry, and schema harness coverage
10. daemon control token and canonical event-stream projection

Completion proof:

- remote control is projection-driven
- no second runtime emerges
- public relay and native packaging are explicitly outside the M5 claim
- a later native app may still use Tailscale and the same daemon API; it is
  deferred as a product surface, not rejected as an architecture path
- workbench, message, permission, replay-ticket, lease-expiry, and revocation
  cases pass under conformance/operator coverage
- `/api/events` exposes canonical `.pmcli/events/events.jsonl` updates after a
  cursor without making the PWA an event authority
- daemon token enforcement is covered at both handler and HTTP daemon level

Explicit non-goal:

- M5 does not claim that the phone PWA exposes every terminal CLI command. The
  PWA is a remote operator/control surface over selected high-value workflows.
  Full mobile CLI parity is deferred until after the terminal CLI itself is
  complete, stable, and covered as the canonical product surface.

## Future: Mobile Full CLI Parity

This work starts only after the complete terminal CLI is graduated.

Build in this order:

1. freeze the complete terminal command registry as the authoritative public
   product surface
2. ensure every terminal command has stable operator behavior, typed failures,
   and coverage
3. add a daemon command-execution API that invokes the same Rust runtime instead
   of reimplementing command behavior for mobile
4. expose a mobile command palette generated from the canonical command
   registry
5. add GUI panels for high-frequency command families where touch interaction is
   better than typing long command lines
6. prove parity with conformance tests that compare mobile command entries
   against `research-cli help --json`

Completion proof:

- every externally visible terminal CLI command has a mobile entry
- mobile execution reuses the same kernel/runtime path as the terminal CLI
- mobile GUI helpers improve command ergonomics without creating a second
  runtime, second state store, or second command contract

## M6: Multi-Agent Core

Preflight gate:

- review `51-m6-m10-reference-superiority-design-audit.md`
- freeze the M6 schema and fixture gates before adding broad delegation UX
- treat `TaskPacket` as the durable authority for run intent, retention,
  run class, IO mode, replay/resume policy, budget, scope, write authority,
  success criteria, and output-manifest obligation
- keep Happier-style execution-run lifecycle lessons, Happy-style
  sequence/concurrency lessons, and claw-code-style mock parity lessons
  explicit in the M6 implementation plan

Build in this order:

1. agent directories and task packet persistence
2. agent runtime records
3. reviewer blinding
4. review open/retry surfaces
5. branch scheduler skeleton
6. agent stop/traces

Completion proof:

- reviewer and executor are isolated
- task packet governs every launched agent
- operator can inspect and stop agents safely
- deterministic mock-agent lifecycle fixtures pass without live providers
- reviewer blinding is mechanically enforced by a packet builder, not by prompt
  convention

## M7: Working Memory And Silent Consolidation

Build in this order:

1. working-memory append store
2. bounded eviction
3. digest candidate generation
4. promotion queue
5. session-end silent summary

Completion proof:

- working memory stays bounded through schema-valid `memory append/status`
  with deterministic unpinned eviction and pin protection
- digest candidates are staged in `.pmcli/memory/promotion_queue/` with
  support refs and are not silently promoted to durable memory
- compaction-triggered silent summaries publish traceable `ProjectOpsTick`
  records and persist without transcript replay

## M8: Durable Project Memory And Active Query

Build in this order:

1. durable `MemoryRecord`
2. promotion/demotion
3. invalidation and decay
4. retrieval budgets
5. deterministic hybrid retrieval: exact identifier/source-artifact,
   provenance, support-ref, lexical, semantic-lite, vector-embedding,
   temporal-graph, and temporal-trust lanes fused by RRF
6. query/explain/invalidate surfaces
7. project inspection integration

Completion proof:

- memory injection is explainable
- identifier/source-backed durable records can outrank noisy lexical-only hits
- semantic-alias queries can recover vector-backed records, and superseded
  records can route to current successors through temporal graph edges
- stale/superseded memory stops auto-injecting
- cleanup/rollback restore invalidates dependent trusted memory before later
  query injection

## M9: Evolutionary Branch Intelligence

M9 is no longer only a branch-search scheduler. It is a Git-native
evolutionary multi-agent optimizer over executable project hypotheses.

Build in this order:

1. `SearchBatch` and `BranchRun` population records
2. variation operators: mutate, repair, crossover, simplify, test expansion,
   refresh, and canonicalization
3. evaluation packet with tests, schema/conformance, canonicality, stale-base,
   risk, metric, and reviewer-gate signals
4. stale-base / refresh handling
5. debate trace and red-team comparison
6. director promotion decision with gated automatic winner merge
7. archive and loser cleanup
8. lineage and operator-credit ledger for future memory/skill learning

Completion proof:

- branch intelligence is budgeted, auditable, and lineage-backed
- every candidate is an executable hypothesis tied to a Git base, a detached
  Git worktree, hidden `.pmcli/branches/` state, and a candidate-local artifact
- no direct promotion without evaluation + debate/review + canonicality gates
- at most one winner per batch can reach the public canonical project surface

Current graduated boundary: M9 proves the local governance loop plus LLM/agent
mutation through a command adapter and default automatic winner merge. The merge
excludes hidden candidate artifacts and is rolled back if post-merge
canonicality fails.

## M10: Research Runtime And Skill Pack

Build in this order:

1. `SkillManifest`
2. `ResearchSkillContract`
3. `StageExecutionMap`
4. stage execution runtime
5. result-to-claim repair routing
6. supervised experiment integration

Completion proof:

- research automation is native runtime behavior, not external glue

## 3. Forbidden Shortcuts

The following shortcuts are specifically forbidden:

- implementing remote UX before lease/binding/cursor persistence exists
- implementing branch promotion before evaluation/review gates exist
- implementing silent memory promotion before candidate/promotion separation exists
- implementing research stages before `StageExecutionMap` exists
- implementing fancy commands before their protocol/state machine exists

These were the exact failure patterns caught during external review.

## 4. Recommended First Three Coding Sprints

## Sprint 1

- WS0 core bootstrap
- schema runner
- bundle/event/session persistence

## Sprint 2

- continue/resume/compact/inspect
- doctor/setup
- config/provider/auth surface

## Sprint 3

- permission/tool classification
- golden base fixture runner
- artifact family and review packet baseline

If Sprint 3 is not stable, do not start remote or multi-agent.

## 5. Documentation Update Rule

When a milestone lands:

- update the corresponding schema files
- update the milestone doc if sequencing changed
- update or generate `DocFrame` blocks for changed milestone, plan, review,
  handoff, and skill-output documents; after the general skill-output runtime
  lands, skill-output documents must pass through `SkillOutputEnvelope` and the
  publication gate before becoming public-latest
- rebuild `.pmcli/docs/index.json` and inspect stale or missing frames once the
  DocFrame tooling is graduated
- update the deep-study README status
- add fixture references

This keeps the docs as living implementation controls rather than stale design
artifacts.

## 6. Ready-to-Code Definition

The architecture is now ready to implement, but the implementation is only
"ready to proceed" milestone by milestone if:

- the prior milestone exit criteria are truly met
- the next milestone schemas exist
- the next milestone fixture shells exist

That is how this project preserves the strength of its current design during
the actual build.
