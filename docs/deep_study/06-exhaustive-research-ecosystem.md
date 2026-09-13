# Exhaustive Research Ecosystem Pass

This document records the second-phase exhaustive pass over supplementary
research-agent, framework, and skill-pack repos that were not fully included in
the first mainline study.

## Goal

- Find orchestration and research-loop patterns missing from the mainline pass
- Distinguish runtime kernels from workflow kits, paper demos, and repo lists
- Refine the eventual final architecture with stronger research-system coverage

## Repos In Scope

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

## Status

- In progress

## What to extract

For each repo:

1. Design goal
2. Agent topology or workflow topology
3. State and artifact protocol
4. Experiment / search / review loop design
5. Long-horizon self-improvement or cross-run accumulation
6. Repo/project hygiene mechanisms
7. Reusable strengths
8. Limits for the target native project-memory CLI

## Expected decision outputs

- Which repos materially change the multi-agent and research-workflow design
- Which repos should influence only the skill-pack layer
- Which repos are useful primarily as comparative references or literature maps

## Triage

### Highest-value architecture sources

- `reference_repos/research_agents/AI-Scientist`
- `reference_repos/research_agents/AgentLaboratory`
- `reference_repos/research_agents/AutoResearch-SibylSystem`
- `reference_repos/research_agents/OpenHands`
- `reference_repos/research_agents/gpt-researcher`
- `reference_repos/research_agents/open_deep_research`

### Useful framework reference

- `reference_repos/research_agents/autogen`

### Still pending in later pass

- `reference_repos/research_agents/autonomous-researcher`
- `reference_repos/skill_packs/AI-Research-SKILLs`
- `reference_repos/lists/awesome-autoresearch`
- `reference_repos/lists/deepresearch`

## Findings

## AI-Scientist

### Design goal

- Automate a full scientific discovery loop from idea generation to
  experimentation, paper writing, review, and improvement.

### Core orchestration model

- The main launch path is explicit and sequential:
  - generate ideas
  - novelty check
  - perform experiments
  - perform writeup
  - perform review
  - perform improvement
- The workflow is organized per idea folder under a results directory.

### Reusable strengths

- Strongest supplementary evidence for "idea as unit of execution."
- The per-idea artifact directory pattern is highly relevant for branch-style
  research tracking.
- Review followed by improvement followed by re-review is a useful paper-loop
  pattern.

### Limits for our target system

- Template-centric and experiment-domain-centric.
- Less focused on repo cleanliness, project memory, or multi-agent state
  protocol than our target requires.

## AgentLaboratory

### Design goal

- Serve as an end-to-end research assistant that helps a human researcher move
  from literature review to experimentation to report writing.

### Core orchestration model

- AgentLaboratory is structured around three phases:
  - literature review
  - experimentation
  - report writing
- It explicitly stays in a human-assistant posture rather than full autonomy.
- It also introduces AgentRxiv:
  a mechanism for agents to upload, retrieve, and build on each other's
  research.

### Reusable strengths

- Strong supplementary evidence for multi-project cumulative research memory.
- AgentRxiv is especially important conceptually:
  agents should build on prior agent outputs, not only their own local run.
- Good reminder that not every useful research system must be fully autonomous.

### Limits for our target system

- Less operationally strict than ARIS or OpenAGS on file/state protocol.
- More valuable as a research-lifecycle reference than as a runtime kernel.

## AutoResearch-SibylSystem

### Design goal

- Build a fully autonomous AI scientist native to Claude Code, with large
  multi-agent teams, GPU scheduling, iterative quality gates, and outer-loop
  self-evolution.

### Core orchestration model

- The repo centers on a 19-stage state machine.
- It uses:
  - large agent teams
  - debate stages
  - GPU scheduling
  - iteration and pivot logic
  - background sync
  - cross-project evolution overlays
- Debate is not decorative:
  the docs explicitly call out multi-agent idea debate and result debate.

### Reusable strengths

- One of the strongest supplementary sources for long-horizon autonomous
  research orchestration.
- Cross-project lesson overlays are especially relevant to the user's request
  for native, accumulating project/research memory.
- GPU scheduling and stage checkpointing are highly relevant for practical
  experiment loops.

### Limits for our target system

- Very ambitious and potentially overbuilt.
- The final architecture should absorb its outer-loop lessons and stage
  checkpointing without inheriting unnecessary complexity or Claude-specific
  assumptions.

## OpenHands

### Design goal

- Provide a broad AI-driven software development platform spanning SDK, CLI,
  local GUI, cloud, and enterprise deployment.

