# Exhaustive Memory Ecosystem Pass

This document records the second-phase exhaustive pass over supplementary memory
repos that were not included in the first mainline study.

## Goal

- Find memory-system patterns that the first pass may have missed
- Separate real architecture contributions from product packaging
- Refine the eventual final architecture with stronger evidence
- After user clarification, explicitly separate direct-name `mempalace` /
  `memory-palace` repos from the broader surrounding ecosystem

## Repos In Scope

- `reference_repos/memory_palace/mempalace`
- `reference_repos/memory/graphiti`
- `reference_repos/memory/langmem`
- `reference_repos/memory/letta`
- `reference_repos/memory/mem0`
- `reference_repos/memory_palace/Memory-Palace`
- `reference_repos/memory_palace/Memory-Palace-Openclaw`
- `reference_repos/memory_palace/memory-palace-setup`
- `reference_repos/memory_palace/mcp-memory-service`
- `reference_repos/memory_palace/supermemory`
- `reference_repos/memory_palace_ecosystem/memoriki`
- `reference_repos/memory_palace_ecosystem/memory-store-plugin`

## Status

- In progress

## Naming Clarification

The user clarified that "memory palace" should first refer to repositories
whose names directly include `mempalace` or `memory-palace`, not the wider
memory-palace-style ecosystem.

So the direct-name family for this study is:

- `reference_repos/memory_palace/mempalace`
- `reference_repos/memory_palace/Memory-Palace`
- `reference_repos/memory_palace/Memory-Palace-Openclaw`
- `reference_repos/memory_palace/memory-palace-setup`

Broader ecosystem repos such as `mcp-memory-service`, `supermemory`,
`memory-store-plugin`, and `memoriki` are still useful, but only as secondary
evidence around the direct-name Memory Palace family.

## Triage

These supplementary repos are not equally valuable.

### Primary direct-name Memory Palace sources

- `reference_repos/memory_palace/mempalace`
- `reference_repos/memory_palace/Memory-Palace`
- `reference_repos/memory_palace/Memory-Palace-Openclaw`
- `reference_repos/memory_palace/memory-palace-setup`

These are the first-class sources for answering the user's correction. Together
they cover:

- layered memory loading and token-efficient wake-up
- audited write-path governance
- host/plugin-specific durable-memory integration
- onboarding and installation routing across different agent hosts

### Highest-value architecture sources

- `reference_repos/memory/graphiti`
- `reference_repos/memory/langmem`
- `reference_repos/memory/letta`
- `reference_repos/memory/mem0`

These add substantial missing coverage:

- temporal graph memory
- hot-path vs background dual-channel memory management
- explicit core / recall / archival memory stratification
- production-style memory APIs with history and optional graph layer

### Medium-value architecture sources

- `reference_repos/memory_palace/Memory-Palace`
- `reference_repos/memory_palace/mcp-memory-service`
- `reference_repos/memory_palace/supermemory`
- `reference_repos/memory_palace_ecosystem/memory-store-plugin`

These appear especially relevant for:

- MCP-facing productization
- shared memory service patterns
- plugin/lifecycle capture
- production retrieval and contradiction handling

### Supporting or compositional sources

- `reference_repos/memory_palace_ecosystem/memoriki`

This looks more like a fusion pattern:

- wiki structure + memory-palace retrieval + entity graph

It may matter more as an ontology/composition reference than as a runtime
kernel.

## What to extract

For each repo:

1. Design goal
2. Core memory entities
3. Storage, indexing, and retrieval
4. Temporal / contradiction / supersession handling
5. MCP / plugin / CLI integration model
6. Hot-path vs background memory management
7. Reusable strengths
8. Limits for the target native project-memory CLI

## Expected decision outputs

- Which repos materially change the memory-plane design
- Which repos only confirm patterns already found
- Which repos should contribute implementation details but not top-level
  architecture

## Findings

## MemPalace

### Why it returns to the main line

- `mempalace` was already studied in the first pass, but the user's
  clarification makes it a direct-name Memory Palace source rather than just
  one memory-style repo among many.
- So in this second pass it needs implementation-level extraction, not only a
  passing mention in synthesis.

### Design goal

- Preserve memory as verbatim source text, then make retrieval efficient
  through structural organization and layered recall instead of aggressive
  summarization-first storage.

### Core design contributions

- The repo is unusually explicit about three coupled design choices:
  - verbatim storage in the main palace
  - a second pointer/index layer called `closets`
  - a four-layer recall stack for low-token wake-up and deeper retrieval on
    demand
