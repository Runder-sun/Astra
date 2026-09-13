# Revised Final Architecture for a Native Project-Memory Multi-Agent Code Agent CLI

## Goal

Build a code-agent CLI that is already excellent as a general terminal coding
assistant, then extend it with native project memory, multi-agent debate and
branch search, repo governance, and a research skill pack.

## Core Decision

Use a from-scratch kernel. Borrow concrete mechanisms from Hermes, Claw Code,
Crush, memoryOSS, hippo-memory, ARIS, GPT Researcher, Autonomous Researcher,
Meta-Harness, Squad, AutoGen, and Ruah, but do not inherit any one repo's
architecture wholesale.

## System Shape

The system has one executable state machine and seven capability planes:

1. runtime and protocol plane
2. session and workspace plane
3. permission and sandbox plane
4. agent orchestration plane
5. memory and retrieval plane
6. repo governance plane
7. research workflow plane

Everything else is a service behind those planes.

## 1. Executable State Machine

The top-level persisted object is `KernelStateBundle`, not a chat transcript.

`ProjectState` is the project-scoped child payload inside that bundle. It is
not a second top-level authority.

### `KernelStateBundle`

Fields:

- `project_state`
- `session_state`
- `turn_state`
- `agent_state`
- `review_state`
- `branch_state`
- `memory_state`
- `repo_state`
- `artifact_state`
- `projectops_state`
- `permission_state`
- `feature_state`
- `rollback_state`
- `remote_state`
- `seq_cursor`
- `checkpoint_epoch`

### `ProjectState`

Fields:

- `project_id`
- `workspace_root`
- `current_stage`
- `stage_owner`
- `stage_mode`
- `substates`
- `active_session_id`
- `active_agent_ids`
- `artifact_families`
- `memory_index`
- `branch_batches`
- `open_reviews`
- `repo_health`
- `last_checkpoint`
- `transition_log`

### `StageState`

Stages:

- `booting`
- `ready`
- `intake`
- `planning`
- `exploring`
- `executing`
- `debating`
- `reviewing`
- `consolidating`
- `promoting`
- `compacting`
- `idle`
- `recovering`
- `shutting_down`

### `TransitionGuard`

Every stage transition checks:

- permission policy
- workspace cleanliness
- active agent status
- pending review status
- memory flush state
- branch budget
- artifact supersession status

The top-level stage is only the coordinator state.

The executable system must also maintain orthogonal substates for:

- sessions
- agents
- reviews
- branches
- memory
- repo maintenance

This is how the system can support active agents, background consolidation,
open reviews, and recovery without splitting into multiple hidden state
machines.

### `PromotionRule`

Nothing becomes canonical by accident. Promotion must explicitly decide:

- append
- fork
- supersede
- archive
- reject

## 2. Runtime And Session Plane

The runtime owns:

- CLI/TUI event loop
- tool execution
- model calls
- structured events
- compaction
- persistence
- doctor/debug/smoke commands

The same runtime must support:

- interactive chat
- one-shot prompt
- long-running project sessions
- background maintenance
- remote or local execution

The kernel must also own a cross-project registry so the system can manage
multiple workspaces instead of only the current repo.

Session persistence should follow a workspace-scoped layout, with a small
session store and a separate durable project store.

The cross-project registry is global operator state, not project runtime
authority.

Default location:

- `$XDG_STATE_HOME/research-cli/registry/`
- fallback: `~/.local/state/research-cli/registry/`

Rules:

- it stores `ProjectRegistryEntry` records, current-project pointer state, and
  recency metadata
- it may reference `data_dir`, but it never overrides the canonical runtime
  truth inside the selected project's `.pmcli/`
- `data_dir` defaults to `<workspace_root>/.pmcli` unless an explicitly
  supported detached data-root mode is configured in a later milestone

### `.pmcli/` persistence protocol

The runtime should persist under the project root:

