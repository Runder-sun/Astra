# Layer C - Multi-Agent and Research Integration Deep Study

This document records design and implementation findings for systems that wire
memory into multi-agent orchestration, research loops, review loops, and
experiment-driven self-improvement.

## Repos In Scope

- `reference_repos/memory_palace_ecosystem/multi-agent-ralph-loop`
- `reference_repos/requested/hermes-agent`
- `reference_repos/requested/claw-code`
- `reference_repos/requested/openclaw`
- `reference_repos/requested/ARIS`
- `reference_repos/research_agents/AutoResearchClaw`
- `reference_repos/requested/auto-research`
- `reference_repos/research_agents/autoresearch`
- `reference_repos/research_agents/AutoSOTA`

## Findings

### Status

- First implementation pass complete

## multi-agent-ralph-loop

### Design goal

- Extend Claude Code into a parallel-first multi-agent framework with explicit
  quality gates, learned rules, and memory-palace-inspired wake-up context.
- Treat orchestration as a runtime discipline rather than an optional skill.

### Core orchestration model

- Ralph combines several layers:
  - parallel teammate roles
  - mandatory decomposition for tasks above a complexity threshold
  - blocking quality/security gates
  - continuous learning into a local/global rule taxonomy
- The repo is especially opinionated about the control loop:
  every non-trivial task should be decomposed, delegated, validated, then
  written back into memory/rules.

### Memory and learning integration

- Ralph's four-layer wake-up model mirrors MemPalace ideas:
  - `L0` identity
  - `L1` essential rules
  - `L2` project learned rules
  - `L3` full vault search
- The interesting addition is the learned-rules taxonomy:
  halls / rooms / wings across type, topic, and scope.
- It also explicitly records the failure of "encoding for token savings" and
  concludes that rule selection/filtering matters more than compression.

### Multi-agent runtime design

- Teammates are role-specific and intended for parallel use:
  coder, reviewer, tester, researcher, frontend, security.
- The orchestrator is not a vague "manager agent"; it is coupled to:
  - Aristotle-style first-principles analysis
  - task classification
  - hook-triggered quality gates
  - plan/task synchronization
- This makes Ralph one of the strongest examples of memory being wired into
  execution policy, not merely retrieval.

### Reusable strengths

- Strongest explicit "parallel-first" doctrine in the set.
- Very relevant lesson for our target system: multi-agent work needs
  gatekeeping primitives, not just agent spawning.
- The learned-rules pipeline is a promising bridge between episodic outcomes
  and procedural memory.

### Limits for our target system

- Ralph is strong on coding-task orchestration, but less explicit about
  research artifacts like hypotheses, claims, papers, and experiment lineage.
- Its memory is more rules-centric than project-state-centric.

## Hermes Agent

### Design goal

- Build a general-purpose, self-improving agent that lives across CLI and
  messaging platforms, with persistent memory, skill creation, delegation, and
  portable runtime backends.
- Make memory and skill evolution part of the agent loop rather than
  separate add-ons.

### Core orchestration model

- Hermes is fundamentally a unified agent runtime:
  - one main agent loop
  - many execution backends
  - skills hub / skill guard
  - cron automation
  - optional subagent delegation and RPC-style tool use
- The system is less "research pipeline shaped" than ARIS or AutoResearchClaw,
  but stronger as a reusable general substrate.

### Memory and context design

- `MemoryManager` is a strong implementation artifact:
  - built-in memory always on
  - at most one external memory provider
  - unified prefetch / sync / tool-routing interface
- This is a pragmatic anti-chaos decision: multiple memory backends create tool
  bloat and semantic conflicts.
- Hermes also separates:
  - static provider prompt blocks
  - prefetch recall
  - post-turn sync
  - end-of-session hooks
  - delegation observations
- `ContextEngine` is similarly abstracted: compaction is a pluggable engine,
  not hardcoded behavior.

### Reusable strengths

