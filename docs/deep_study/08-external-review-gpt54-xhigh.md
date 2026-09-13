# External Review of Final Architecture (`gpt-5.4`, `xhigh`)

This document records the external architecture review requested after the
first final architecture draft in `07-final-architecture.md`.

## Review target

- `docs/deep_study/01-layer-a-memory-core.md`
- `docs/deep_study/02-layer-b-project-repo-memory.md`
- `docs/deep_study/03-layer-c-multi-agent-research.md`
- `docs/deep_study/04-fusion-patterns.md`
- `docs/deep_study/05-exhaustive-memory-ecosystem.md`
- `docs/deep_study/06-exhaustive-research-ecosystem.md`
- `docs/deep_study/07-final-architecture.md`

## Review goal

Check whether the current architecture:

1. covers the user's actual goals
2. truly absorbs the best ideas from the studied repos
3. is concrete enough for implementation
4. has dangerous abstraction / operability gaps
5. is ready for implementation, or only ready for blueprinting

## High-level result

The external review judged the current architecture as:

- strong as a final architecture mother draft
- not yet strong enough as a direct implementation blueprint
- good enough to stop broad repo research
- not yet good enough to start coding without another blueprint pass

In other words:

- **Go for implementation blueprint**
- **No-Go for direct implementation**

## Findings

### 1. Critical — No unified executable state machine

**Problem**

`07-final-architecture.md` defines planes, stages, interfaces, and artifacts,
but does not yet collapse them into one executable system contract.

Missing core contracts include:

- `ProjectState`
- `StageState`
- `TransitionGuard`
- `PromotionRule`
- unified event-to-stage mapping

**Why this matters**

Without one state machine, runtime, memory, and research workflow will each
grow their own implicit control logic.

That creates three semi-independent systems instead of one coherent operating
layer.

**Required correction**

Create an implementation blueprint that explicitly defines:

- project state object
- stage state object
- event model
- allowed transitions
- stage entry / exit gates
- artifact promotion rules

### 2. Critical — Multi-agent design is still too soft

**Problem**

The current design has agent roles and general contracts, but it does not yet
formalize:

- per-agent filesystem protocol
- reviewer independence / review blinding
- anti-self-confirmation rules
- explicit governor role resolution

There is also an inconsistency:

- shared memory rules mention `governor or director commits`
- but `governor` is not fully defined as a system role

**Why this matters**

Without hard agent contracts, multi-agent orchestration will degrade into
parallel chat.

Without reviewer independence, debate and review become optimistic self-talk
instead of real adversarial evaluation.

**Required correction**

Define a hard per-agent working contract under `.pmcli/agents/<agent_id>/`:

- `DIRECTIVE.md`
- `STATUS.md`
- `PRIVATE_MEMORY.md`
- `TRACE.jsonl`
- `OUTPUT_MANIFEST.json`

Also define:

- reviewer blinding rules
- cross-model diversity policy
- submission/review trace retention policy
- explicit commit authority (`director` only, or a formal `governor`)

### 3. High — Memory plane lacks concrete schemas and retrieval budgets

**Problem**

The memory plane has strong conceptual layering, but is still missing:

- `MemoryRecord` schema
- working-memory contract
- retrieval router / budget matrix
- explicit promotion / demotion / forgetting rules

**Why this matters**

Without concrete memory records, the memory plane will collapse into ad hoc
markdown and jsonl piles.

Without retrieval budgets, memory use will either be too timid or too bloated.

Without forgetting rules, long-term projects will drown in their own history.

**Required correction**

Define:

- `MemoryRecord`
- `RecallRoute`
- promotion / demotion / forgetting policy

At minimum, `MemoryRecord` should include:

- id
- surface
- namespace
- type
- body
- provenance
- source session
- source artifacts
- confidence
- status
- validity window
- supersession / contradiction links
- usage statistics

### 4. High — Repo anti-chaos policy is still conceptual

**Problem**

The architecture has good governance ideas, but still lacks a fully operational
artifact-family protocol.

It does not yet define enough detail for:

- `ArtifactFamily`
- canonical pointer semantics
- latest-vs-archived behavior
- supersession chain rules
- append / fork / supersede / reject decision logic

**Why this matters**

Without a real artifact-family protocol, `canonical` remains only a label.
It will not prevent version sprawl or confusing parallel drafts.

**Required correction**

Define an `ArtifactFamily` contract with:

- family id
- kind
- canonical id
- latest pointer
- archive policy
- supersession chain
- merge policy

Also define the pre-write decision tree:

- append
- fork
- supersede
- reject

### 5. High — Branch search / grid-search is not executable yet

**Problem**

The design correctly includes branch-search and grid-search-like exploration,
but it does not yet define:

