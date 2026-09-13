# Fusion Patterns for a Native Project-Memory Multi-Agent CLI

This document fuses the best design patterns from Layer A (memory core),
Layer B (project/repo memory), and Layer C (multi-agent research
orchestration) into a concrete architecture for the target system:

- a native code agent CLI
- with project-level active memory
- with native multi-agent execution and debate
- with proactive silent summarization and memory writeback
- with repository anti-chaos governance
- with a full research skill pack spanning survey -> idea -> plan -> code ->
  experiment -> repair -> rerun

The goal here is not "copy one repo." The goal is to combine the strongest
mechanisms from the studied systems into a tighter, more operational design.

## Status

- First fusion pass complete

## What The Three Layers Actually Proved

### Layer A proved memory is not one store

From `MemPalace`, `ClawMem`, `AgentRecall`, `memoryOSS`, and `Hippo`, the main
lesson is that usable memory must split into:

- hot-path wake-up memory
- on-demand retrieval memory
- durable long-term memory
- consolidation / compaction memory
- provenance-aware and status-aware memory

This means our target CLI must not treat memory as "just vector recall."

### Layer B proved project memory is not the same as personal memory

From `code-review-graph`, `DiffMem`, `Ars Contexta`, `Sekha`, and `ARIS`, the
main lesson is that project memory needs at least four repo-facing surfaces:

- structural code memory
- repository evolution memory
- operational project-state memory
- governance / enforcement memory

This means a memory-palace-only system will still fail on real repositories.

### Layer C proved multi-agent success depends on control surfaces

From `multi-agent-ralph-loop`, `Hermes`, `Claw Code`, `OpenClaw`, `ARIS`,
`AutoResearchClaw`, `OpenAGS`, `autoresearch`, and `AutoSOTA`, the main lesson
is that multi-agent systems work only when they have:

- an execution substrate
- a task / state protocol
- an independent critique protocol
- a long-horizon improvement loop

This means "spawn many agents" is not the design. The design is the set of
runtime and artifact contracts that make many agents safe and cumulative.

## The Core Fusion Decision

The target system should be designed as a native project operating system for
agentic coding and research, not as:

- a single chatbot with memory
- a workflow-only markdown kit
- a pure graph index
- a pure research pipeline
- a pure agent tree manager

The fused system should combine:

- `Hermes` for provider/engine extensibility
- `Claw Code` for session/worktree isolation and recovery
- `OpenAGS` for explicit folder/file protocol between agents
- `ARIS` for research artifact discipline and reviewer independence
- `ClawMem` for hot-path memory gating and injection
- `MemPalace` for layered wake-up vs on-demand recall
- `DiffMem` for git-native temporal retrieval
- `code-review-graph` for structural repo orientation
- `Sekha` for hook-level anti-chaos enforcement
- `AutoResearchClaw` for long research loops and lesson evolution
- `autoresearch` for local search / branch search / bounded experiment loops

## Target Architecture Overview

The best fused architecture is a seven-plane system:

1. Runtime Plane
2. Session and Recovery Plane
3. Memory Plane
4. Repo Intelligence Plane
5. Multi-Agent Orchestration Plane
6. Governance Plane
7. Research Workflow Plane

These planes should be implemented natively, but persisted through explicit
files so the whole system remains inspectable and recoverable.

## Plane 1 - Runtime Plane

### What to keep

From `Hermes`:

- `MemoryManager` style single integration point
- built-in memory always on
- at most one external memory backend active at a time
- separate memory provider from context engine

From `OpenClaw`:

- explicit background task model
- task registry, delivery semantics, and session isolation

From `Claw Code`:

- worktree-aware runtime
- forkable sessions
- recovery as a first-class subsystem

### Fused design

The CLI runtime should expose these native subsystems:

- `runtime.session_manager`
- `runtime.task_registry`
- `runtime.context_engine`
- `runtime.memory_manager`
- `runtime.repo_governor`
- `runtime.agent_dispatcher`
- `runtime.recovery_manager`

The key rule is:

- memory plugins do not directly own the runtime
- orchestration plugins do not directly own memory
- research workflows do not directly own session recovery

Everything routes through a shared runtime kernel.

## Plane 2 - Session and Recovery Plane

### Why this matters

Long-running coding and research work will fail without:

- session resume
- session fork
- worktree isolation
- crash recovery
- pre-compaction extraction

This is where `Claw Code` is especially strong.

### Fused design

Each repo should have a native agent state root, for example:

