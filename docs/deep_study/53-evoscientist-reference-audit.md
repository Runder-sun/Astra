---
doc_frame:
  doc_id: docs.deep_study.53_evoscientist_reference_audit
  schema_version: "1"
  source_path: docs/deep_study/53-evoscientist-reference-audit.md
  title: EvoScientist Reference Audit
  doc_type: reference_audit
  lifecycle: active
  scope: cross_milestone
  milestone: M5-M11+
  summary: Source-level audit of EvoScientist as a research-agent product, middleware, multi-channel, skill, MCP, and human-on-loop reference.
  key_claims:
    - EvoScientist is stronger than most references on productized research-agent UX: six explicit research roles, lazy CLI initialization, prompt-level scientific workflow, adaptive tool selection, ask_user interrupts, multi-channel adapters, MCP routing, and installable skills.
    - EvoScientist's core workflow remains mostly prompt/middleware driven; research-cli should absorb the UX and middleware lessons as schema-owned packets, gates, traces, and projections rather than hidden prompt behavior.
    - Research-cli keeps the higher authority ceiling when role dispatch, human gates, tool selection, memory, channels, and skill outputs remain governed by TaskPacket, ResearchTurnClassification, SkillOutputEnvelope, canonicality, and event-backed project truth.
  decisions:
    - Adopt EvoScientist-style role profiles as typed role templates, not as free-form subagent prompts.
    - Adopt planner reflection JSON as a future typed StageReflectionPacket feeding M10 repair, retry, pivot, and todo updates.
    - Adopt adaptive tool selection only if every selection emits a ToolSelectionTrace and respects per-role capability routing.
    - Adopt ask_user/HITL channel behavior as M10 pending human gates with typed resume decisions, not as opaque chat pauses.
    - Treat multi-channel and mobile endpoints as adapters over one runtime truth, with pairing, allowlist, mention gating, per-chat serialization, backpressure, and timeout metrics.
    - Keep skill/MCP ergonomics, but route tools by role and publish outputs only through governed envelopes.
  interfaces:
    - docs/deep_study/33-advanced-systems-implementation-plan.md
    - docs/deep_study/51-m6-m10-reference-superiority-design-audit.md
    - docs/deep_study/52-ultron-verifier-tailscale-reference-audit.md
    - docs/deep_study/23-cli-operator-contract.md
    - docs/deep_study/25-remote-transport-auth-contract.md
    - docs/deep_study/26-memory-branch-research-runtime-policy.md
  evidence_refs:
    - reference_repos/research_agents/EvoScientist/README.md
    - reference_repos/research_agents/EvoScientist/EvoScientist/EvoScientist.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/subagent.yaml
    - reference_repos/research_agents/EvoScientist/EvoScientist/prompts.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/tool_selector.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/context_editing.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/context_overflow.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/tool_error_handler.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/memory.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/middleware/ask_user.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/README.md
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/base.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/middleware.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/channel_manager.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/consumer.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/channels/bus/events.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/cli/channel.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/stream/events.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/stream/state.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/stream/tracker.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/sessions.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/backends.py
    - reference_repos/research_agents/EvoScientist/EvoScientist/mcp/README.md
    - reference_repos/research_agents/EvoScientist/EvoScientist/tools/skill_manager.py
    - reference_repos/research_agents/EvoScientist/tests/test_hitl.py
    - reference_repos/research_agents/EvoScientist/tests/test_tool_selector_middleware.py
  next_actions:
    - Add ToolSelectionTrace and role-scoped MCP exposure to the post-M11 tool-governance backlog.
    - Add StageReflectionPacket to the future M10 refinement backlog before claiming planner self-repair beyond current stage-operation records.
    - Add channel-adapter hardening gates to M5 future work: pairing expiry, sender allowlist, mention gating, per-chat locks, bounded queues, late-response handling, and remote ask_user resume.
    - Keep EvoScientist memory extraction as a UX lesson only; do not replace research-cli's schema/provenance/invalidation memory model with markdown injection.
  non_goals:
    - This audit does not mark remote mobile full-CLI parity complete.
    - This audit does not authorize global tool exposure to every role.
    - This audit does not weaken canonical project truth, publication gates, or Git-native promotion rules.
  generated_by: codex
  updated_at: 2026-04-27
---

# EvoScientist Reference Audit

This audit records lessons from the newly pulled repository:

- `reference_repos/research_agents/EvoScientist`

EvoScientist is not only an algorithm reference. It is a polished
research-agent product reference: CLI/TUI entrypoints, six research subagents,
multi-channel remote adapters, MCP routing, installable skills, persistent
memory, streaming UI state, tool-selection middleware, and human-on-loop
interrupts.

The adoption rule is simple: copy the product and middleware lessons, but move
their authority into research-cli's typed runtime. EvoScientist often relies on
prompts, middleware side effects, and LangGraph interrupts. Research-cli should
turn those same ideas into schemas, durable events, canonical artifacts, and
operator-visible traces.

