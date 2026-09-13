# Layer B - Project and Repo Memory Deep Study

This document records design and implementation findings for systems that model
project context, codebase structure, repo evolution, and human-readable memory
surfaces tied to real development work.

## Repos In Scope

- `reference_repos/memory_palace/code-review-graph`
- `reference_repos/memory_palace/DiffMem`
- `reference_repos/memory_palace/arscontexta`
- `reference_repos/memory_palace_ecosystem/sekha`
- `reference_repos/requested/ARIS`

## Findings

### Status

- First implementation pass complete

## code-review-graph

### Design goal

- Turn a real codebase into an inspectable structural graph, then keep a
  lightweight human-readable memory surface around that graph.
- Optimize for token-efficient agent workflows: the graph does the heavy
  lifting, while the memory/wiki outputs provide compact re-entry points.

### Core data model

- The persistent substrate is a code knowledge graph over files, functions,
  communities, and flows.
- `code_review_graph/memory.py` adds a second layer: saved Q&A or review
  results as markdown files under `.code-review-graph/memory/`.
- `wiki.py` materializes graph communities into markdown wiki pages with:
  - overview
  - members
  - execution flows
  - dependency summaries

### Storage and indexing

- Structural knowledge lives in the graph store / sqlite-backed graph layer.
- Interaction memory is filesystem-native:
  - `.code-review-graph/memory/*.md` for persisted answers
  - generated wiki pages for architectural navigation
- This split is important: graph for machine reasoning, markdown for human and
  agent re-entry.

### Retrieval and compression

- `tools/context.py` explicitly targets "ultra-compact context".
- `get_minimal_context()` compresses the repository into a small starter
  packet:
  - graph stats
  - risk from changed files
  - top affected functions
  - top communities
  - top critical flows
  - next-tool suggestions
- This is one of the clearest examples in the set of context compression being
  treated as a first-class product feature, not an afterthought.

### Reusable strengths

- Very strong candidate for repo-native project orientation:
  the agent should not start from raw files when a graph summary can provide
  immediate structure.
- The markdown memory pattern is simple but useful: save high-value Q&A back
  into the repo-adjacent memory folder so later sessions can re-ingest it.
- Wiki generation from communities is highly reusable for project-level memory:
  a durable "architecture notebook" can be regenerated from current structure,
  avoiding stale hand-written architecture docs.

### Limits for our target system

- The memory layer is intentionally lightweight and does not yet model
  long-lived project decisions, experiment history, or contradiction lineage.
- It is strongest at code-structure understanding, weaker at cross-session
  research-state compounding.

## DiffMem

### Design goal

- Treat memory as a git-native repository that can be explored through the same
  primitives developers already use to understand project history:
  `git log`, `git diff`, `git blame`, `git show`, and constrained shell tools.
- Make retrieval agentic: the retriever decides which commands to run rather
  than relying on one-shot embedding recall.

### Core data model

- The retrieval agent reasons over a memory repository through a single
  `run(command="...")` tool.
- Output is a structured `RetrievalPlan` containing:
  - content pointers
  - reasons
  - priority
  - estimated token cost
  - optional git command provenance
- The writer agent updates markdown memory files and stages them in git,
  turning memory evolution into versioned repository state.

### Storage and indexing

- Storage is plain files in a git repository, not a database.
- Indexing is effectively deferred to:
  - git history
  - file paths
  - shell search
  - LLM-driven command selection
- `command_router.py` is important: it whitelists commands and subcommands,
  caps output, and separates execution from presentation.

### Retrieval and governance

- Retrieval is multi-turn and budget-aware.
- The command router allows read-only exploration while constraining blast
  radius:
  - whitelisted commands only
  - whitelisted git subcommands only
  - timeout and truncation guards
  - binary-output guard
- This gives DiffMem a distinctive quality: memory retrieval is not a
  similarity search over opaque blobs but an explicable exploration over repo
  state transitions.

### Reusable strengths

- Best repo-evolution-native idea in the set.
- Strong fit for project memory that must answer questions like:
  - when did this decision appear?
  - what replaced it?
  - which file or branch introduced this behavior?
  - what used to be true but is no longer true?