- Excellent runtime abstraction for a future native code/research CLI.
- The provider/engine split is especially reusable:
  memory provider and context engine should be separate extension points.
- Hermes is also one of the clearest examples of skill lifecycle management as
  a product surface, not a hidden internal hack.

### Limits for our target system

- Hermes is broad and production-oriented, but not specifically optimized for
  project-structured research workflows.
- It needs stronger repo-native and paper/experiment-native ontology above the
  general memory/provider substrate.

## Claw Code

### Design goal

- Provide a public agent harness implementation whose real product lesson is
  autonomous software development through coordinated agent labor rather than a
  single interactive CLI.
- Emphasize system-level coordination: direction by human, labor by claws.

### Core orchestration model

- The repo philosophy is unusually explicit:
  Claw Code is evidence of a coordination system, not just a binary.
- Its surrounding worldview includes:
  - human gives direction
  - coordination layer decomposes and routes work
  - agent roles execute/review/retry
  - status routing stays out of the main coding context
- The Rust runtime adds serious operational primitives:
  - per-worktree session stores
  - session forking
  - recovery recipes
  - branch/worktree safety
  - compacted-session health probes

### State and recovery design

- `session_control.rs` namespaces sessions by workspace fingerprint so parallel
  worktrees do not collide.
- `summary_compression.rs` is particularly relevant: it compresses summaries by
  prioritizing structural lines (`Scope`, `Current work`, `Pending work`,
  `Key files`, etc.), not by naive truncation.
- This is a meaningful design insight for our target system:
  compression should preserve control-state fields, not only semantic gist.

### Reusable strengths

- Very strong session/worktree isolation model.
- Strong idea of treating recovery as a first-class subsystem rather than a
  best-effort resume flag.
- Valuable for project-memory CLI because session forks, alternative branches,
  and stale-state detection are core needs.

### Limits for our target system

- Claw Code itself is more harness/runtime than research workflow.
- It suggests the execution substrate but does not define a research-memory
  ontology or claim-driven experiment loop.

## OpenClaw

### Design goal

- Build a local-first personal assistant gateway spanning channels, sessions,
  tools, and device surfaces.
- Support multiple isolated agents and background tasks without overcommitting
  to hierarchical manager trees.

### Core orchestration model

- OpenClaw's most relevant contribution here is architectural restraint.
- It supports:
  - multi-agent routing
  - spawned sessions/tasks
  - background task registry
  - per-session sandboxing
- But its vision explicitly rejects default "manager-of-managers" hierarchy as
  a core architecture.
- This is an important counterweight to overbuilt multi-agent systems.

### Task and session design

- Memory is a special one-provider slot, similar in spirit to Hermes.
- The task subsystem is stronger than the README first suggests:
  - sqlite-backed task registry
  - task-flow registry
  - owner-scoped access
  - explicit delivery policies
  - suppression of duplicate terminal updates
- It also distinguishes task runtimes such as ACP and subagent, and applies
  different delivery semantics to them.

### Reusable strengths

- Strong background-task accounting model.
- Good evidence that a production CLI should track spawned work as explicit
  task records/flows, not implicit chat turns.
- The anti-overhierarchy stance is useful: multi-agent support should exist,
  but not force every workflow through a brittle agent-tree bureaucracy.

### Limits for our target system

- OpenClaw is productized as a personal assistant platform more than a focused
  research/code agent harness.
- It needs more explicit claim/experiment/document semantics for our target.

## ARIS

### Design goal

- Turn research into composable markdown workflows with cross-model execution
  and review, while keeping all state inspectable on disk.
- Treat the workflow as the product and the platform as replaceable.

### Core orchestration model

- ARIS is fundamentally a workflow harness, not a runtime kernel.
- Its orchestration centers on:
  - executor/reviewer role separation
  - workflow chaining through file artifacts
  - hard rules for reviewer independence
  - progressively richer memory via research wiki and meta-optimize