## 1. Product And Runtime Shape

`EvoScientist.py` shows two useful runtime practices.

First, CLI commands are lazily initialized. Non-agent commands do not pay the
cost of model, MCP, memory, and subagent setup. This matters for research-cli
because setup, doctor, status, remote, memory inspection, artifact inspection,
and canonicality audit should stay fast even when the full agent runtime is
large.

Second, EvoScientist builds one research agent by composing:

- workspace backend
- skills backend
- memory backend
- subagents from YAML
- MCP tools, optionally routed to specific agents
- context editing
- context overflow mapping
- tool error handling
- adaptive tool selection
- memory middleware
- optional ask_user middleware

Adoption decision:

- keep research-cli's kernel-first architecture
- preserve fast non-agent command paths
- treat every heavy runtime service as optional until a command actually needs
  it
- expose initialization cost and degraded-service state through structured
  status, not hidden startup delay

Why research-cli can exceed it:

- EvoScientist composes a strong agent shell, but the shell owns much of the
  behavior.
- Research-cli can keep the shell ergonomic while making project truth owned by
  `.pmcli`, schemas, Git/canonicality gates, and event records.

## 2. Six Research Roles

`subagent.yaml` defines explicit role profiles:

- planner-agent
- research-agent
- code-agent
- debug-agent
- data-analysis-agent
- writing-agent

The useful pattern is not just the names. Each role has a constrained mission,
expected outputs, and stage-specific success criteria. The planner role is
especially relevant because its reflection mode returns structured JSON with
completion status, unmet signals, skill suggestions, stage modifications, new
stages, and todo updates.

Adoption decision:

- keep M6 `TaskPacket.role_profile` and `TaskPacket.success_criteria` as the
  authority source
- add future typed role templates that mirror these six categories without
  reducing them to prompts
- add a future `StageReflectionPacket` that can carry planner reflection into
  M10 repair, retry, pivot, fork, supersede, and todo-update operations

Why research-cli can exceed it:

- EvoScientist role prompts are practical and well-written.
- Research-cli can make role work replayable, packet-bound, scope-bound,
  reviewable, and canonicality-safe.

## 3. Scientific Workflow Prompting

`prompts.py` encodes a complete scientific workflow:

- intake and scope
- plan
- execute and debug
- evaluate and iterate
- write report
- verify

It also emphasizes baseline-first execution, changing one major variable per
iteration, explicit success signals, stage reflection, and memory logging after
important outcomes.

Adoption decision:

- keep these as useful role prompt defaults
- do not let prompt text define the real research runtime
- M10 stage legality must remain owned by `StageExecutionMap`,
  `ResearchStageExecution`, `ResearchTurnClassification`, and explicit
  `research decide` records

Why research-cli can exceed it:

- EvoScientist is strong at guiding an agent through a scientific process.
- Research-cli can make the process nonlinear, inspectable, replayable, and
  recoverable after long conversations or stage backtracking.

## 4. Adaptive Tool Selection

`middleware/tool_selector.py` adds a practical optimization:

- run LLM tool selection only when tool count exceeds a threshold
- always include the thinking tool and delegation tool
- track which tools were selected so the UI can surface them
- fail open to all tools if the selector itself fails

This is directly relevant to M11 and MCP/skill routing. A large research CLI
will have many tools; exposing every tool to every call wastes tokens and
increases mistakes.

Adoption decision:

- implement future adaptive tool exposure as `ToolSelectionTrace`
- record candidate tools, selected tools, role profile, threshold, reason, and
  fallback mode
- tool selection must respect role-scoped MCP/skill permissions
- selection can reduce exposure, but cannot grant capabilities outside the
  task packet, skill contract, or MCP routing policy

Why research-cli can exceed it:

- EvoScientist tracks selected tools for display.
- Research-cli should make tool selection auditable governance evidence.

## 5. Context, Tool Error, And Memory Middleware

EvoScientist's context middleware gives three lessons:

- map provider-specific context-limit errors into one overflow path
- compact/edit context before the model hits a hard failure
- preserve recent tool-call continuity while removing older tool noise

Its tool error middleware catches tool execution exceptions and returns them as
tool messages rather than crashing the entire agent loop.

Its memory middleware injects a markdown memory file and uses structured model
extraction after thresholds to update user profile, preferences, experiment
conclusions, and learned preferences.

Adoption decision:

- keep early context editing and provider-error normalization as runtime
  hardening patterns
- keep tool errors as structured recoverable events with trace refs
- keep memory extraction triggers, but publish memory only through
  research-cli's schema/provenance/invalidation system

Why research-cli can exceed it:

- EvoScientist's memory path is pragmatic and useful.
- Research-cli already targets stronger memory correctness through staged
  candidates, durable promotion, hybrid retrieval, vector/temporal backends,
  explainability, supersession, rollback invalidation, and public projection
  discipline.

## 6. ask_user And HITL Interrupts