```text
.pmcli/
├── project_state.json
├── project_meta.json
├── sessions/
│   └── <session_id>/
│       ├── transcript.jsonl
│       ├── summary.md
│       └── meta.json
├── events/
│   ├── events.jsonl
│   └── agents/
├── agents/
│   └── <agent_id>/
├── memory/
│   ├── working.jsonl
│   ├── durable.jsonl
│   ├── invalidations.jsonl
│   └── recall_cache.sqlite
├── artifacts/
│   ├── families.json
│   ├── manifests/
│   └── archive/
├── branches/
│   └── <batch_id>/
├── reviews/
│   └── <review_id>/
└── repo/
    ├── health.json
    └── cleanup_log.jsonl
```

The runtime must treat this as protocol, not incidental storage.

Bootstrap milestones may create only the subset needed by the owning milestone,
but they may not change these canonical paths.

Every canonical object/snapshot file under `.pmcli/` must carry:

- `schema_version`
- `runtime_version`
- `migration_version`
- `conformance_line`

Append-only line-record logs such as `events/events.jsonl` and
`sessions/<session_id>/transcript.jsonl` follow their line schema contracts
instead of snapshot-envelope metadata. They remain canonical, but they are not
wrapped object snapshots.

### Event model

Every important action emits a structured `KernelEventEnvelope`.

Canonical first-release `event_name` stems are frozen in
`24-kernel-state-machine-contract.md` and include:

- `command`
- `runtime_preflight`
- `repl`
- `session_open`
- `session_resume`
- `session`
- `turn`
- `provider_resolution`
- `provider_auth`
- `permission`
- `tool`
- `session_compaction`
- `project`
- `config`
- `memory_query`
- `memory_invalidation`
- `cleanup_plan`
- `cleanup_apply`
- `review_open`
- `review_retry`
- `remote_pair`
- `remote_attach`
- `remote_takeover`
- `remote_control_owner`

This event stream is the source for resume, silent summarization, and audit.

Recent reference sharpening:

- `Meta-Harness` strengthens the need for explicit run artifacts, environment
  snapshots, and bounded mutable scope
- `Squad` strengthens the need for durable, human-led team state and health
  surfaces
- `AutoGen` strengthens the runtime/orchestration/extensions separation
- `Ruah` strengthens the requirement for claim-aware worktree execution,
  takeover, and durable task artifacts

See `docs/deep_study/47-meta-harness-and-agent-team-review.md`.

## 2.5 Host, Plugin, And Setup Boundary

The system should explicitly separate:

- `CoreRuntimeAPI`
- `HostAdapterAPI`
- `SetupDoctorAPI`

`CoreRuntimeAPI` owns sessions, state transitions, tools, agents, memory,
artifacts, branches, and reviews.

`HostAdapterAPI` owns the TUI, CLI, server mode, editor/IDE bridges, and other
host-facing shells.

`SetupDoctorAPI` owns setup, migration checks, conformance checks, repair, and
smoke surfaces.

The remote/mobile/web/desktop host is now important enough to be a named
sub-layer under `HostAdapterAPI`, not just an implementation detail.

The implementation route should be:

- `Happy` as the remote-host substrate
- `Happier` as the reference for later host/workbench expansion
- `research-cli` as the runtime authority behind that host

See `docs/deep_study/19-remote-host-control-plane.md` and
`docs/deep_study/20-happy-happier-integration-blueprint.md`.

## 3. Permission And Sandbox Plane

Borrow the Claw Code and Crush pattern: permissions are explicit policy, not
ad hoc prompts.

Required pieces:

- permission mode
- tool allow/deny/ask rules
- workspace write boundary
- per-agent sandbox or worktree binding
- deterministic mock parity harness

The kernel should expose:

- `doctor`
- `smoke`
- `verify`
- `resume`
- `compact`
- `agents`
- `skills`
- `mcp`

Permission defaults should be conservative:

- read-only tools can run immediately
- workspace writes require workspace-write or explicit approval
- out-of-workspace writes require full access
- branch workspaces inherit a narrower default than the main workspace

## 4. Agent Orchestration Plane