- The most important control idea is that executor and reviewer must be
  different model families whenever possible.

### Multi-agent / multi-model discipline

- The shared references are more valuable than they first appear.
- `reviewer-independence.md` forbids:
  - executor summaries
  - leading interpretations
  - pre-digested strengths/weaknesses
  - prior-round coaching
- `review-tracing.md` requires raw prompt/response trace capture for audit.
- `effort-contract.md` standardizes workload intensity while keeping review
  rigor non-negotiable.
- This is one of the strongest governance layers for adversarial collaboration
  in the entire repo set.

### Reusable strengths

- Best explicit cross-model debate / critique protocol in the study.
- Strongly relevant to the user's goal of multi-agent debate producing better
  research direction and stronger validation.
- ARIS also demonstrates that process discipline can live in markdown protocol
  files and still be operationally effective.

### Limits for our target system

- ARIS is workflow-rich but runtime-light; many guarantees depend on the agent
  following the markdown protocol.
- It benefits from being paired with a stronger native execution substrate such
  as Hermes / Claw / OpenClaw style session/task machinery.

## AutoResearchClaw

### Design goal

- Convert a single topic into a paper through a long-form autonomous pipeline
  with experiments, peer review, repair, branching, and optional human
  collaboration.
- Blend pipeline structure with self-evolution and persistent research memory.

### Core orchestration model

- AutoResearchClaw is the most pipeline-explicit repo in this layer:
  23 stages across scoping, literature, synthesis, experiment design,
  execution, analysis, writing, review, and export.
- It also adds:
  - gate stages
  - pivot/refine decisions
  - branch exploration
  - multi-agent review dialog
  - human intervention modes

### Memory, knowledge, and evolution

- `knowledge/base.py` writes stage outputs into categorized markdown memory:
  questions, literature, experiments, findings, decisions, reviews.
- `evolution.py` extracts lessons from:
  - failed stages
  - blocked stages
  - pivot/refine decisions
  - runtime anomalies
- Lessons are categorized, time-stamped, and later turned into prompt overlays.
- The config also exposes:
  - persistent memory injection points
  - knowledge graph storage
  - lesson-to-skill conversion
- This is one of the strongest examples of memory being updated directly from
  pipeline outcomes.

### Reusable strengths

- Strongest closed-loop research pipeline in the set.
- Particularly valuable for our target are:
  - claim/evidence-aware writing prompts
  - repair loops between failed experiments and later decisions
  - branch manager for exploring alternatives without losing prior state
  - evolutionary lessons that feed later stages

### Limits for our target system

- The design is ambitious and broad; risk of complexity creep is real.
- It still needs stronger native repo cleanliness / file-governance rules than
  ARIS, and stronger local CLI runtime substrate than Hermes/Claw/OpenClaw.

## OpenAGS (`auto-research`)

### Design goal

- Build an open autonomous generalist scientist where each folder is an agent,
  each `SOUL.md` defines its role, and the workflow is coordinated through
  explicit file protocols rather than opaque runtime state.
- Unify builtin agents and external CLI agents behind the same project shell.

### Core orchestration model

- OpenAGS has one of the clearest agent communication protocols in the set:
  - coordinator writes `DIRECTIVE.md`
  - subagent reads `DIRECTIVE.md`
  - subagent writes `STATUS.md`
  - Node.js orchestrator watches filesystem changes and dispatches again
- This is backed by real parser/orchestrator code, not only docs.
- The `WorkflowOrchestrator` tracks:
  - per-agent state
  - per-agent timeouts
  - pause/resume
  - pending triggers
  - refine/pivot counts
  - per-module provider session IDs

### State, protocol, and recovery

- The TypeScript parser uses multi-layer fallback:
  - frontmatter parse
  - regex extraction
  - heuristics
  - synthesized failure state
- This is an excellent robustness pattern: markdown protocol is great, but
  orchestration must survive malformed status files.
