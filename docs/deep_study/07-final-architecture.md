# Final Architecture for a Native Project-Memory Multi-Agent CLI

> Superseded by `docs/deep_study/10-revised-final-architecture.md`.

This document is the final architecture proposal after the mainline study
(`01-04`) and the exhaustive supplementary passes (`05-06`).

It is written for the user's target system:

- a code agent CLI
- with native project-level memory
- native multi-agent debate and branch exploration
- proactive silent progress summarization and durable accumulation
- active project/repo cleanup and anti-chaos governance
- a full research skill pack from survey to experiment to repair to rerun

The goal is not to copy one existing repo. The goal is to combine the best
parts of the studied repos into a system that is both stronger and more
operable.

## Final judgment

The final system should be built as a **seven-plane operating system for
project work**:

1. runtime plane
2. session and recovery plane
3. memory plane
4. repo intelligence plane
5. multi-agent orchestration plane
6. governance plane
7. research workflow plane

No studied repo covers all seven well.

The strongest overall composition is:

- **runtime and session discipline**
  - `claw-code`
  - `OpenHands`
  - `hermes-agent`
- **memory structure**
  - `mempalace`
  - `Memory-Palace`
  - `Graphiti`
  - `LangMem`
  - `Letta`
  - `Mem0`
- **project/repo memory**
  - `DiffMem`
  - `code-review-graph`
  - `ARIS`
  - `Sekha`
- **multi-agent and search**
  - `hermes-agent`
  - `claw-code`
  - `multi-agent-ralph-loop`
  - `AutoResearch-SibylSystem`
  - `OpenAGS`
  - `AutoSOTA`
  - `Autonomous Researcher`
- **research workflow**
  - `AI-Scientist`
  - `AgentLaboratory`
  - `GPT Researcher`
  - `open_deep_research`
  - `AI-Research-SKILLs`
- **host/plugin and operational packaging**
  - `Memory-Palace-Openclaw`
  - `memory-palace-setup`
  - `OpenHands`

The final system should therefore be designed as a **project-memory operating
layer** for code and research work, not as:

- a pure memory product
- a pure chat loop
- a pure workflow graph
- a pure swarm shell
- a pure research demo

## Design principles

### 1. Project is the top-level object

The system is not centered on "user memory" or "chat history."
It is centered on a **project operating graph**:

- goals
- repo state
- decisions
- files
- experiments
- branches
- agents
- claims
- failures
- lessons

Everything else attaches to that.

### 2. Memory must be layered, not monolithic

From `mempalace`, `Letta`, `LangMem`, and `Mem0`, the final answer is clear:
memory is not one bucket.

The final system needs:

- always-visible core memory
- session memory
- project durable memory
- graph memory
- repo/artifact memory
- compiled wiki memory
- background consolidation memory

### 3. Multi-agent is not just "spawn many agents"

The good multi-agent repos prove that multi-agent quality comes from control
surfaces:

- clear task ownership
- bounded write scopes
- explicit debate artifacts
- branch comparison
- aggregation
- rollback
- coordination memory

Without those, "many agents" becomes chaos.

### 4. Repo cleanliness is a first-class capability

This is not a nice-to-have.
If the system keeps generating extra versions of files, half-merged documents,
or unclear drafts, project memory becomes poison.

So repo governance must be native.

### 5. Research flow needs both fast loops and reflective loops

The best research repos converge on two rhythms:

- fast execution loop
- slower synthesis / direction loop

The final system should make both first-class.

## Plane 1 - Runtime Plane

### Role

The runtime plane owns:

- task execution
- tool calling
- sandbox and shell boundaries
- event stream
- host/plugin integration
- per-agent workspaces

### Final design

The runtime should combine three ideas:

- from `claw-code`
  - session control
  - summary compression
  - CLI-native long-session discipline
- from `OpenHands`
  - runtime abstraction
  - broader packaging separation
  - repo-local extensibility surfaces
- from `Memory-Palace-Openclaw`
  - stable host-native command surface
  - `verify / doctor / smoke`

The runtime kernel should stay relatively small.
It should not own all project logic directly.
It should provide stable interfaces for:

- turn execution
- event emission
- background workers
- session compaction
- memory calls
- repo graph calls
- agent dispatch

### Required runtime interfaces

- `run_turn`
- `emit_event`
- `compact_session`
- `dispatch_agent`
- `resume_agent`
- `persist_checkpoint`
- `write_memory_event`
- `query_project_graph`
- `run_research_stage`
- `doctor_runtime`