- `layers.py` formalizes:
  - `L0` identity
  - `L1` essential story
  - `L2` wing/room-scoped recall
  - `L3` full semantic search
- `palace.py` adds a non-obvious but important intermediate structure:
  `closets` are compact pointer lines built from drawers, so the system can
  maintain a lighter searchable indirection layer without discarding the
  original text.
- The repo also treats agent memory as a first-class topology:
  specialist agents can get their own wings and diaries instead of sharing one
  undifferentiated memory pool.

### Reusable strengths

- Very strong evidence for a low-cost wake-up surface in the final
  architecture:
  always-visible identity and essential-story context should stay tiny and
  predictable.
- Strong support for keeping original project evidence verbatim, then building
  lighter derivative indexes above it instead of rewriting source content too
  early.
- The wing / room / drawer scheme is highly reusable for project-level memory:
  it maps naturally to repo, subsystem, task family, experiment line, and
  agent workspace partitions.
- Agent-specific wings and diaries are especially relevant for the target
  multi-agent CLI:
  each agent should have both private working memory and selectively promoted
  shared memory.

### Limits for our target system

- `mempalace` is strongest on memory loading and retrieval structure, but
  weaker than `Memory-Palace` on auditable write governance.
- The palace metaphor is useful, but the final system should translate it into
  project-native terms so repo memory does not feel like an external metaphor
  pasted onto code work.

## Graphiti

### Design goal

- Build a temporal context graph instead of a flat memory store.
- Focus on evolving facts, provenance, and historical truth windows for agent
  memory.

### Core data model

- The repo explicitly defines a temporally-aware graph over:
  - episodes
  - entities
  - entity edges / facts
- The server DTO exposes fact fields such as:
  - `fact`
  - `valid_at`
  - `invalid_at`
  - `created_at`
  - `expired_at`
- The docs also emphasize a bi-temporal model and explicit episode provenance.

### Retrieval and indexing

- Graphiti combines:
  - semantic retrieval
  - BM25 keyword retrieval
  - graph traversal
- This is one of the strongest confirmations in the whole study that durable
  memory needs hybrid retrieval rather than embeddings alone.

### Integration model

- The system is split into:
  - core graph library
  - FastAPI service
  - MCP server
- This is valuable because it separates memory semantics from client protocol.

### Reusable strengths

- Strongest supplementary evidence for a temporal graph plane in the final
  system.
- Strong confirmation that validity windows and episode provenance should be
  first-class in project memory.
- Useful for final design of claim history, decision supersession, and "what
  used to be true" queries.

### Limits for our target system

- Graphiti is powerful as a context graph substrate, but less directly focused
  on repo/project artifact governance.
- It still needs project-native entities above the generic graph model.

## LangMem

### Design goal

- Give agents both conscious memory management in the hot path and
  subconscious background memory extraction.
- Treat memory as a LangGraph-native capability rather than a separate product.

### Core model

- LangMem distinguishes:
  - graph/thread checkpointing
  - long-term store namespaces
  - memory tools for hot-path use
  - background reflection / extraction for offline consolidation
- The strongest implementation distinction is:
  - `MemorySaver` for thread state
  - `BaseStore` namespaces for long-term memory

### Retrieval and maintenance

- The hot-path guide shows explicit memory tool use plus prompt-time search.
- The background guide shows automatic extraction through
  `create_memory_store_manager`.
- This is one of the clearest examples in the whole repo set of separating:
  - active reasoning memory
  - durable memory storage
  - background consolidation

### Reusable strengths

- Very important for our target CLI:
  memory should have both hot-path and background channels.
- Namespace templating is highly reusable for user/project/org scoped memory.
- Strong confirmation that short-term execution state and long-term memory
  store should stay distinct.

### Limits for our target system

- LangMem is a memory toolkit, not a full repo-memory or multi-agent operating
  system.
- It contributes strongly to memory-plane internals, but less to repo hygiene
  or agent coordination policy.

## Letta

### Design goal

- Build agents around explicit multi-tier memory rather than treating memory as
  an add-on.
- Keep a small always-visible core memory while offloading deeper state to
  searchable stores.

### Core memory stratification

- The system prompt examples are especially informative:
  - recall memory = searchable conversation history
  - core memory = always-visible small block
  - archival memory = infinite external storage with explicit retrieval
- This is one of the clearest architectural memory splits in the study.

### Reusable strengths

- Strong confirmation that the final CLI should maintain an explicit
  "always-visible core" instead of only retrieval-based memory.
- The block-based core-memory idea is useful for:
  - persona
  - human/user profile
  - task queue
  - operating contract