- search budget
- evaluation contract
- worktree / sandbox lifecycle
- termination criteria
- keep / merge / archive rules

**Why this matters**

This is one of the user's most explicit requirements.
Without hard limits and evaluation structure, it becomes the most expensive,
messy, and irreproducible subsystem.

**Required correction**

Define:

- `SearchBatch`
- `BranchRun`
- `EvaluationContract`

And specify:

- max branches per wave
- concurrency
- stop criteria
- comparison metrics
- winner thresholds
- sandbox/worktree binding

### 6. High — Several key repo strengths were not fully absorbed

**Problem**

The external review judged the overall synthesis as good, but not yet complete.

The most important under-absorbed patterns are:

- `memoryOSS`
  - gateway / object model discipline
- `hippo-memory`
  - bounded working memory and forgetting
- `ARIS`
  - reviewer independence and query-pack discipline
- `GPT Researcher`
  - planner / executor / publisher split
- `Autonomous Researcher`
  - persistent per-agent sandbox contract

**Why this matters**

These are not decorative details.
They are the mechanisms that turn a strong concept design into a system that
actually operates cleanly.

**Required correction**

Add these as explicit required blueprint modules:

- memory gateway contract
- bounded working memory and forgetting
- query-pack / failed-idea briefing
- planner / executor / publisher split
- per-agent persistent sandbox contract

### 7. Medium — Host / plugin / setup layers still need API boundaries

**Problem**

The final architecture correctly says the runtime should be core and host
integrations should be shells on top, but it still does not define those API
boundaries concretely.

**Why this matters**

Without this, the first implementation can easily become coupled to one host
CLI.

**Required correction**

Define three API layers:

- `CoreRuntimeAPI`
- `HostAdapterAPI`
- `SetupDoctorAPI`

Each must specify:

- who calls it
- when it is called
- whether it may write repo state
- what guarantees it provides

### 8. Medium — `.pmcli/` is still a layout proposal, not a protocol

**Problem**

The `.pmcli/` directory tree is useful, but still lacks:

- schema versioning
- migration rules
- trace retention
- task registry
- hook registry
- storage rules for debate traces / research reviews / evaluation artifacts

**Why this matters**

Without schema versioning and migration, `.pmcli/` will become brittle quickly.

Without trace retention rules, auditability and replay quality will degrade.

**Required correction**

Upgrade `.pmcli/` from layout to persistence protocol:

- `SCHEMA_VERSION`
- `migrations/`
- `traces/`
- `tasks/`
- `hooks/`
- `configs/`

For each subdirectory, define:

- record type
- retention policy
- cleanup rule
- migration rule

## Top 5 risks

1. runtime / memory / research workflow each evolve their own implicit state
   machine
2. multi-agent review lacks real reviewer independence and becomes self-bias
   amplification
3. memory system lacks bounded working-memory and forgetting, then collapses
   under history load
4. repo governance lacks artifact-family protocol and recreates version chaos
5. branch/grid search lacks budget and evaluation contracts, causing runaway
   cost and irreproducibility

## Verdicts

### Coverage Verdict

The architecture covers almost all user requirements directionally, but not yet
in a fully native or executable sense.

It covers:

- project-level memory
- native multi-agent debate / branch search
- silent summarization into memory
- repo anti-chaos governance
- full research lifecycle

But the review judged that these remain too abstract in several critical
places.

### Synthesis Verdict

The synthesis is strong on the main lines, but does not yet fully absorb every
important operational advantage from all studied repos.

The biggest under-absorbed assets are:

- memory gateway discipline
- bounded working memory
- reviewer independence
- query-pack discipline
- planner / executor / publisher decomposition
- persistent per-agent sandboxes

### Operability Verdict

Not yet sufficient for direct implementation.

Sufficient for the next phase:

- implementation blueprinting

Not sufficient for:

- immediate coding

### Go / No-Go

- **Go**: enter implementation blueprint phase
- **No-Go**: start direct implementation now

## Minimum required additions before implementation

- unified `ProjectState` / `StageState` / `Event` / `TransitionGuard` /
  `PromotionRule`
- schemas for:
  - `MemoryRecord`
  - `ArtifactRecord`
  - `BranchRecord`
  - `ExperimentRecord`
  - `ClaimRecord`
  - `AgentRunRecord`
- per-agent filesystem contract
- reviewer independence rules
- retrieval router and token budget matrix
- working-memory / forgetting rules
- artifact family protocol
- branch-search / evaluation protocol
- R1-R9 stage entry/exit and required artifact table
- core runtime / host adapter / setup-doctor API boundary definition

## Final recommendation

Do not continue broad repo research first.

The next correct move is to convert `07-final-architecture.md` into a strict
implementation blueprint with schemas, protocols, state transitions, and
artifact contracts.