### Runtime outputs

Every action should produce structured events, not only raw text:

- `turn_started`
- `tool_started`
- `tool_finished`
- `write_candidate_created`
- `memory_write_committed`
- `artifact_created`
- `artifact_superseded`
- `agent_spawned`
- `debate_round_closed`
- `experiment_result_recorded`
- `repo_cleanup_applied`

This is one of the main lessons from `Autonomous Researcher`,
`OpenHands`, and the Memory Palace operational repos:
structured status is necessary for long-horizon control.

## Plane 2 - Session And Recovery Plane

### Role

This plane owns:

- session continuity
- interruption recovery
- context compaction
- resumability
- crash-safe progress persistence

### Final design

Use a three-part recovery model:

1. **active session state**
   - live turns, open sub-agents, current branch
2. **compact session summary**
   - what happened so far, what is in flight, next unresolved actions
3. **durable project checkpoint**
   - current project state outside the chat transcript

This combines:

- `hermes-agent`
  - structured compaction
  - protected head/tail logic
  - session search over older work
- `claw-code`
  - session control and summary compression
- `LangMem`
  - thread checkpointing vs long-term store separation

### Required recovery triggers

- token pressure
- too many tool results
- task boundary reached
- agent wave closed
- experiment wave closed
- shutdown / crash / restart
- explicit user "continue"
- explicit branch handoff

### Recovery artifacts

- `SESSION_STATE.json`
- `SESSION_SUMMARY.md`
- `NEXT_ACTIONS.md`
- `OPEN_THREADS.md`
- `RECOVERY_CHECKPOINT.json`

These should live under the project operating directory, not hidden inside one
client-specific transcript store.

## Plane 3 - Memory Plane

### Role

This is the heart of the system.
It owns:

- project memory ontology
- recall routing
- consolidation
- promotion/demotion
- temporal truth
- memory writes and rollback

### Final fused memory model

The final model should use seven memory surfaces:

- `M0` core identity and contract
- `M1` active operating context
- `M2` session summaries and recovery state
- `M3` project durable memory
- `M4` repo/artifact memory
- `M5` temporal graph memory
- `M6` compiled wiki / notebook memory

### `M0` - Core identity and contract

From `Letta`, `OpenClaw`, and `mempalace`:

- who the agent is in this project
- user preferences
- project mission
- operating rules
- critical always-visible constraints

This must stay tiny and stable.

### `M1` - Active operating context

This is the current "hot strip":

- current objective
- current branch
- open risks
- current hypotheses
- open blockers
- immediate next actions

This changes often.

### `M2` - Session summaries

This is the handoff and continuity layer:

- compact summaries
- recent decisions
- latest experiment outcomes
- pending threads
- partially finished changes

### `M3` - Project durable memory

This is the long-lived project memory:

- major decisions
- architecture rationale
- accepted plans
- repeated user preferences
- known failure patterns
- lessons learned
- important non-code artifacts

This should follow the `Mem0` lesson:
it needs CRUD plus history, not just search.

### `M4` - Repo/artifact memory

This is a special surface.
It is not "memory about people."
It is memory about project objects:

- files
- modules
- tests
- docs
- experiments
- datasets
- outputs
- branches
- PR-like changes
- benchmarks

This surface should be tightly linked to the repo intelligence plane.

### `M5` - Temporal graph memory

From `Graphiti`, `DiffMem`, and `Memory-Palace`:

- entities
- facts
- decisions
- causal edges
- contradiction edges
- supersession edges
- validity windows

This plane answers:

- what is true now
- what used to be true
- what changed
- why it changed
- which artifact replaced which older one

### `M6` - Compiled wiki / notebook memory

From `Memoriki`, `ARIS research_wiki`, and research-oriented repos:

- curated pages
- syntheses
- field maps
- experiment storylines
- topic notebooks
- glossary / index / logs

This is the memory surface optimized for human and agent re-entry together.

### Layered recall

The final recall router should explicitly borrow the `mempalace` layered idea:

- `L0`
  - core contract
- `L1`
  - essential project story
- `L2`
  - namespace-scoped recall
- `L3`
  - deep search
- `L4`
  - graph and wiki traversal

This gives:

- low-cost wake-up
- bounded hot context
- deeper search only when needed

### Write pipeline

