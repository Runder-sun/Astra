# Native Project-Memory Multi-Agent CLI Deep Study Plan

**Goal:** Deep-read the relevant repos across memory core, repo memory,
multi-agent orchestration, research automation, and skill ecosystems, then
document concrete design and implementation patterns that can be fused into a
native project-memory code/research agent.

**Approach:** Use a two-phase process.

- Phase 1: mainline three-layer study over the most architecture-defining repos
- Phase 2: exhaustive supplementary pass over remaining memory, research-agent,
  framework, and skill-pack repos so the final architecture is not biased by
  only the first sample set

For each repo, focus on source files that define storage, retrieval,
consolidation, hooks, MCP interfaces, context bootstrapping, orchestration, and
artifact governance. Record both strengths and limits. Only after the
supplementary pass is complete do we write the final architecture document.

**Output Format:** Markdown documents in `docs/deep_study/`, written
incrementally after each repo pass.

---

## Study Order

### Phase 1: Mainline Repos

- Layer A, Layer B, Layer C, and first fusion pass already completed
- These are the current docs:
  - `01-layer-a-memory-core.md`
  - `02-layer-b-project-repo-memory.md`
  - `03-layer-c-multi-agent-research.md`
  - `04-fusion-patterns.md`

### Layer A: Memory Core

Target repos:

- `reference_repos/memory_palace/mempalace`
- `reference_repos/memory_palace_ecosystem/ClawMem`
- `reference_repos/memory_palace/AgentRecall`
- `reference_repos/memory_palace/memoryOSS`
- `reference_repos/memory_palace/hippo-memory`

Questions:

- What are the core memory entities?
- How is memory stored, indexed, consolidated, and recalled?
- How is context surfaced into the agent hot path?
- What mechanisms exist for forgetting, decay, or memory compression?

### Layer B: Project / Repo Memory

Target repos:

- `reference_repos/memory_palace/code-review-graph`
- `reference_repos/memory_palace/DiffMem`
- `reference_repos/memory_palace/arscontexta`
- `reference_repos/memory_palace_ecosystem/sekha`
- `reference_repos/requested/ARIS`

Questions:

- How do these systems model project context, codebase structure, or repo evolution?
- How do they avoid context bloat and duplicate files?
- What kinds of indexes, graphs, or markdown structures are used?
- Which mechanisms can become project-memory and repo-governance primitives?

### Layer C: Multi-Agent + Research Integration

Target repos:

- `reference_repos/memory_palace_ecosystem/multi-agent-ralph-loop`
- `reference_repos/requested/hermes-agent`
- `reference_repos/requested/ARIS`
- `reference_repos/research_agents/AutoResearchClaw`
- `reference_repos/research_agents/autoresearch`

Questions:

- How is memory wired into orchestration instead of staying as a passive store?
- How do agents share, debate, and consolidate findings?
- How are experiments, claims, and reviews looped back into memory?
- Which orchestration patterns fit a native code/research CLI instead of a web app?

### Phase 2: Exhaustive Supplementary Pass

This pass is required before claiming a final architecture.

#### Supplementary Memory / Context / MCP repos

Target repos:

- `reference_repos/memory/graphiti`
- `reference_repos/memory/langmem`
- `reference_repos/memory/letta`
- `reference_repos/memory/mem0`
- `reference_repos/memory_palace/Memory-Palace`
- `reference_repos/memory_palace/mcp-memory-service`
- `reference_repos/memory_palace/supermemory`
- `reference_repos/memory_palace_ecosystem/memoriki`
- `reference_repos/memory_palace_ecosystem/memory-store-plugin`

Questions:

- Which systems add truly new memory abstractions beyond the first pass?
- Which ones improve temporal memory, user modeling, contradiction handling,
  retrieval latency, or MCP integration?
- Which ones are production memory products but weak architecture sources for
  our target CLI?
- Which ones strengthen project-memory, team-memory, or shared-memory design?

#### Supplementary Research / Framework / Skill repos

Target repos:

- `reference_repos/research_agents/AI-Scientist`
- `reference_repos/research_agents/AgentLaboratory`
- `reference_repos/research_agents/AutoResearch-SibylSystem`
- `reference_repos/research_agents/OpenHands`
- `reference_repos/research_agents/autogen`
- `reference_repos/research_agents/autonomous-researcher`
- `reference_repos/research_agents/gpt-researcher`
- `reference_repos/research_agents/open_deep_research`
- `reference_repos/skill_packs/AI-Research-SKILLs`
- `reference_repos/lists/awesome-autoresearch`
- `reference_repos/lists/deepresearch`

Questions:

- Which systems add genuinely new orchestration, search, review, or
  self-improvement patterns beyond Layer C?
- Which ones are reusable runtime substrates vs. workflow catalogs only?
- Which ones improve research skill decomposition, cross-run accumulation, or
  experiment/report loops?
- Which ones should influence the final architecture only indirectly?

#### Low-signal auxiliary repos

These may be referenced for context but should not dominate the final design
unless code inspection reveals a strong unique mechanism:

- `reference_repos/memory_palace_ecosystem/openclaw-config`
- `reference_repos/memory_palace_ecosystem/awesome-ai-anatomy`

### Final Deliverable Order

1. `05-exhaustive-memory-ecosystem.md`
2. `06-exhaustive-research-ecosystem.md`
3. `07-final-architecture.md`

## Reading Template

For each repo, record:

1. Design goal
2. Core data model
3. Storage and indexing choices
4. Retrieval and ranking logic
5. Hooks, MCP, CLI, or runtime integration
6. Consolidation, reflection, or forgetting mechanisms
7. Reusable strengths
8. Limits for our target system
