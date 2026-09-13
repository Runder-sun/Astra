# Layer A - Memory Core Deep Study

This document records design and implementation findings for memory core
systems: memory representation, storage, retrieval, consolidation, and hot-path
integration into agents.

## Repos In Scope

- `reference_repos/memory_palace/mempalace`
- `reference_repos/memory_palace_ecosystem/ClawMem`
- `reference_repos/memory_palace/AgentRecall`
- `reference_repos/memory_palace/memoryOSS`
- `reference_repos/memory_palace/hippo-memory`

## Findings

### Status

- First implementation pass complete

## MemPalace

### Design goal

- Position itself as a memory-palace-native memory system, not just a vector
  cache.
- Keep startup context bounded via a layered wake-up model and defer deeper
  retrieval until needed.
- Add two graph layers:
  - a semantic/temporal knowledge graph for entities and relationships
  - a palace graph for cross-room and cross-wing navigation

### Core data model

- The public model is explicitly a 4-layer stack:
  - `L0` identity
  - `L1` essential story
  - `L2` room recall
  - `L3` deep search
- The entity graph in `mempalace/knowledge_graph.py` stores:
  - `entities(id, name, type, properties, created_at)`
  - `triples(id, subject, predicate, object, valid_from, valid_to, confidence, source_closet, source_file, extracted_at)`
- The palace graph in `mempalace/palace_graph.py` treats:
  - rooms as graph nodes
  - cross-wing room reuse as tunnel edges
  - halls as corridor-like edge labels

### Storage and indexing

- Uses local SQLite for the temporal entity graph.
- Uses Chroma-backed palace storage for drawers/rooms/wings, then derives a
  graph view from collection metadata.
- Important detail: the system treats graph and vector memory as different
  surfaces, not a single merged store.

### Retrieval and ranking

- `L0 + L1` are fixed-budget wake-up context.
- `L2` does targeted recall by wing/room.
- `L3` performs semantic search only when topic demand is strong enough.
- The palace graph adds BFS-style traversal and tunnel discovery, so retrieval
  is not only embedding-based but also topology-aware.
- The temporal KG supports `as_of` queries, which is critical for memories that
  can become false later.

### Hook and runtime integration

- `hooks_cli.py` is one of the most useful implementation artifacts in this
  repo.
- It uses `stop` and `precompact` hooks to interrupt the harness before context
  loss and force a save cycle.
- The hook logic explicitly tells the agent to:
  - write a compressed diary
  - write verbatim drawers
  - optionally write KG relations
- This is stronger than passive memory capture: it turns memory writeback into
  a runtime checkpoint.
- `mcp_server.py` exposes read/write/maintenance tools and contains stdout
  protection so MCP JSON transport is not corrupted by noisy dependencies.

### Reusable strengths

- The `L0/L1/L2/L3` split is one of the clearest hot-path vs. cold-path memory
  designs in the set.
- Temporal validity in the KG is important for project memory because design
  decisions, bugs, and workarounds all have time windows.
- Hook-time save barriers before compaction are directly applicable to a native
  code agent CLI.
- Separating a topological palace graph from semantic search is a useful design
  move; one answers "what is related by structure", the other answers "what is
  semantically similar".

### Limits for our target system

- MemPalace is strong on personal/agent memory metaphor but weaker on
  codebase-native and repo-evolution-native memory than the repo-memory systems.
- The room/wing abstraction is elegant, but for a code agent it likely needs a
  second parallel ontology for files, symbols, branches, ADRs, and experiments.

## ClawMem

### Design goal

- Build an on-device memory layer that agents actually use during active coding,
  not just a store they can search manually.
- Combine hooks, MCP, retrieval gating, and a local SQLite vault into one
  memory runtime for Claude Code, OpenClaw, and Hermes.

### Core data model

- The central storage is a SQLite-backed vault plus index structures inside
  `src/store.ts`.
- The memory layer enriches raw documents with metadata such as:
  - domain
  - workstream
  - tags
  - content type
  - confidence
  - access count
- It also tracks:
  - session logs
  - context usage
  - recall events
  - entity graph neighbors

### Retrieval and scoring

- `src/memory.ts` implements a SAME-style composite scoring layer over search
  results.
- Important mechanisms:
  - content-type-specific half-lives
  - confidence baselines by content type
  - memory-type inference: episodic / semantic / procedural
  - access-adjusted recency half-life extension
  - confidence score with attention decay
  - dynamic composite weights, including recency-heavy mode