```text
.pmcli/
  sessions/
  tasks/
  agents/
  memory/
  repo/
  governance/
  research/
  traces/
  reports/
```

Each session record should include:

- `session_id`
- `workspace_root`
- `worktree_id`
- `branch`
- `task_scope`
- `parent_session_id`
- `summary`
- `recovery_state`
- `active_agents`
- `last_successful_checkpoint`

### Required recovery triggers

The runtime should force writeback on:

- pre-compaction
- session end
- branch switch
- task completion
- agent completion
- experiment completion
- debate completion

This is directly inspired by `MemPalace` precompact hooks and `Claw Code`
recovery discipline.

### Compression rule

Do not summarize blindly.

Follow `Claw Code`'s main lesson:

- preserve control-state fields first
- only compress narrative detail second

That means summaries must explicitly preserve fields such as:

- scope
- current work
- pending work
- key files
- active branches/worktrees
- open risks
- next actions

## Plane 3 - Memory Plane

### The fused memory model

The target CLI should have six memory surfaces.

#### M0 - Identity and Operating Contract

Inspired by `MemPalace L0`, `AgentRecall`, and `Ars Contexta self/`.

Contents:

- agent identity
- user preferences
- global style rules
- stable mission
- execution constraints

Load behavior:

- always loaded
- tiny and stable

#### M1 - Active Session Memory

Inspired by `ClawMem` hot-path retrieval and `Ars Contexta ops/`.

Contents:

- current task
- recent session events
- recently surfaced files
- active debate branches
- currently running experiments

Load behavior:

- always available to the context engine
- aggressively decayed and compacted

#### M2 - Durable Project Memory

Inspired by `Ars Contexta notes/`, `ARIS` project files, and `AgentRecall`
digest/awareness.

Contents:

- project decisions
- architecture notes
- accepted plans
- failure lessons
- known constraints
- recurring pitfalls
- stable TODO lineage

Load behavior:

- loaded by topic and task
- not fully auto-injected

#### M3 - Repo Structural Memory

Inspired by `code-review-graph`.

Contents:

- module map
- symbol graph
- critical flows
- ownership/community clusters
- architectural wiki

Load behavior:

- used for orientation and task scoping
- favored at task start and after repo changes

#### M4 - Repo Evolution Memory

Inspired by `DiffMem`.

Contents:

- branch history
- important diffs
- superseded decisions
- change provenance
- when/why behaviors changed

Load behavior:

- loaded when a task involves regression, blame, reversion, duplication, or
  "what changed" questions

#### M5 - Research Memory

Inspired by `ARIS research-wiki`, `AutoResearchClaw`, and `AutoSOTA`.

Contents:

- papers
- ideas
- experiments
- claims
- reviews
- branch comparisons
- quality reports
- result ledgers

Load behavior:

- loaded by research phase
- summarized into phase-specific packs

#### M6 - Governance Memory

Inspired by `Sekha`.

Contents:

- dangerous actions policy
- repo cleanliness rules
- file naming conventions
- promotion/archive rules
- duplicate suppression rules
- allowed experimental scratch zones

Load behavior:

- loaded mainly by hooks and tool interceptors
- not a big prompt block

## Layered Recall Over The Memory Plane

The runtime should apply a `MemPalace` style access model over all six memory
surfaces:

- `L0`: identity / mission / safety
- `L1`: essential project state
- `L2`: targeted project memory by domain
- `L3`: deep semantic or structural search
- `L4`: temporal diff/history reconstruction

This extends the original `L0-L3` pattern into repo-native time-aware recall.

## Memory Writeback Pipeline

### Silent writeback principle

The system should silently summarize and store project progress without
interrupting the user unless:

- the system is uncertain about a merge/supersession decision
- the system detects conflicting canonical files
- the system needs human confirmation for destructive cleanup

### Writeback events

Every important runtime event should map to a memory write:

- user approves plan -> decision memory
- agent finishes task -> session summary + task ledger
- branch experiment finishes -> experiment record + branch comparison
- review finishes -> critique record + claim status update
- file promoted/replaced -> supersession edge
- compaction begins -> compressed checkpoint

### Writeback artifacts

Recommended file layout:

```text
.pmcli/memory/
  self/
  ops/
  project/
  repo/
  research/
  governance/
  digests/
  ledgers/
```

Recommended durable entities:

- `decision:<id>`
- `lesson:<id>`
- `experiment:<id>`
- `claim:<id>`
- `review:<id>`
- `branch:<id>`
- `artifact:<id>`
- `risk:<id>`