The write path should follow the strongest lessons from `Memory-Palace`,
`memory-store-plugin`, and `LangMem`:

1. event capture
2. write candidate creation
3. write guard / dedup / contradiction check
4. snapshot before mutation
5. serialized write lane
6. commit
7. async index / graph / wiki updates
8. maintenance loop

### Memory write events

Important memory-generating events include:

- user correction
- architecture decision
- successful experiment
- failed experiment with diagnostic value
- benchmark delta
- file supersession
- duplicate artifact detection
- task completion
- debate conclusion
- project restructuring
- recovery summary

### Maintenance loop

The maintenance loop should run silently in the background:

- vitality decay
- orphan cleanup
- contradiction detection
- stale artifact detection
- duplicate version clustering
- wiki gap detection
- summary promotion
- low-value memory demotion

This is a direct carry-over from `Memory-Palace`, but extended to project
artifacts and research traces.

## Plane 4 - Repo Intelligence Plane

### Role

This plane gives the agent a native model of the repository as an evolving
project, not only a folder tree.

### Final structural model

Represent the repo as typed entities:

- repository
- branch / branch-family
- directory
- module
- file
- symbol
- test
- document
- experiment
- dataset
- benchmark run
- generated artifact
- canonical artifact
- archived artifact

### Required relations

- `contains`
- `imports`
- `depends_on`
- `tests`
- `documents`
- `implements`
- `generated_by`
- `supersedes`
- `contradicts`
- `derived_from`
- `owned_by_agent`
- `belongs_to_hypothesis`
- `supports_claim`
- `refutes_claim`

### Why this plane must be native

The user's target system needs:

- project-level memory
- automatic cleanup
- anti-version-chaos governance

Those are impossible if the repo is treated as opaque text only.

### Temporal repo model

From `DiffMem`, `code-review-graph`, and `Graphiti`, the repo plane should
also be temporal:

- current canonical file
- older superseded file
- abandoned branch result
- reverted experiment artifact
- document lineage

This lets the system answer:

- which file is the authoritative one
- which older results should not be reused
- why a design pivot happened
- what branch produced the winning implementation

## Plane 5 - Multi-Agent Orchestration Plane

### Role

This plane owns:

- agent topology
- task decomposition
- debate
- branch search
- aggregation
- coordination memory

### Final topology

Use a bounded hierarchy:

- **director**
  - owns project direction, stage transitions, aggregation
- **specialist workers**
  - code, docs, experiments, literature, debugging, evaluation
- **critics**
  - review plans, detect flaws, challenge conclusions
- **search agents**
  - branch / sweep / alternative proposal exploration
- **maintenance agents**
  - summarize, clean repo, consolidate memory

### Required agent contracts

Every agent must have:

- role
- owned scope
- write scope
- read scope
- input artifact
- expected output artifact
- stop condition
- escalation condition

This is the main lesson from the better multi-agent repos:
ownership and artifacts matter more than fancy dialogue.

### Debate mode

Debate should be explicit and artifact-backed.

Use debate for:

- idea selection
- architecture choices
- experiment interpretation
- suspicious result analysis
- benchmark anomalies

Each debate round should produce:

- proposition
- supporting evidence
- opposing evidence
- unresolved assumptions
- decision
- confidence
- next verification action

### Branch-search / grid-search mode

The user explicitly asked for grid-search-like search over ideas and plans.

So the system should support:

- alternative plan branches
- parameter/config sweeps
- prompt strategy sweeps
- architecture branch comparisons
- experiment-family search
- repair-strategy search

Each branch should have:

- branch id
- parent branch
- hypothesis / design delta
- owned artifacts
- evaluation result
- keep / merge / archive decision

### Shared memory rules

Multi-agent memory must not be one shared mutable notebook.

Use three scopes:

- private agent scratch
- shared candidate memory
- promoted project memory

Children do not write directly into promoted project memory.
They propose, then a governor or director commits.

That is one of the clearest lessons from `hermes-agent` restrictions and the
better governed memory repos.

## Plane 6 - Governance Plane

### Role

This plane prevents project chaos.

It owns:

- write permissions
- canonical artifact policy
- duplicate prevention
- supersession tracking
- archival rules
- cleanup automation

### Canonical artifact policy

Every important artifact should have one of these statuses:

- `draft`
- `candidate`
- `canonical`
- `superseded`
- `archived`
- `rejected`

Only one artifact in a family should normally be `canonical`.