- `src/retrieval-gate.ts` decides when retrieval should be skipped entirely:
  greetings, shell commands, tiny acknowledgements, emoji-only turns, etc.
- This repo is very explicit that recall quality is not only about ranking but
  also about *when not to recall*.

### Hot-path integration

- `src/hooks/context-surfacing.ts` is the real core.
- It runs on every user prompt, but with multiple guards:
  - short prompt skip
  - slash command skip
  - adaptive retrieval gate
  - heartbeat suppression
  - duplicate-prompt suppression
- It supports:
  - multi-turn retrieval query expansion
  - vector + FTS hybrid retrieval
  - skill-vault secondary search
  - file-aware supplemental search
  - session-topic boost
  - tiered injection budgets
- `src/recall-buffer.ts` records which memories were surfaced to the model,
  making later feedback and recall quality analysis possible.

### Reusable strengths

- This is one of the strongest hot-path memory implementations in the set.
- The retrieval gate is directly reusable.
- The separation between:
  - discovery query
  - ranking query
  - injection budget
  - recall attribution
  is very mature and avoids many common memory-system mistakes.
- The system is already close to "project memory for active coding sessions."

### Limits for our target system

- ClawMem is excellent as a memory *runtime*, but by itself it does not define a
  full long-horizon project-memory ontology.
- It still needs a stronger persistent project model for branches, decisions,
  experiments, and file-version governance.

## AgentRecall

### Design goal

- Turn session memory into a compounding learning loop.
- Capture human corrections, cross-project insights, and cold-start context.
- Hide subsystem complexity behind a small MCP surface.

### Core data model

- The SDK reveals a five-layer shape:
  - L1 working capture
  - L2 journal / episodic memory
  - L3 palace memory
  - L4 awareness
  - L5 insight index
- The system also has:
  - graph edges between rooms
  - digest cache for precomputed analyses
  - awareness as a bounded compounding document

### Retrieval and ranking

- The MCP layer exposes six consolidated tools rather than many tiny tools:
  - `session_start`
  - `remember`
  - `recall`
  - `session_end`
  - `check`
  - `digest`
- `recall` uses RRF across multiple stores.
- The README also documents:
  - semantic auto-naming
  - multiple index layers
  - relativity edges in `graph.json`
  - salience scoring
  - Ebbinghaus-style decay by memory type
  - query-aware feedback that re-ranks future recall

### Hot-path integration

- `session_start` is especially strong: it loads identity, active rooms,
  cross-project insights, recent activity, and predictive warnings in one call.
- `session_end` writes journal, updates awareness, consolidates palace memory,
  and archives demoted insights.
- `digest` is particularly relevant for a code agent because it acts as a
  reusable context cache for previous audits and subagent explorations.

### Reusable strengths

- AgentRecall is the clearest example of memory as a compounding control loop,
  not a passive index.
- The "few high-level MCP tools, many internal subsystems" design is highly
  reusable for our target system.
- The bounded awareness cap is a strong anti-bloat design choice.
- Digest caching is very attractive for repeated codebase analysis, repeated
  repo audits, and repeated experiment reviews.

### Limits for our target system

- AgentRecall appears stronger on session continuity and personal alignment than
  on codebase structure.
- The palace/journal/awareness abstraction will need a project-native extension
  for files, branches, experiments, and repo cleanup decisions.

## memoryOSS

### Design goal

- Act as a portable local memory runtime in front of LLM API calls, not merely
  a database or plugin.
- Support both proxy-mode automatic recall/injection and explicit MCP tools.
- Define a versioned runtime contract so memory semantics are portable.

### Core data model

- `src/memory.rs` formalizes several important concepts:
  - `MemoryType`: episodic / semantic / procedural / working
  - `MemoryStatus`: candidate / active / contested / stale
  - a full `Memory` object with:
    - namespace
    - provenance
    - content hash
    - confidence
    - evidence count
    - superseded_by
    - derived_from
    - contradicts_with
    - usage counters
    - review events
    - optional team governance
- The object model is substantially more governance-heavy than most memory
  systems in this set.

### Retrieval and scoring

- `src/scoring.rs` is not a simple vector-score merger.
- It encodes:
  - multi-channel score combination
  - task-context-aware hinting
  - trust scoring
  - precision gating
  - diversity
  - optional primitive algebra decomposition