Every entity should support:

- provenance
- timestamp
- source session
- source files
- supersedes / superseded_by
- contradicts
- confidence
- current status

This is the combined lesson from `MemPalace`, `memoryOSS`, `DiffMem`, and
`ARIS`.

## Plane 4 - Repo Intelligence Plane

### Why this needs to be native

The target system is a code agent CLI. So repository understanding cannot be
only document-based.

It needs a native repo intelligence stack:

- graph view
- diff view
- branch/worktree view
- artifact manifest view
- cleanup/governance view

### Structural model

Borrow from `code-review-graph`:

- build a symbol/file/community graph
- generate minimal context packets
- generate a durable architecture wiki

The system should maintain:

- code graph snapshot
- critical path summary
- changed-area risk summary
- module-to-memory links

### Temporal model

Borrow from `DiffMem`:

- retrieve via git-native command plans
- answer temporal questions using diffs, blame, log, and show
- store structured pointers, not giant payloads

This should power:

- regression diagnosis
- stale decision detection
- duplicate implementation detection
- "what replaced what" queries

## Plane 5 - Multi-Agent Orchestration Plane

### The key fusion

The best combination is:

- `Hermes` / `Claw` / `OpenClaw` for runtime substrate
- `OpenAGS` for explicit filesystem protocol
- `ARIS` for critique independence
- `Ralph` for quality-gated parallel execution

### Agent topology

Do not build a deep manager tree by default.

Use a shallow topology:

- one coordinator
- many role agents
- optional reviewer/critic agents that stay independent
- optional branch explorers for search/debate

Recommended native roles:

- `coordinator`
- `planner`
- `coder`
- `reviewer`
- `critic`
- `repo_librarian`
- `repo_governor`
- `researcher`
- `experimenter`
- `synthesizer`

### Agent workspace contract

Borrow from `OpenAGS`:

- each agent gets a folder
- each agent reads `DIRECTIVE.md`
- each agent writes `STATUS.md`
- each agent can maintain `memory.md`

Recommended layout:

```text
.pmcli/agents/
  coordinator/
    DIRECTIVE.md
    STATUS.md
    memory.md
  coder/
    DIRECTIVE.md
    STATUS.md
    memory.md
  reviewer/
    DIRECTIVE.md
    STATUS.md
    memory.md
```

### Why file protocol beats pure chat state

Because it gives:

- inspectability
- resumability
- crash recovery
- cross-model compatibility
- auditability

But it must be paired with a stronger runtime than pure markdown protocol.
That is why the `OpenAGS` protocol should sit on top of a `Hermes`/`Claw`
style kernel, not replace it.

## Debate, Search, And Alternative Branch Exploration

### Debate mode

For hard design/research questions, use a fixed artifact protocol:

1. proposer writes candidate plan
2. critic writes strongest objections
3. reviewer evaluates evidence/support gaps
4. synthesizer merges or rejects
5. decision ledger records chosen path and rejected alternatives

Borrow from `ARIS reviewer independence`:

- do not give reviewers the executor's self-serving summary
- preserve raw traces
- keep reviewer and executor independent when possible

### Grid-search / branch-search mode

Borrow from `autoresearch` and `AutoResearchClaw`:

- branch alternative implementations or experiment plans
- assign one worker per branch/worktree
- run bounded experiments
- keep only winning variants
- record losers to avoid repetition

This is the right native form for:

- prompt search
- implementation strategy search
- parameter/config sweeps
- evaluation protocol search
- experiment repair loops

### Required search artifacts

Each search batch should produce:

- candidate manifest
- branch-to-worktree mapping
- evaluation contract
- result ledger
- keep/drop decisions
- final synthesis note

## Plane 6 - Governance Plane

### Why this is non-negotiable

The user's target explicitly needs repository anti-chaos behavior:

- no silent proliferation of duplicate files
- no multi-agent stepping on each other
- no stale draft confusion
- no untracked version sprawl

This is where `Sekha`, `Claw Code`, and `ARIS` matter most.

### Governance hooks

The CLI should intercept tool/file actions and enforce:

- branch ownership
- allowed write zones
- artifact naming conventions
- promotion/archive policy
- duplicate candidate detection
- destructive action confirmation

### Canonical artifact policy

For every semantic artifact type, there should be one canonical location.

Examples:

- current architecture note -> one canonical path
- current experiment plan -> one canonical path
- current best run summary -> one canonical path
- current claim ledger -> one canonical path