Multi-agent is not "spawn more chats". It is a controlled work protocol.

### Agent roles

- `director` - final commit authority and stage owner
- `executor` - makes changes in a bounded workspace
- `reviewer` - independent critic with blinding rules
- `planner` - decomposes goals and budgets
- `researcher` - gathers evidence and candidate ideas
- `synthesizer` - merges findings into project memory and artifacts

### Per-agent filesystem contract

Each agent gets its own directory:

`.pmcli/agents/<agent_id>/`

Files:

- `DIRECTIVE.md`
- `STATUS.md`
- `PRIVATE_MEMORY.md`
- `TRACE.jsonl`
- `OUTPUT_MANIFEST.json`

Rules:

- reviewers never read executor summaries before they inspect primary files
- reviewers are blinded from prior review rounds unless explicitly needed
- only the director can promote shared artifacts
- agents never write to shared memory directly unless granted

### Reviewer independence

Borrow the ARIS rule as a hard system invariant:

- reviewers get paths, objectives, and output schema
- reviewers do not get executor summaries of the target artifact
- re-review is fresh by default
- prior review can only be passed through an explicit `compare_against` field

This should be compiled into a machine-readable `ReviewPacket`, not left only
as prose.

### Agent workspaces

Agents bind to one of:

- `main` workspace
- `git worktree`
- `sandbox`
- `remote runner`

The binding is recorded in `OUTPUT_MANIFEST.json` so every artifact can be
traced back to a filesystem scope.

## 5. Memory And Retrieval Plane

This is where the project-memory goal lives.

### Memory layers

- working memory
- session memory
- durable project memory
- repo artifact memory
- review memory
- synthesis memory

### `MemoryRecord`

Required fields:

- `id`
- `namespace`
- `type`
- `surface`
- `body`
- `provenance`
- `source_session`
- `source_artifacts`
- `confidence`
- `status`
- `valid_from`
- `valid_to`
- `supersedes`
- `superseded_by`
- `usage_count`

### Retrieval rules

- bounded retrieval budget per turn
- separate budgets for recall, evidence lookup, and synthesis
- topically scoped recall first, global recall second
- explicit forgetting/invalidation
- silent background consolidation after turns and at session end
- session-first cache before broad durable recall when scoped evidence exists
- hybrid lexical/semantic/temporal/graph retrieval for hard cases
- pointer-first retrieval followed by verbatim evidence hydration where possible
- every injected memory must have a query/explain path with source evidence

Default budgets should start small and visible:

- working memory: max 20 items per scope
- turn recall: max 8 records or 1800 tokens, whichever comes first
- compaction summary: 1200 to 2000 chars
- silent consolidation: once at session end plus periodic background sweep

### `WorkingMemory`

Working memory is small, scoped, and evicted by importance.

It stores:

- current task
- current hypotheses
- open questions
- recent decisions
- pending follow-ups

### Forgetting and invalidation

Borrow hippo-memory's discipline:

- stale records decay
- migration or deprecation events can invalidate related memories
- contradicted records become `contested`
- superseded records remain searchable as history, but do not inject by default

Borrow Memory-Palace's discipline as well:

- writes go through a write lane
- snapshots are created before destructive memory changes
- background index work shares the same write gate as foreground writes
- silent summaries should enter memory as candidates before promotion to trusted durable records

## 6. Repo Governance Plane

This plane prevents version sprawl.

### `ArtifactFamily`

Each family has:

- `family_id`
- `kind`
- `canonical_id`
- `latest_id`
- `archive_ids`
- `supersession_chain`
- `merge_policy`
- `write_mode`
- `promotion_history`
- `source_branch_ids`

### Repo decisions

Before writing a new file, the system must choose:

- append if the file is the living log
- fork if the work branches
- supersede if a newer version replaces the old one
- reject if it is redundant or unsafe

### Anti-chaos rules

- one canonical latest file per family
- timestamped history for overwritten outputs
- no silent duplicate drafts
- cleanup is a first-class task, not a manual afterthought
- duplicate-file detection and orphan cleanup must be reviewable before destructive changes