- The primitive system is notable:
  - memory can be decomposed into policy, constraint, incident, habit,
    environment, dependency, actor, evidence, task state
  - transfer operators then map those primitives into actionable reuse forms

### Runtime integration

- memoryOSS is architected as a gateway:
  - recall before model call
  - inject scoped context
  - forward upstream
  - extract after response
  - keep MCP available for explicit control
- This makes it the strongest candidate for a memory runtime placed "in front of
  the model" rather than embedded inside the harness only.

### Reusable strengths

- The namespace/provenance/supersede/merge/export semantics are very strong.
- The object model is close to what a serious project-memory OS needs.
- The primitive algebra idea is promising for turning memory from "text to
  recall" into "text to reusable operational state."

### Limits for our target system

- memoryOSS is runtime- and contract-centric, but less expressive about repo
  structure than code-review-graph or DiffMem.
- Its gateway architecture is powerful, but for a local coding CLI we may want
  a tighter coupling to files, diffs, and branch state.

## Hippo

### Design goal

- Build a shared portable memory layer across coding agents.
- Model memory more like a brain than a filing cabinet: decay, reinforcement,
  working memory, invalidation, consolidation, import/export.
- Keep storage local, git-trackable, and human-readable.

### Core data model

- `src/memory.ts` defines a concrete memory entry with:
  - timestamps
  - retrieval count
  - current strength
  - half-life
  - layer
  - tags
  - emotional valence
  - schema fit
  - source
  - outcome counters
  - conflicts
  - pinning
  - confidence
  - content
- The model uses three long-term layers in the type system:
  - buffer
  - episodic
  - semantic
- Separate working memory exists in `src/working-memory.ts` as a bounded buffer
  with scope, task, session, importance, and eviction.

### Storage and consolidation

- SQLite is the source of truth; markdown/YAML remain compatibility mirrors.
- The store creates human-readable mirrors under `.hippo/`.
- Working memory is explicitly separate from long-term memory and has bounded
  size with importance-based eviction.
- The CLI and hooks automate `sleep`, which triggers consolidation.

### Retrieval and decay logic

- Strength is computed from:
  - half-life decay
  - retrieval boost
  - emotional multiplier
  - reward-proportional decay modulation
- Decay basis is configurable:
  - clock
  - session
  - adaptive
- This repo is the strongest in explicit forgetting and reinforcement mechanics.

### Reusable strengths

- Working memory is implemented as a first-class subsystem, not a side effect.
- Adaptive decay for intermittent usage is highly relevant to real-world coding
  agents that are not always on.
- Import paths from ChatGPT, Claude, Cursor, markdown, and git history are very
  practical.
- The "sleep" metaphor maps well to session-end consolidation and nightly
  project cleanup.

### Limits for our target system

- Hippo is broad and pragmatic, but less graph-native than MemPalace and
  code-review-graph.
- Its design is excellent for portable agent memory, but project-specific
  structural memory still needs stronger graph and repo-state modeling.

## Layer A Summary

### Strongest reusable patterns

- **MemPalace:** layered wake-up model plus graph-aware memory
- **ClawMem:** retrieval gating plus active hot-path context surfacing
- **AgentRecall:** compounding session loop plus digest cache
- **memoryOSS:** rigorous runtime contract plus governed memory object model
- **Hippo:** bounded working memory plus biologically inspired decay

### Immediate fusion lessons

- A serious system should have at least four native memory planes:
  - wake-up identity and active project state
  - mission frame / goal hierarchy anchors
  - bounded working memory
  - structured durable memory with lineage and provenance
  - cold-path deep retrieval and graph traversal
- Retrieval quality depends as much on *gating* and *budgeting* as on ranking.
- Session-end and pre-compaction checkpoints are mandatory if we want memory to
  survive long coding sessions without transcript archaeology.
- Long-horizon goal anchors should survive compaction as schema-owned project
  state, not as model-rewritten summary prose. The project max goal, milestone
  goal, and current implementation goal form a stable "memory palace" spine for
  active coding context.
- Durable project memory should support:
  - namespace
  - provenance
  - supersession
  - contradiction
  - derivation lineage
  - usage/outcome counters
- No single Layer A repo solves project/repo memory alone. Layer B will need to
  supply file graph, repo evolution, and codebase-native structure.