- The structured pointer output is ideal for a bounded context loader: the
  retriever returns what to load, not the full payload.

### Limits for our target system

- DiffMem is strong on temporal retrieval, but weaker on higher-order project
  ontology such as claims, experiments, ADRs, or active task state.
- It still needs a semantic project layer above the raw git/file substrate.

## Ars Contexta

### Design goal

- Generate a full persistent knowledge system from conversation instead of
  shipping a fixed memory template.
- Separate durable knowledge, agent identity, and operational scaffolding into
  distinct spaces so the system can scale without self-pollution.

### Core data model

- The central architectural primitive is the three-space split:
  - `self/`: persistent agent mind / identity / goals
  - `notes/`: durable knowledge graph
  - `ops/`: operational coordination, sessions, queue, health, observations
- The docs emphasize that these spaces have different durability, growth, and
  load patterns, and that conflating them leads to predictable failures.
- The repo also formalizes promotion paths:
  - session observations -> durable notes
  - operational learnings -> methodology
  - temporary state stays in ops instead of polluting durable knowledge

### Storage and indexing

- Storage is plain markdown with wiki links, MOCs, schemas, and directory
  semantics.
- The system is filesystem-native, but more architected than a simple folder of
  notes because it defines:
  - session lifecycle
  - evolution lifecycle
  - methodology folder
  - session capture
  - reseed / rethink mechanisms
- Optional semantic search exists, but the core design does not depend on it.

### Retrieval and maintenance

- Retrieval is designed around progressive disclosure rather than global load:
  - fully load self/orientation state
  - progressively traverse notes through MOCs and links
  - target ops state only when relevant
- The most useful implementation idea here is not ranking but separation:
  by giving `ops/` its own space, the system avoids mixing temporary project
  state with long-term knowledge.
- The evolution docs further add:
  - observation capture
  - methodology self-knowledge
  - reseed triggers
  - condition-based maintenance instead of schedule-based maintenance

### Reusable strengths

- One of the strongest ontologies in the whole study.
- The three-space architecture maps almost perfectly onto the user's target:
  we need a native distinction between agent identity, project durable memory,
  and transient execution coordination.
- The explicit discussion of failure modes is especially valuable for avoiding
  repo chaos and memory contamination.

### Limits for our target system

- Ars Contexta is closer to a generated knowledge-work vault than a code-agent
  repo runtime.
- It needs additional codebase-native structure:
  symbols, branches, patches, experiments, evaluation artifacts, and cleanup
  policy for duplicated files.

## Sekha

### Design goal

- Provide persistent markdown memory plus hard enforcement of dangerous
  tool-call policies.
- Move beyond "remember the rule" to "deny the tool call at the hook
  boundary."

### Core data model

- Memory is stored under `~/.sekha/` in fixed categories:
  - `sessions`
  - `decisions`
  - `preferences`
  - `projects`
  - `rules`
- Rules are markdown files with structured frontmatter:
  - severity
  - triggers
  - matches
  - regex pattern
  - priority
- This means both memory and governance policy are inspectable, grep-friendly,
  and versionable.

### Storage and indexing

- `storage.py` is more important than it first appears:
  - atomic writes
  - cross-process file locks
  - deterministic path generation
  - restricted frontmatter parser for stable diffs
- Search is stdlib-only and scores by term frequency, recency decay, and
  filename bonus.
- Rule loading is cache-aware and filtered by hook event and tool name.

### Hook and enforcement path

- The strongest differentiator is `hook.py`.
- Sekha reads `PreToolUse` events, loads scoped rules, and can:
  - deny a tool call
  - inject warning context
  - fail open safely if internal errors occur
- It even includes:
  - lazy imports for cold-start budget
  - kill switch after repeated failures
  - benchmark tooling for hook latency
- This is directly relevant to the user's goal of preventing repository chaos
  and destructive multi-agent divergence.

### Reusable strengths

- Excellent governance primitive for native code agents:
  memory alone is not enough; the system needs enforceable repository rules.