### Anti-duplicate protocol

Before creating a new major artifact, check:

- does an equivalent artifact already exist
- is this a revision or a new branch
- what family should it belong to
- should it supersede, fork, or append

If not clear, create under a branch family with explicit lineage metadata.

### Required governance hooks

- before major write
- before doc creation
- before experiment result promotion
- before summary promotion
- before final report generation
- before branch merge
- before cleanup/archive

### Repo management directories

The final project model should use an explicit hidden operating directory:

```text
.pmcli/
  runtime/
  sessions/
  memory/
  graph/
  repo/
  agents/
  debates/
  branches/
  experiments/
  wiki/
  governance/
  checkpoints/
```

The user-facing repo should remain clean.
Most operational state should live here, not scattered through random folders.

### Human-visible control files

At repo root, keep a tiny set of stable, human-readable control files:

- `DIRECTIVE.md`
- `STATUS.md`
- `NEXT_ACTIONS.md`
- `PROJECT_INDEX.md`

These make re-entry cheap for both human and agent.

### Silent maintenance actions

The system should proactively:

- merge duplicate notes
- archive superseded drafts
- relink orphan experiments
- normalize branch metadata
- rebuild indexes
- update wiki index
- record cleanup decisions in memory

But it should do this under governance rules, not by random deletion.

## Plane 7 - Research Workflow Plane

### Role

This plane turns the system from a coding assistant into a native research
agent.

### Final native research stages

The final workflow should have nine native stages:

- `R1` problem intake and framing
- `R2` literature mapping and evidence collection
- `R3` idea generation and debate
- `R4` method refinement and hypothesis design
- `R5` implementation and environment prep
- `R6` experiment execution and monitoring
- `R7` result analysis and claim judgment
- `R8` repair / pivot / rerun loop
- `R9` report / paper / decision package generation

### Stage behavior

#### `R1` Problem intake and framing

Outputs:

- problem statement
- constraints
- success criteria
- baseline assumptions

#### `R2` Literature mapping

Outputs:

- source map
- paper summaries
- claim graph
- benchmark map
- open gaps

This stage should combine:

- `GPT Researcher`
- `open_deep_research`
- `AgentLaboratory`
- local research wiki accumulation

#### `R3` Idea generation and debate

Outputs:

- candidate ideas
- novelty concerns
- debate record
- ranked shortlist

This stage should use explicit debate agents, not single-agent brainstorming
only.

#### `R4` Method refinement and hypotheses

Outputs:

- focused method plan
- hypothesis tree
- experiment matrix
- risk list

#### `R5` Implementation and environment prep

Outputs:

- code branches
- infra checklist
- dataset readiness
- evaluation harness readiness

#### `R6` Experiment execution and monitoring

Outputs:

- run registry
- metrics
- logs
- failures
- partial findings

This stage should learn from `Autonomous Researcher`, `Sibyl`, and `AI-Scientist`:

- hypothesis-level dispatch
- GPU-aware scheduling
- worker isolation
- multi-wave execution

#### `R7` Result analysis and claim judgment

Outputs:

- supported claims
- unsupported claims
- ambiguity report
- anomaly list

This is where many projects are weak.
The final system should be explicit about "what the evidence actually shows."

#### `R8` Repair / pivot / rerun

Outputs:

- bug fix list
- design revision
- document revision
- rerun plan

This is a native loop, not a special case.

#### `R9` Report / paper / decision package

Outputs:

- internal report
- paper outline
- experiment appendix
- future work map
- final memory promotion package

### Research skill pack structure

The final system should not have one flat research skill.
It should have four layers:

1. **orchestration skills**
   - stage transitions
   - routing
   - debate
   - branch search
2. **research operation skills**
   - literature
   - novelty
   - idea refinement
   - experiment planning
   - result-to-claim
3. **domain execution skills**
   - coding
   - ML training
   - evaluation
   - infrastructure
   - plotting
   - paper writing
4. **maintenance skills**
   - summarize
   - consolidate memory
   - clean repo
   - archive
   - doctor / verify

This is the cleanest lesson from `AI-Research-SKILLs` and the better workflow
repos:
research capability needs a tree, not a blob.

## Native On-Disk Project Model

The system should treat `.pmcli/` as the operational truth store.

### Proposed layout