Old versions are not deleted immediately. They are:

- archived
- marked superseded
- linked from the canonical file

### Anti-duplicate protocol

Whenever an agent creates a new artifact, the runtime should ask:

- does a canonical artifact of this type already exist?
- is this a replacement, a branch variant, or scratch?
- should this go to `drafts/`, `branches/`, `archive/`, or canonical path?

This prevents the classic agent failure mode of writing:

- `plan.md`
- `plan_v2.md`
- `new_plan.md`
- `final_plan.md`
- `final_plan_revised.md`

### Recommended repo management directories

```text
.pmcli/repo/
  manifests/
  canonical/
  archive/
  branches/
  diffs/
  graph/
  cleanup/
```

### Supersession as a first-class relation

Do not merely overwrite files.

Track:

- `supersedes`
- `superseded_by`
- `derived_from`
- `invalidated_by`

This is the clearest fusion of `DiffMem`, `memoryOSS`, and `ARIS`.

## Plane 7 - Research Workflow Plane

### The research skill pack should be stage-native

The system should not offer a pile of isolated skills only. It should offer a
stage-native research pack where every stage has:

- input contract
- multi-agent execution pattern
- output contract
- memory writeback contract
- possible repair loop

### Recommended native research stages

#### Stage R1 - Landscape and Memory Bootstrap

Goals:

- survey relevant papers/repos
- build field map
- ingest known project context

Outputs:

- landscape summary
- repo/reference ledger
- research memory bootstrap

Multi-agent pattern:

- researcher + librarian + critic

#### Stage R2 - Idea Generation and Debate

Goals:

- generate candidate ideas
- critique novelty and feasibility
- record rejected ideas and why

Outputs:

- idea ledger
- debate traces
- ranked shortlist

Multi-agent pattern:

- proposer + skeptic + synthesizer + novelty reviewer

#### Stage R3 - Method Refinement

Goals:

- turn one idea into an elegant, buildable method
- identify assumptions, failure modes, evaluation targets

Outputs:

- method note
- design assumptions
- risk register

Multi-agent pattern:

- planner + critic + implementation reviewer

#### Stage R4 - Experiment Plan

Goals:

- turn method into a claim-driven evaluation plan

Outputs:

- experiment matrix
- ablations
- compute budget
- run order

Multi-agent pattern:

- planner + experimenter + claim auditor

#### Stage R5 - Implementation

Goals:

- build code in isolated worktrees
- enforce repo cleanliness

Outputs:

- code changes
- implementation notes
- file manifest updates

Multi-agent pattern:

- coder + reviewer + repo governor

#### Stage R6 - Experiment Execution and Search

Goals:

- run experiments
- search over repair/config branches where useful

Outputs:

- run ledgers
- result tables
- keep/drop decisions

Multi-agent pattern:

- experimenter + search workers + result auditor

#### Stage R7 - Result-to-Claim

Goals:

- decide what the data really supports
- invalidate overclaims

Outputs:

- claim ledger
- support / partial / invalid verdicts

Multi-agent pattern:

- result auditor + skeptic + synthesizer

#### Stage R8 - Repair Loop

Goals:

- patch method/docs/code based on evidence
- rerun only the minimal needed checks

Outputs:

- repair plan
- repaired artifacts
- updated lessons

Multi-agent pattern:

- fixer + reviewer + experimenter

#### Stage R9 - Writing and Packaging

Goals:

- produce docs/paper/report/README/slides without losing provenance

Outputs:

- paper/report
- figure manifests
- traceable citations to experiments and claims

Multi-agent pattern:

- writer + reviewer + evidence auditor

## Native Hot-Path Retrieval Strategy

The memory system should not dump everything into prompt context.

Use a `ClawMem` style gated pipeline:

1. classify the prompt
2. skip retrieval for trivial commands/chatter
3. load `M0 + M1` always
4. route to the relevant memory surfaces
5. return compact structured context packet
6. preserve provenance and status

### Retrieval router

Prompt classes should include:

- trivial command
- coding task
- repo diagnosis
- design discussion
- research ideation
- experiment monitoring
- review / critique
- repo cleanup / governance

Each class maps to preferred memory surfaces.

Example routing:

- coding task -> `M1 + M2 + M3`
- regression/debug -> `M1 + M3 + M4 + M6`
- idea generation -> `M0 + M2 + M5`
- repo cleanup -> `M2 + M4 + M6`
- experiment follow-up -> `M1 + M5 + ledger summaries`