- This reinforces the `M0` / `M1` distinction in the fusion design.

### Limits for our target system

- Letta is more agent-persona and user-memory oriented than repo-memory
  oriented.
- Its memory blocks are important, but they need project/repo/research-native
  extensions in our final architecture.

## Mem0

### Design goal

- Provide a production-grade memory layer with clear APIs, identifiers, update
  paths, history, and optional graph augmentation.
- Support user, agent, and run scoped memory across both OSS and hosted modes.

### Core model

- Mem0 formalizes scoped identifiers:
  - `user_id`
  - `agent_id`
  - `run_id`
- The API surface is notable because it includes:
  - `add`
  - `search`
  - `get`
  - `get_all`
  - `update`
  - `delete`
  - `delete_all`
  - `history`
- Optional graph memory exists on top of vector memory.

### Reusable strengths

- Strong supplementary evidence that project-memory systems need history APIs,
  not just add/search.
- Stronger CRUD and history discipline than many memory-palace repos.
- Good fit for the final architecture's need for artifact history and scoped
  session/run memory.

### Limits for our target system

- Mem0 is more general-purpose memory infrastructure than repo-governed project
  memory.
- The current public framing appears more memory-platform-like than
  project-operating-system-like.

## Interim judgment after first batch

- `Graphiti` materially strengthens the final design's temporal graph layer.
- `LangMem` materially strengthens the final design's dual-channel
  hot-path/background memory management.
- `Letta` materially strengthens the final design's explicit always-visible
  core-memory blocks.
- `Mem0` materially strengthens the final design's scoped memory CRUD and
  history APIs.

- None of these four replaces the repo-memory or governance conclusions from
  Layer B.
- Instead, they deepen the memory plane and make the final design less biased
  toward memory-palace-style abstractions alone.

## Memory Palace (AGI-is-going-to-arrive)

### Design goal

- Build a memory system that is not only persistent and searchable, but also
  operationally governed and auditable.
- Treat memory maintenance as an explicit product surface with review,
  rollback, vitality cleanup, and dashboard observability.

### Core design contributions

- This repo is much more than a simple MCP memory server.
- It combines:
  - write guard
  - snapshots before mutation
  - serialized write lanes
  - background index worker
  - hybrid retrieval
  - intent-aware routing
  - vitality-based lifecycle management
  - review/rollback UI

### Retrieval and governance

- Three retrieval modes are formalized:
  - keyword
  - semantic
  - hybrid
- It also adds intent classes:
  - factual
  - exploratory
  - temporal
  - causal
- This is one of the strongest supplementary examples of retrieval policy being
  an explicit control system, not just a ranker.

### Write-path design

- The write pipeline is especially valuable:
  - Write Guard decides add/update/noop/delete behavior
  - snapshots are taken pre-write
  - writes are serialized through write lanes
  - background reindexing is gated behind the same write lane
- This is one of the best concrete anti-chaos write-path designs in the entire
  memory repo set.

### Reusable strengths

- Strongest supplementary evidence that project memory needs:
  - pre-write governance
  - rollback-ready snapshots
  - write serialization
  - maintenance and observability surfaces
- Vitality decay plus governance loop is directly relevant to project memory
  cleanup and stale artifact control.

### Limits for our target system

- The repo is broad and highly productized, so not every deployment/profile
  detail should influence the final CLI.
- Its memory ontology still needs stronger project/repo/research-native typing
  above the current general memory path model.

## mcp-memory-service

### Design goal

- Provide a shared memory backend for multi-agent systems, with both REST and
  MCP access, self-hosted operation, and causal knowledge graph support.

### Core design contributions

- Shared memory is the central idea:
  memory is not bound to a single run or graph instance.
- The README highlights:
  - framework-agnostic REST API
  - typed graph edges such as causes/fixes/contradicts
  - agent-scoped tagging via `X-Agent-ID`
  - SSE notifications for memory mutations
  - local ONNX embeddings

### Reusable strengths

- Strong evidence for a shared-memory service plane in multi-agent research
  setups.
- The `X-Agent-ID` tagging model is very relevant for multi-agent provenance.
- Real-time memory mutation events are useful for agent coordination and
  observability.

### Limits for our target system

- Strong as a backend service, weaker as a full project-memory ontology.
- It does not replace repo governance, canonical artifacts, or research-stage
  memory discipline.

## Supermemory

### Design goal

- Be a unified memory and context engine that merges memory, RAG, user
  profiles, connectors, and multimodal extraction into one stack.

### Core design contributions