```text
.pmcli/
  runtime/
    events.jsonl
    health.json
  sessions/
    active/
    summaries/
  memory/
    core.md
    active.md
    durable/
    snapshots/
  graph/
    entities.jsonl
    relations.jsonl
  repo/
    artifacts.jsonl
    canonical.json
    supersession.jsonl
  agents/
    registry.json
    private/
    shared/
  debates/
    rounds/
    decisions/
  branches/
    registry.json
    evaluations/
  experiments/
    registry.jsonl
    results/
  wiki/
    index.md
    log.md
    topics/
  governance/
    rules.md
    audit.jsonl
  checkpoints/
    latest.json
```

### Why this model

It gives the system:

- strong local persistence
- host independence
- explicit operational boundaries
- easier migration across CLI clients
- human inspectability

## What to copy closely

### Copy closely

- `mempalace`
  - layered recall
  - low wake-up context
  - agent-private wings/diaries
- `Memory-Palace`
  - write guard
  - snapshots
  - write lane
  - maintenance loop
- `Memory-Palace-Openclaw`
  - plugin-first host boundary
  - `verify / doctor / smoke`
- `memory-palace-setup`
  - host routing separation
- `LangMem`
  - hot-path vs background separation
- `Graphiti`
  - temporal truth model
- `AI-Research-SKILLs`
  - orchestration skill vs domain skill split

### Adapt heavily

- `hermes-agent`
  - session search
  - self-improving skills
  - constrained delegation
- `claw-code`
  - session compaction and runtime discipline
- `Sibyl`
  - stage machine
  - debate
  - GPU scheduling
- `AI-Scientist`
  - idea-centric execution
- `GPT Researcher`
  - planner / executor / publisher split
- `open_deep_research`
  - configurable graph workflow

### Do not copy literally

- giant monolithic agent files
- one giant shared memory store with no write governance
- flat multi-agent chat without ownership
- unlimited artifact creation without canonical status
- "session transcript = project memory"
- research loops with no claim-judgment stage

## Main tradeoffs and final choices

### Tradeoff 1: verbatim memory vs extracted memory

Decision:

- keep verbatim evidence as primary storage
- allow extracted facts / summaries / wiki pages as derived layers

Reason:

- `mempalace` is right that early rewriting loses evidence
- `Memory-Palace` is right that higher-level governance and maintenance are
  still needed

### Tradeoff 2: shared memory service vs local project memory

Decision:

- local project memory is primary
- shared service is optional secondary layer

Reason:

- the target system is fundamentally project-native
- shared memory can be added later for cross-machine or cross-agent sync

### Tradeoff 3: plugin-first vs backend-first

Decision:

- use a local project runtime as the true core
- expose plugin/host integrations as shells on top

Reason:

- avoids lock-in to one host
- preserves portability across Codex/Claude/OpenCode-like clients

### Tradeoff 4: free-form swarm vs bounded teams

Decision:

- use bounded role teams with explicit artifacts

Reason:

- better reliability
- easier repo hygiene
- easier memory attribution

### Tradeoff 5: research-general vs code-general

Decision:

- build a code-agent kernel first
- make research a first-class plane on top, not an afterthought

Reason:

- the user's goal is a code agent CLI with research-native powers, not a
  paper-only bot

## Suggested implementation order

### Phase 1

- runtime kernel
- session checkpoints
- compaction
- event stream
- `.pmcli/` layout

### Phase 2

- layered memory core
- write guard
- snapshots
- serialized writes
- memory router

### Phase 3

- repo intelligence graph
- canonical artifact policy
- supersession tracking
- duplicate prevention

### Phase 4

- bounded multi-agent topology
- private/shared/promoted memory scopes
- debate artifacts
- branch-search engine

### Phase 5

- research stages `R1-R9`
- literature map
- hypothesis tree
- experiment registry
- result-to-claim
- repair/rerun loop

### Phase 6

- compiled wiki
- maintenance agents
- automatic cleanup
- doctor / verify / smoke surfaces

## Final answer in one sentence

The best possible target system is a **project-memory operating layer for code
and research work**: `mempalace`-style layered recall, `Memory-Palace`-style
write governance, `Memory-Palace-Openclaw`-style host productization,
`memory-palace-setup`-style installation routing, `Graphiti`-style temporal
truth, `DiffMem`-style repo lineage, `hermes/claw`-style session discipline,
and `AI-Scientist` plus `AI-Research-SKILLs`-style multi-stage research loops
with bounded multi-agent debate and branch search.