## The Native On-Disk Project Model

The most promising fused directory design is:

```text
.pmcli/
  sessions/
  tasks/
  agents/
  memory/
    self/
    ops/
    project/
    repo/
    research/
    governance/
    digests/
    ledgers/
  repo/
    graph/
    diffs/
    manifests/
    canonical/
    archive/
    cleanup/
  research/
    papers/
    ideas/
    experiments/
    claims/
    reviews/
    debates/
    reports/
  traces/
  hooks/
  configs/
```

This is deliberately a fusion of:

- `Ars Contexta` three-space separation
- `ARIS` research-wiki and project file discipline
- `OpenAGS` agent folders
- `Claw Code` session persistence
- `Sekha` governance hooks

## What Should Be Copied Almost Directly

### Copy closely

- `Hermes` provider/engine split
- `ClawMem` retrieval gating
- `Claw Code` worktree-aware session namespace
- `Claw Code` summary compression priorities
- `OpenAGS` `DIRECTIVE.md` / `STATUS.md` protocol
- `ARIS` reviewer independence discipline
- `ARIS` typed research wiki entities and query pack
- `DiffMem` git-native retrieval pointers
- `Sekha` hook-level rule enforcement

### Adapt heavily

- `MemPalace` room/wing metaphor -> keep layered recall, replace ontology with
  repo/project/research-native entities
- `Ralph` mandatory decomposition -> keep quality gates, avoid over-rigidity
- `AutoResearchClaw` large 23-stage pipeline -> keep stage memory/evolution,
  simplify default runtime
- `autoresearch` overnight autonomy -> keep bounded experiment loop, not the
  entire one-domain assumption

### Do not copy literally

- any system that assumes one giant prompt can carry the project state
- any system that stores only semantic chunks without provenance
- any system that treats review as executor self-summary plus approval
- any system that lets agents write arbitrarily into the repo without
  governance checks

## Initial Implementation Order

To make this design operational quickly, implement in five phases.

### Phase 1 - Runtime Kernel + Session Recovery

Build first:

- session manager
- task registry
- worktree-aware session storage
- pre-compaction checkpoint hooks
- structured summary compression

Main inspirations:

- `Claw Code`
- `Hermes`

### Phase 2 - Memory Core + Repo Intelligence

Build next:

- layered `M0-M6` memory model
- retrieval gating
- structural repo graph
- diff/history retriever
- digest cache

Main inspirations:

- `ClawMem`
- `MemPalace`
- `code-review-graph`
- `DiffMem`

### Phase 3 - Governance And Anti-Chaos

Build next:

- hook-level rule engine
- canonical artifact manifest
- supersession tracker
- duplicate-file prevention
- branch ownership rules

Main inspirations:

- `Sekha`
- `ARIS`
- `DiffMem`

### Phase 4 - Multi-Agent Protocol

Build next:

- agent directories
- `DIRECTIVE.md` / `STATUS.md` contract
- session resume per agent
- debate/search orchestration
- reviewer independence

Main inspirations:

- `OpenAGS`
- `ARIS`
- `Hermes`
- `Ralph`

### Phase 5 - Research Workflow Pack

Build last:

- survey
- ideation
- debate
- refine
- plan
- implement
- experiment
- result-to-claim
- repair
- write/package

Main inspirations:

- `ARIS`
- `AutoResearchClaw`
- `autoresearch`
- `AutoSOTA`

## Final Design Judgment

The strongest final design is:

- not a memory plugin added to an existing agent
- not a research workflow pasted onto a code assistant
- not a repo graph glued to a vector store

It is a native local CLI operating system for project work, with:

- layered memory
- repo-native intelligence
- explicit multi-agent artifact protocol
- enforced governance
- long-horizon research loops
- silent progress compaction and writeback

In short:

- `MemPalace` gives the wake-up shape
- `ClawMem` gives the hot-path retrieval discipline
- `code-review-graph` gives the structural repo lens
- `DiffMem` gives temporal repo memory
- `Ars Contexta` gives the memory-space ontology
- `Sekha` gives the enforcement layer
- `Hermes` gives the runtime abstraction
- `Claw Code` gives the recovery substrate
- `OpenAGS` gives the agent filesystem protocol
- `ARIS` gives the research operating model
- `AutoResearchClaw` gives the long-loop evolution pattern
- `autoresearch` gives the small-loop search pattern

That combination is the most operationally strong path toward the user's
target: a native project-memory, multi-agent, research-capable code agent CLI.