- OpenAGS also standardizes:
  - agent-local memory files
  - upstream/downstream path contracts
  - provider session resume
  - browser/Electron shared orchestration layer

### Reusable strengths

- Best explicit file-protocol orchestration model in the set.
- Very aligned with the user's desire for inspectable, operable multi-agent
  workflows.
- "Folder = agent" is a strong idea for research projects because it keeps
  workspaces, memory, and outputs naturally separated.

### Limits for our target system

- OpenAGS is elegant, but somewhat more doc/protocol-heavy than runtime-hard.
- It needs stronger native repo-history integration and quality-review doctrine
  than ARIS/AutoResearchClaw.

## Karpathy `autoresearch`

### Design goal

- Let an agent autonomously improve a compact ML training setup overnight by
  editing code, running a fixed-time experiment, and keeping only wins.
- Make the "research organization" itself editable through `program.md`.

### Core orchestration model

- This is not a multi-agent system in code, but it is deeply relevant as an
  outer-loop pattern:
  - baseline
  - propose change
  - commit
  - run fixed-budget experiment
  - measure
  - keep or revert
  - repeat indefinitely
- The key design choice is environmental control:
  one file to edit, one metric, one time budget, one branch.

### Reusable strengths

- Strongest minimal closed-loop experimentation pattern.
- Important lesson for our target system:
  many research loops should be cast as small explicit optimization problems
  instead of giant all-knowing agent plans.
- The use of `program.md` as "research org code" is also highly compatible with
  markdown-native agent systems.

### Limits for our target system

- Not a full multi-agent or project-memory system.
- Best reused as a design pattern for local search / grid search / branch-based
  autonomous experimentation.

## AutoSOTA

### Design goal

- Curate and publish the outputs of many automated code-optimization runs over
  research repositories.
- Emphasize result tracking and per-paper optimization deltas.

### What it contributes here

- AutoSOTA is less an agent runtime than an output ledger of large-scale
  automated optimization.
- Its main relevance is conceptual:
  the system organizes work per paper/repo, keeps patches and summaries, and
  foregrounds measurable improvement deltas.
- This reinforces a design lesson:
  large autonomous research systems need durable run ledgers and per-target
  optimization dossiers, not just ephemeral chats.

### Limits for our target system

- AutoSOTA itself is not the orchestration engine we want to copy.
- It is more useful as evidence of the importance of result ledgers, patch
  histories, and benchmark-facing evaluation summaries.

## Cross-Repo Conclusions

### What Layer C teaches us

- Multi-agent systems need four different control surfaces:
  - execution substrate
  - task/state protocol
  - critique/governance protocol
  - long-horizon improvement loop
- `Hermes`, `Claw Code`, and `OpenClaw` contribute the execution substrate:
  sessions, tasks, compaction, recovery, background work, pluggable memory.
- `OpenAGS` contributes the best explicit multi-agent filesystem protocol.
- `ARIS` contributes the strongest adversarial review discipline.
- `AutoResearchClaw` contributes the deepest research pipeline with memory and
  evolutionary overlays.
- `autoresearch` contributes the strongest minimal local-search loop.

### Native design implications for our target system

- Our target CLI should probably NOT be a single monolithic agent.
- It should combine:
  - Claw/Hermes-style session/task substrate
  - OpenAGS-style agent directory protocol
  - ARIS-style reviewer independence and traceability
  - AutoResearchClaw-style stage memory + lessons + pivot/refine decisions
  - autoresearch-style small-loop experiment search where applicable
- Debate records, synthesis decisions, and branch comparisons should be durable
  first-class artifacts, not hidden inside conversation history.

### Emerging design principle

- The best systems in this layer do not treat multi-agent as "many chatbots."
- They treat it as:
  - explicit roles
  - explicit artifacts
  - explicit resume state
  - explicit failure handling
  - explicit critique independence
  - explicit memory writeback after outcomes