### Core orchestration model

- OpenHands is not a research workflow system.
- Its value here is as a runtime/platform reference:
  - composable SDK
  - CLI and GUI surfaces
  - sandbox/runtime abstraction
  - skills/microagents
  - repository-specific local instructions

### Reusable strengths

- Strong supplementary evidence for separating:
  - core SDK/runtime
  - CLI surface
  - GUI/cloud product layers
- Repository-local microagents/skills are relevant to project-native agent
  extensions.
- Sandbox/runtime abstraction matters for a future serious code-agent CLI.

### Limits for our target system

- OpenHands is broad platform infrastructure, not a project-memory or research
  memory design.
- It should influence runtime packaging and extensibility more than core memory
  ontology.

## AutoGen

### Design goal

- Provide a general framework for building multi-agent AI applications.

### Current relevance

- The repo is in maintenance mode.
- It remains important historically as a multi-agent framework reference, but
  its current role in this study is mostly architectural baseline/comparison.

### Reusable strengths

- Useful as evidence for canonical multi-agent conversation/team abstractions.
- Helps anchor what newer repos improved upon.

### Limits for our target system

- Too generic and too framework-oriented to drive the final architecture.
- Lower-value source than OpenAGS, ARIS, Hermes, Claw, or Sibyl for the
  specific target system here.

## GPT Researcher

### Design goal

- Produce factual, cited research reports through planner/execution/publisher
  style deep research over web and local sources.

### Core orchestration model

- The README makes the architecture explicit:
  - planner generates research questions
  - execution agents gather relevant information
  - publisher aggregates findings into a report
- The project emphasizes:
  - parallelized agent work
  - source tracking
  - deterministic/stable research reports

### Reusable strengths

- Strong supplementary evidence for planner/executor/publisher separation.
- Useful for the final research skill pack, especially in the survey and report
  generation stages.
- Maintains memory/context through the research process, which is relevant to
  our research-memory plane.

### Limits for our target system

- Strong on deep research reporting, weaker on repo-memory and coding-project
  governance.
- Better used as a research workflow component than as the system kernel.

## Open Deep Research

### Design goal

- Provide a configurable open deep research agent built on LangGraph, with
  multiple provider/search/MCP options and benchmark-facing evaluation.

### Core orchestration model

- The repo has:
  - main LangGraph implementation
  - configurable models for summarization/research/compression/final report
  - legacy plan-and-execute and supervisor-researcher variants
  - built-in evaluation against Deep Research Bench
- This is one of the clearest supplementary examples of research workflow being
  treated as a configurable graph rather than a fixed script.

### Reusable strengths

- Strong evidence for making research pipelines configurable by:
  - model role
  - search tool
  - concurrency
  - MCP configuration
- Benchmark-facing evaluation support is highly relevant for the final design's
  research-loop verification layer.

### Limits for our target system

- More focused on research synthesis/reporting than on project memory or repo
  hygiene.
- Best reused as a configurable research workflow pattern.

## Autonomous Researcher

### Design goal

- Take a high-level research objective, decompose it into experiments, run
  specialist agents with GPU-backed sandboxes, and synthesize a paper-style
  report.

### Core orchestration model

- The repo centers on an explicit orchestrator / worker split:
  - orchestrator decomposes the problem into hypotheses
  - workers run as separate researcher agents
  - each worker gets its own persistent Modal sandbox
  - orchestrator can launch multiple waves and then synthesize a paper
- It also emits structured frontend events, so the UI is not scraping raw
  terminal text blindly.

### Reusable strengths

- Strong evidence for "hypothesis as dispatch unit" instead of only "task as
  dispatch unit."
- The separate-process worker model is useful for real multi-agent isolation.
- Persistent per-agent sandboxes are especially relevant for research/code
  agents that need stateful experimental environments across tool calls.
- Good evidence that structured event emission should be first-class, not an
  afterthought.

### Limits for our target system

- Still early and harness-like.
- Strong on experiment delegation, weaker on repo governance, durable project
  memory, and anti-chaos artifact management.

## AI-Research-SKILLs

### Design goal

- Provide a large skill library that covers the full AI research lifecycle,
  with one top-level autoresearch orchestrator routing into many specialized
  domain skills.

### Core design contributions

- This is not just a single orchestration skill:
  the repo actually contains a large domain tree with dozens of concrete
  `SKILL.md` files across architecture, training, evaluation, inference,
  agents, RAG, multimodal, MLOps, ideation, and paper writing.