- The strongest supplementary concept here is the combination of:
  - auto memory extraction
  - static + dynamic user profiles
  - query-time memory retrieval
  - connectors / file ingestion
- Its SDK examples expose modes like:
  - `profile`
  - `query`
  - `full`
- This is a useful packaging of "always inject stable profile + optionally add
  query-specific recall."

### Reusable strengths

- Strong evidence that profile memory should be explicitly distinct from query
  recall.
- Useful reminder that context engines should support:
  - profile-only mode
  - query-only mode
  - full combined mode
- Connectors and multimodal ingestion are useful future extensions for the
  research memory plane.

### Limits for our target system

- Supermemory is highly product-centric and broad.
- It contributes more to context-engine mode design than to repo-native or
  research-governed architecture.

## Memory Store Plugin

### Design goal

- Add persistent memory to Claude Code through hooks and queue-based automatic
  tracking, without requiring the user to manually save everything.

### Core design contributions

- This repo is valuable because it focuses on workflow capture rather than
  abstract memory theory.
- Its main pipeline is:
  - session/file/commit/error events are captured by hooks
  - events are appended to `.memory-queue.jsonl`
  - a queue-processing skill ships them to the memory backend
- It also tracks:
  - corrections
  - quality score
  - periodic checkpoints
  - CLAUDE.md / anchor-comment relationships

### Reusable strengths

- Very strong evidence for event-queue-based background memory writeback.
- Especially relevant for the user's request that the system should silently
  summarize progress and absorb it into memory.
- Good fit for project-level operational memory and team memory sharing.

### Limits for our target system

- The plugin is tied to a specific host/plugin ecosystem.
- It captures development flow well, but does not define a full project-memory
  ontology or repo governance model by itself.

## Memoriki

### Design goal

- Fuse a persistent markdown wiki with semantic search and an entity graph, so
  knowledge compounds instead of being rediscovered from raw sources every time.

### Core design contributions

- Memoriki is explicitly a three-layer composition:
  - wiki structure
  - MemPalace drawers for semantic recall
  - MemPalace KG for entity relationships
- The most important idea is not runtime machinery but compounding knowledge
  compilation:
  raw sources are transformed into a maintained wiki.

### Reusable strengths

- Very strong support for the final architecture's research-memory and
  project-notebook layers.
- The `index.md` + `log.md` + generated synthesis pages pattern is highly
  relevant for research accumulation and project documentation.
- Linting for contradictions, orphans, and gaps is directly applicable to
  project memory maintenance.

### Limits for our target system

- Memoriki is more a pattern composition than a runtime kernel.
- Best reused as a design pattern for memory-backed knowledge compilation, not
  as the main execution substrate.

## Memory-Palace-Openclaw

### Design goal

- Turn Memory Palace from a general durable-memory runtime into an explicit
  OpenClaw plugin with a stable host command surface, onboarding flow, and
  verification path.

### Core design contributions

- This repo matters because it shows how a memory system becomes a
  host-native product rather than just a backend service.
- The README repeatedly reinforces a strict boundary:
  - repo wrapper commands for installation and onboarding
  - stable `openclaw memory-palace ...` commands for end users
- It also formalizes:
  - chat-first onboarding
  - profile-based rollout (`B` / `C` / `D`)
  - plugin install / verify / doctor / smoke lifecycle
  - experimental ACL-style multi-agent isolation

### Reusable strengths

- Strong direct-name evidence that the final target system needs a first-class
  host integration layer, not only a memory backend.
- The split between:
  - repo-local installer / wrappers
  - stable user-facing command surface
  - runtime verification commands
  is highly reusable for a serious code-agent CLI.
- The profile rollout path is useful for staging memory capability from
  minimal local mode to full provider-backed mode.

### Limits for our target system

- It is strongly OpenClaw-shaped.
- It contributes more to host packaging, onboarding, and operational
  boundaries than to the core memory ontology itself.

## memory-palace-setup

### Design goal

- Serve as the onboarding and routing skill for the main `Memory-Palace`
  system, teaching AI clients how to choose the right install and integration
  path for each host.

### Core design contributions

- This repo clarifies an important product boundary that many agent projects
  blur:
  - service startup is not the same as client integration
  - onboarding skill is not the same as runtime skill
  - CLI hosts and IDE hosts need different MCP wiring paths
- It also encodes a concrete routing policy:
  - prefer skills + MCP for normal use
  - treat MCP-only as fallback
  - distinguish temporary onboarding access from persistent runtime install

### Reusable strengths

- Strong direct-name evidence that our final system needs a dedicated setup /
  migration / doctor layer, rather than burying installation logic inside the
  main runtime README.