### Canonical write policy

For mutable deliverables:

- write timestamped version first
- update fixed-name latest pointer second
- register the new artifact in the family manifest third

That gives history without breaking downstream consumers.

No artifact family should exist without a manifest entry and canonical path
rule.

## 7. Research Workflow Plane

The research pack should be a native skill stack, not a sidecar repo.

### Stage stack

1. `research-lit`
2. `idea-creator`
3. `novelty-check`
4. `research-refine`
5. `design-doc-sync`
6. `experiment-plan`
7. `implement-solution`
8. `run-experiment`
9. `monitor-experiment`
10. `result-to-claim`
11. `paper-plan`
12. `paper-write`
13. `paper-compile`
14. `research-review`
15. `rebuttal`
16. `meta-optimize`

### Workflow contract

- planner generates hypotheses and branches
- executor runs evidence gathering or code changes
- reviewer independently attacks the plan
- publisher or synthesizer writes the final artifact
- experiment results feed back into memory and repo governance
- design docs, code, and experiment plans are synchronized artifact families
- `result-to-claim` can route back into docs, implementation, or experiment plan before rerun

Each skill must declare:

- required inputs
- produced outputs
- output artifact family
- stage preconditions
- completion criteria
- promotion target
- repair targets

The research workflow should be a DAG, not only a flat stage list.

This prevents the research pack from becoming an ungoverned pile of prompts.

## 8. Search And Debate

Branch search and multi-agent debate must be budgeted.

Required controls:

- max branches per wave
- max concurrent agents
- evaluation metric per branch
- stop criteria
- winner threshold
- archive policy for losers
- review pass/fail gate before winner promotion
- score provenance for every branch result
- explicit worktree refresh/reconcile/GC lifecycle

### `SearchBatch`

Required fields:

- `batch_id`
- `objective`
- `wave`
- `max_branches`
- `max_parallel`
- `budget_tokens`
- `budget_tool_calls`
- `evaluation_contract`
- `stop_rule`

### `BranchRun`

Required fields:

- `branch_id`
- `batch_id`
- `assigned_agent_ids`
- `workspace_binding`
- `hypothesis`
- `artifacts`
- `result_summary`
- `score`
- `status`

### `EvaluationContract`

Required fields:

- `primary_metric`
- `secondary_metrics`
- `reviewer_roles`
- `required_evidence`
- `winner_rule`
- `tie_break_rule`

Recommended starting defaults:

- 2 to 4 branches per wave
- 2 concurrent execution branches
- stop after 2 waves unless evidence clearly improves
- archive losers, do not merge them implicitly

A branch may only win if:

- the metric threshold is met
- the review gate passes
- the artifact can be promoted cleanly
- the branch does not violate repo governance

The best branch-search mode should behave like a grid search over method
variants, not like an unbounded swarm.

## 9. Silent Project Progress Summaries

The CLI should periodically summarize progress without interrupting the user.

Output should be written to:

- session compaction summaries
- candidate summary queue
- durable project memory after promotion
- artifact family changelogs

The summary should record:

- what changed
- what is still open
- what was tried and rejected
- what should happen next

The user must also be able to inspect why a summary was promoted and what
source artifacts support it.

This summary is written to structured storage first, and rendered to markdown
second. The markdown is a view, not the canonical state.

## 10. Recommended Build Order

1. build the runtime kernel and state machine
2. add workspace/session persistence and compaction
3. add permissions, sandboxing, and tool policy
4. add agent orchestration and reviewer blinding
5. add memory layers and retrieval budgets
6. add repo artifact governance
7. add research skill packs and branch search
8. add meta-optimization and continuous self-improvement

## Final Judgment

This architecture is designed to be able to outperform Hermes, Claw Code, and
Crush on general code-agent quality, but that claim still depends on the code,
benchmarks, and iteration loop. The important point is that the personalized
features sit on top of a real code-agent kernel, not beside it.