`middleware/ask_user.py`, `channels/consumer.py`, and `tests/test_hitl.py`
show a mature human-on-loop pattern:

- typed questions: text and multiple choice
- model interrupt, UI collection, and graph resume
- channel delivery of approval prompts
- allow-list based auto-approval for safe commands
- timeout handling
- per-session pending approval and pending ask_user slots

The strongest lesson is when to ask: dataset choice, benchmark choice,
resource constraints, heavy compute confirmation, methodology ambiguity, OOM or
timeout recovery, and significant workflow changes.

Adoption decision:

- map ask_user to M10 `human_gate_required` plus pending operations
- resume through `research decide` or equivalent typed channel decisions
- distinguish clarification questions from approval gates
- never let a channel reply mutate project truth without passing the same
  typed decision and permission gate as local CLI input

Why research-cli can exceed it:

- EvoScientist handles interactive pauses well.
- Research-cli can make those pauses durable research decisions tied to stage
  operations, evidence refs, and canonical project changes.

## 7. Multi-Channel Remote Architecture

The channel system is one of EvoScientist's strongest references.

Useful source-level patterns:

- `MessageBus` separates inbound and outbound queues
- channel adapters normalize raw platform messages
- middleware handles dedup, allowlist, pairing, group history, mention gating,
  formatting, chunking, retry, typing, and reactions
- `InboundConsumer` uses bounded queues, worker pools, per-chat locks, session
  dedup, pending ask_user/HITL replies, idle timeouts, and metrics
- `cli/channel.py` bridges the bus thread and main CLI thread so channel
  messages do not directly own agent runtime state
- slash commands from channels route through the same command manager as local
  CLI/TUI surfaces

Adoption decision for M5 and mobile:

- external channels and phone apps are adapters, never runtime authorities
- every inbound message needs channel, sender, chat/session identity, message
  id, allowlist/pairing state, and dedup behavior
- same chat/session should serialize work unless a typed concurrency policy
  says otherwise
- remote ask_user and HITL replies should resume existing pending gates, not
  start new agent turns
- mobile full-CLI parity should combine the existing Tailscale/PTY plan with a
  channel-adapter plan for notifications and structured approvals

Why research-cli can exceed it:

- EvoScientist has better ready-made multi-channel UX than research-cli today.
- Research-cli can exceed it by keeping every channel action projected from the
  same kernel events, terminal leases, permissions, canonicality gates, and
  publication transactions.

## 8. Sessions, Backends, Skills, And MCP

EvoScientist's sessions use SQLite checkpoints with thread list/resume and
metadata previews. This is a useful UX pattern, but research-cli's `.pmcli`
event and schema ownership should remain the durable project authority.

`backends.py` has practical sandbox lessons:

- deny dangerous system paths and commands
- map virtual paths into workspace paths
- keep built-in/global skills read-only
- merge skill backends with priority: workspace, global, built-in

`mcp/README.md` is important for M11:

- MCP supports stdio, HTTP, streamable HTTP, SSE, and websocket
- tools can be allowlisted with wildcards
- `expose_to` routes servers/tools to main or specific subagents

`tools/skill_manager.py` shows good runtime ergonomics:

- install from GitHub shorthand, GitHub URL, or local path
- browse remote skills
- list system and user skills
- inspect and uninstall

Adoption decision:

- role-scoped MCP exposure should become native tool governance
- tool allowlists should support exact and wildcard matching
- skill discovery/install ergonomics should improve, but public outputs still
  go through `SkillOutputEnvelope`
- local skill execution should preserve native skill directories while auditing
  side effects and allowed write scopes

Why research-cli can exceed it:

- EvoScientist is easier to extend at runtime.
- Research-cli should match that ergonomics while retaining stronger output
  publication, canonical surface, and rollback guarantees.

## 9. Comparative Verdict

EvoScientist is ahead in current research-agent product UX:

- research role prompts
- ask_user/HITL flow
- multi-channel adapters
- easy MCP and skill routing
- adaptive tool selector
- streaming subagent display

Research-cli is architecturally ahead where the implemented design is already
proof-backed:

- schema-owned project truth
- Git-native canonicality
- packet-bound M6 agents
- governed memory promotion and invalidation
- M9 branch promotion and winner merge gates
- M10 nonlinear research DAG and long-deliberation state
- M11 skill output publication instead of direct public writes

The right target is not to copy EvoScientist's hidden prompt/middleware
behavior. The target is to absorb its mature UX as typed runtime objects:

- role templates become packet-bound role profiles
- planner reflection becomes `StageReflectionPacket`
- selected tools become `ToolSelectionTrace`
- ask_user becomes pending human gates and typed resume decisions
- channel messages become remote/channel action envelopes
- skill/MCP routing becomes role-scoped capability governance
- memory extraction becomes candidate memory ingress with provenance and
  invalidation

If these adoptions are implemented this way, EvoScientist raises our product
bar without lowering our governance bar.