- The explicit host matrix is useful for a future CLI that must support
  multiple environments without confusing "service is up" with "client is
  actually bound."
- Good model for separating:
  - onboarding skill
  - runtime memory skill
  - host-specific MCP/config projection

### Limits for our target system

- It is an installer/router layer, not a memory runtime.
- Its value is operational clarity and host routing, not memory retrieval
  innovation.

## Direct-name Memory Palace family synthesis

The direct-name family now has a much clearer internal split than it first
appeared.

### 1. `mempalace` = retrieval-centric memory kernel

- Core strength:
  how memory is structured, loaded, and searched with low wake-up cost.
- Best ideas to carry forward:
  - `L0-L3` layered recall
  - verbatim source preservation
  - lighter pointer/index layer above raw memory
  - agent-specific wings / diaries

### 2. `Memory-Palace` = governed durable-memory runtime

- Core strength:
  how writes are audited, serialized, rolled back, and maintained safely.
- Best ideas to carry forward:
  - write guard
  - snapshot before mutation
  - serialized write lane
  - background index worker
  - vitality / maintenance loop

### 3. `Memory-Palace-Openclaw` = host-native plugin productization

- Core strength:
  how a durable-memory runtime becomes a real host feature with stable
  commands, onboarding, diagnostics, and profile-based rollout.
- Best ideas to carry forward:
  - plugin-first host boundary
  - `verify / doctor / smoke`
  - chat-first onboarding
  - staged capability ladder (`B/C/D`)

### 4. `memory-palace-setup` = onboarding and install router

- Core strength:
  how to keep setup logic explicit across many hosts without conflating skill
  visibility, MCP binding, and service startup.
- Best ideas to carry forward:
  - host matrix
  - onboarding/runtime separation
  - stable setup entrypoint
  - explicit "service up" vs "client connected" distinction

### What this means for the target architecture

The final target system should not copy one Memory Palace repo whole.
It should recombine the family like this:

- take the **memory layout** from `mempalace`
- take the **write governance runtime** from `Memory-Palace`
- take the **host plugin boundary and diagnostics** from
  `Memory-Palace-Openclaw`
- take the **setup/router layer** from `memory-palace-setup`

That combination is much stronger than any single repo in isolation.

## Cross-repo supplementary conclusions

### What this exhaustive memory pass adds beyond the first pass

- `mempalace`, from the first pass, remains the strongest direct-name evidence
  for layered wake-up memory and token-efficient recall.
- `Graphiti` confirms the need for a temporal graph plane.
- `LangMem` confirms the need for separate hot-path and background memory
  channels.
- `Letta` confirms the need for explicit always-visible core memory blocks.
- `Mem0` confirms the need for scoped memory CRUD plus history APIs.
- `Memory Palace` confirms the need for write guard, snapshots, rollback, and
  maintenance loops.
- `Memory-Palace-Openclaw` confirms the need for plugin-native host
  integration, staged rollout profiles, and explicit verify / doctor / smoke
  operations.
- `memory-palace-setup` confirms the need for a separate onboarding/router
  layer that understands host-specific install boundaries.
- `mcp-memory-service` confirms the value of shared memory services plus
  agent-scoped tagging and event notifications.
- `Supermemory` confirms the importance of profile-mode vs query-mode context.
- `memory-store-plugin` confirms the value of hook -> queue -> backend writeback.
- `Memoriki` confirms the value of compiled wiki memory as a durable knowledge
  artifact.

### What the direct-name Memory Palace family specifically contributes

- `mempalace` contributes:
  - layer-based recall
  - low wake-up token cost
  - navigable memory surfaces
- `Memory-Palace` contributes:
  - audited write path
  - snapshots and rollback
  - intent-aware retrieval
  - maintenance and vitality governance
- `Memory-Palace-Openclaw` contributes:
  - host-native pluginization
  - staged capability rollout
  - operational verification commands
- `memory-palace-setup` contributes:
  - host-aware onboarding
  - installation routing
  - explicit separation of onboarding vs runtime memory access

### Net effect on the final architecture

- The final architecture should include a richer memory plane than the first
  fusion pass.
- In particular it now clearly needs:
  - temporal graph memory
  - always-visible core blocks
  - hot-path recall
  - background consolidation
  - history-aware CRUD
  - write governance and rollback
  - event-driven silent writeback
  - compiled wiki/project notebook artifacts
  - host-native onboarding and doctor flows
  - a strict separation between memory runtime, host plugin layer, and setup
    router layer