- The three-process model is also strong:
  MCP server, hook process, and CLI share filesystem state without direct IPC.
- File-based state plus hook-level enforcement is very aligned with a local CLI
  environment.

### Limits for our target system

- Sekha's memory semantics are intentionally simple.
- It is not trying to model experiments, claims, research trajectories, or
  repo evolution in depth.
- Best reused as the repo-governance and safety layer, not the full project
  memory system.

## ARIS

### Design goal

- Persist the entire research lifecycle to project files so long-running,
  compaction-prone agent sessions can recover state from disk rather than from
  chat history.
- Combine workflow outputs, research wiki, contracts, manifests, and pipeline
  status into a disciplined project operating system.

### Core data model

- ARIS uses a layered project file model:
  - `CLAUDE.md` as dashboard / pipeline status
  - stage directories (`idea-stage/`, `refine-logs/`, `review-stage/`, `paper/`)
  - `findings.md` for discovery log
  - `MANIFEST.md` for output tracking
  - `research-wiki/` for durable structured research knowledge
- `research_wiki.py` defines four durable entity types:
  - papers
  - ideas
  - experiments
  - claims
- It also defines typed graph edges in `graph/edges.jsonl`.

### Storage and indexing

- Storage is explicitly project-root-native and markdown-heavy.
- `research-wiki/` is particularly important because it separates:
  - append-only log
  - gap map
  - compressed query pack
  - typed entity pages
  - edge list
- The `query_pack.md` builder is a notable implementation detail:
  it compresses the wiki into a fixed-budget ideation-ready briefing, with
  failed ideas intentionally prioritized to prevent repetition.

### Recovery and repo hygiene

- The Session Recovery guide centers the project `CLAUDE.md` `Pipeline Status`
  block as a 30-second recovery surface.
- The Project Files guide further separates:
  - active contract
  - experiment tracker
  - experiment log
  - findings
  - latest-vs-timestamped outputs
- The output versioning protocol is especially relevant to the user's repo
  cleanliness requirement:
  - every overwrite-prone artifact gets a timestamped archive
  - a fixed-name latest copy remains the read target for downstream skills
  - append-only and dashboard files stay single-source
- This is one of the clearest anti-chaos file-governance systems in the set.

### Reusable strengths

- Best research-project memory design among the repos studied so far.
- Strong separation between:
  - current execution state
  - durable knowledge
  - append-only history
  - active working contract
- The research wiki gives a strong starting ontology for paper/idea/experiment/
  claim memory.
- The manifest and timestamp protocol are directly reusable for preventing
  multiple confusing versions from silently accumulating.

### Limits for our target system

- ARIS is very strong on research workflow state, but less codebase-structural
  than code-review-graph and less git-history-native than DiffMem.
- It needs a deeper coupling to code graph, branch evolution, and active coding
  memory runtime to become a full native code agent CLI.

## Cross-Repo Conclusions

### What Layer B teaches us

- Project memory cannot be one store. We need at least four repo-facing planes:
  - structural code graph memory
  - repo evolution memory
  - operational project state
  - governance / enforcement state
- `code-review-graph` contributes structure compression.
- `DiffMem` contributes repo-history-native retrieval.
- `Ars Contexta` contributes space separation and promotion rules.
- `Sekha` contributes enforceable repo-governance hooks.
- `ARIS` contributes research/project file discipline, recovery surfaces, and
  anti-chaos versioning.

### Native design implications for our target system

- A serious project-memory CLI should not only "remember facts"; it must also:
  - know current project stage
  - know active contract / branch / experiments
  - reconstruct architecture quickly
  - retrieve historical decisions from git-native memory
  - enforce repository safety and anti-chaos rules at tool boundaries
  - separate transient ops state from durable knowledge
  - maintain a compressed project briefing for fresh-session recovery

### Open questions for Layer C

- How should multiple agents share these memory planes without corrupting each
  other?
- Which agent owns debate records, synthesis, and final decisions?
- How should experiment outcomes update claim memory and planning state
  automatically?
- What is the right handoff format between fresh subagents and persistent
  project memory?
