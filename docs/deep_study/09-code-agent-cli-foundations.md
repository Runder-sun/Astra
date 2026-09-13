# Code Agent CLI Foundations

This note isolates the part of the design that must be excellent before any
project-memory or research specialization is layered on top.

## Thesis

A strong code-agent CLI is not just a chat loop with tools. It needs:

- a small, explicit runtime kernel
- a stable session and workspace model
- predictable permissions and sandboxing
- resumable sessions with compaction
- tool, plugin, skill, and MCP boundaries
- visible state and recovery commands
- testable behavior under deterministic harnesses

If these are weak, project memory and multi-agent research only amplify the
chaos.

## What The Best References Actually Contribute

### Hermes

Strengths:

- broad product surface: CLI, gateway, cron, messaging, skills, memory
- built-in session search over older transcripts
- self-improving memory/skills model
- subagent delegation with isolated children
- explicit session context for multi-platform use

Weaknesses:

- runtime is too broad and monolithic for a kernel
- too many concerns live inside the same Python entrypoint
- memory is powerful, but the control boundaries are softer than they should be

Takeaway:

- borrow the recall model, delegation model, and multi-surface delivery
- do not copy the monolithic runtime shape

### Claw Code

Strengths:

- Rust runtime discipline
- per-workspace session namespacing
- explicit permission policy and enforcement
- deterministic mock/parity harnesses
- summary compression with bounded output
- plugin lifecycle and policy engine separation

Weaknesses:

- less native project-memory depth than the memory-focused repos
- more CLI kernel than long-horizon memory system

Takeaway:

- use it as the reference for the CLI kernel, permissions, and recovery
- its session store and compression logic should be first-class patterns

### OpenCode -> Crush

Strengths:

- clear app/service composition
- workspace abstraction shared by TUI and CLI
- session persistence and resume
- permission service with notification flow
- auto-compaction for long sessions
- LSP/MCP integration as native runtime services

Weaknesses:

- OpenCode itself is archived
- the live design line is Crush, not OpenCode

Takeaway:

- treat OpenCode as historical evidence
- treat Crush as the live continuation and preferred terminal UX reference

### Memory-Palace Family

Strengths:

- structured memory topology and retrieval layers
- explicit working-memory vs long-term memory separation
- clear write-lane coordination and flush tracking
- strong review/rollback/state contracts in the Openclaw adaptation

Takeaway:

- reuse the memory layering ideas, not a flat memory bucket
- working memory must stay bounded and ephemeral

### memoryOSS

Strengths:

- runtime contract is versioned and machine-readable
- object model and state machines are explicit
- proxy/gateway behavior is defined as a system, not a guess
- conformance and regression surfaces are part of the product

Takeaway:

- the final CLI needs an explicit contract surface, not just docs

### hippo-memory

Strengths:

- bounded working memory
- active invalidation / forgetting
- daily consolidation and session-end sleep
- cross-tool hooks and multi-tool portability

Takeaway:

- memory is not only recall; it also needs pruning and decay

### ARIS

Strengths:

- reviewer independence is treated as a hard rule
- output versioning avoids overwrite chaos
- research workflows are broken into explicit stages
- skills are the unit of reuse

Takeaway:

- research behavior should be modular, staged, and auditable

### GPT Researcher

Strengths:

- planner / executor / publisher split
- deep research as a tree, not a single pass
- multi-source retrieval and report synthesis

Takeaway:

- research needs a branching planning layer, not just one executor

### Autonomous Researcher

Strengths:

- orchestrator + specialist agents
- persistent per-agent sandbox concept
- experiment-first workflow

Takeaway:

- multi-agent work must be sandboxed and budgeted, not just parallelized

## What A Real Code-Agent Kernel Must Do

The kernel must own these responsibilities:

1. read the project root and establish workspace identity
2. resolve session state, resume state, and compaction state
3. mediate tool calls through permissions and sandbox policy
4. provide a consistent event stream
5. spawn and observe isolated agents
6. compact and recover sessions without losing intent
7. manage plugin/skill/MCP discovery
8. write structured artifacts instead of opaque logs only
9. expose doctor/debug/smoke commands
10. keep the TUI and CLI on top of the same engine

## Design Rule

From this point on, the system should be designed as:

- kernel first
- project memory second
- multi-agent research third
- repository governance and skill packs as native extensions

Not the other way around.

## What We Must Surpass

To be worth building, the new CLI should beat the reference set on clear axes.

### Must surpass Hermes on

- kernel modularity
- explicit state machine
- project-local persistence protocol
- repo governance and anti-chaos rules

### Must surpass Claw Code on

- native project-memory depth
- long-horizon multi-agent orchestration
- research workflow integration

### Must surpass Crush on

- project-memory and repo-memory richness
- branch-search and debate governance
- silent project progress synthesis

### Must preserve from all three

- terminal-native usability
- resumable sessions
- permission clarity
- strong tool surfaces
- operational commands like doctor, resume, compact, and inspect