- The top-level `autoresearch` skill is notable because it formalizes:
  - a two-loop architecture
  - persistent state files
  - `findings.md` as research memory
  - wall-clock continuity loops
  - human-facing progress presentations
  - routing into domain-specific skills

### Reusable strengths

- Strong evidence that the final system should separate:
  - orchestration skills
  - domain execution skills
  - paper/reporting skills
  rather than treating "research" as one monolithic prompt.
- `findings.md` as both narrative and agent memory is especially relevant to
  the target system's research-memory plane.
- The two-loop framing is one of the clearest skill-pack realizations in the
  studied repos.
- The repo also confirms that a serious research agent needs a large, explicit
  capability library, not only a planner.

### Limits for our target system

- Very broad and somewhat library-like.
- It contributes more to skill-pack decomposition and workflow doctrine than to
  low-level runtime architecture.

## Awesome Autoresearch

### Design goal

- Curate the rapidly growing landscape of autoresearch descendants, research
  agents, ports, domain adaptations, and benchmarks.

### Core design contributions

- The repo is valuable less as a runtime and more as a map of the design
  space.
- It makes several trends very visible:
  - autoresearch loops are spreading beyond model training
  - multi-agent and swarm variants are proliferating
  - persistent memory variants are emerging
  - benchmark-driven evaluation is becoming a first-class concern

### Reusable strengths

- Strong evidence that the final system should be benchmark-aware from day one.
- Useful for checking that the final architecture is not overfitting to one
  narrow autoresearch lineage.
- Confirms that keep-or-revert loops, measurable goals, swarm coordination, and
  persistent memory are now mainstream patterns in this design family.

### Limits for our target system

- It is a landscape map, not an implementation substrate.
- Best used as completeness pressure and trend validation, not as a direct
  blueprint.

## Deepresearch

### Design goal

- Act as a broad project and paper list for deep-research systems,
  methodologies, products, and benchmarks.

### Core design contributions

- This repo is not deep in code, but it widens the taxonomy substantially:
  - agent frameworks
  - workflows
  - multimodal agent UIs
  - academic search tools
  - evaluation papers and benchmarks
- Its paper list is especially useful for reminding us that deep research is
  not only about agent loops, but also about evaluation, human factors, and
  scientific-discovery framing.

### Reusable strengths

- Good pressure test for missing categories in the final architecture.
- Reinforces that a production-grade research agent should account for:
  - workflow engines
  - academic search surfaces
  - evaluation methodology
  - multimodal interfaces

### Limits for our target system

- It is a catalog, not a deeply opinionated implementation source.
- Best used to widen coverage and avoid blind spots.

## Cross-repo supplementary conclusions

### What this exhaustive research supplementary pass adds

- `AI-Scientist` reinforces idea-centric research execution and review/improve
  loops.
- `AgentLaboratory` reinforces human-assisted research workflows and
  cross-project cumulative knowledge through AgentRxiv.
- `Sibyl` reinforces long-horizon autonomous iteration, debate teams, GPU
  scheduling, and outer-loop self-evolution.
- `OpenHands` reinforces modular runtime/platform separation and repo-local
  skills/microagents.
- `AutoGen` serves mainly as historical/general framework baseline.
- `GPT Researcher` reinforces planner/executor/publisher report generation with
  source tracking.
- `Open Deep Research` reinforces configurable graph-shaped deep-research
  workflows with explicit evaluation infrastructure.
- `Autonomous Researcher` reinforces hypothesis-level delegation, persistent
  per-agent sandboxes, and structured event streams.
- `AI-Research-SKILLs` reinforces the need for a large explicit research skill
  tree plus a two-loop autoresearch orchestrator.
- `Awesome Autoresearch` reinforces the importance of measurable-goal loops,
  benchmark awareness, swarm variants, and persistent-memory descendants as a
  broader design family.
- `deepresearch` reinforces the wider taxonomy around workflow systems,
  academic search tools, multimodal interfaces, and evaluation methodology.

### Net effect on the final architecture

- The final architecture should strengthen its research workflow plane with:
  - idea-centric execution units
  - planner/executor/publisher decomposition
  - explicit benchmark/evaluation hooks
  - cross-project cumulative research memory
  - debate teams for idea/result analysis
  - GPU-aware experiment scheduling
  - configurable graph-style workflow execution
  - explicit split between orchestration skills and domain skills
  - persistent findings/narrative memory during long-running research
  - hypothesis-level worker dispatch with sandbox isolation
