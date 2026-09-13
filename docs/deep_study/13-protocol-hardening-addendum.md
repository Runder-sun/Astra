# Protocol Hardening Addendum

This addendum addresses the remaining gaps called out by external review.

It does not replace `10`, `11`, or `12`; it tightens the parts that were still
too conceptual.

## 1. What The Reference Projects Concretely Teach Us

### memoryOSS

Key lesson:

- a runtime is only real when it has a versioned contract and conformance kit
- stable semantics must be separated from experimental layers
- object models and known gaps should be machine-readable

Implication:

- `.pmcli` must be versioned and schema-validated
- the project must publish a compatibility surface, not just prose

### Memory-Palace / Memory-Palace-Openclaw

Key lesson:

- memory writes need a guarded pipeline
- snapshots and rollback are first-class, not afterthoughts
- write lanes and background index workers must be serialized and measurable
- session compaction needs explicit flush tracking

Implication:

- our memory plane needs a write lane, snapshot semantics, and index worker
- background consolidation must be visible and recoverable

### mempalace

Key lesson:

- good retrieval is layered: compact index pointers first, verbatim drawers second
- closets/drawers separate fast routing from full evidence

Implication:

- our retrieval router must support a fast pointer layer plus verbatim evidence

### hippo-memory

Key lesson:

- working memory is bounded and ephemeral
- forgetting is a feature
- decay, invalidation, and session-end consolidation prevent noise buildup

Implication:

- our memory contract must include decay/invalidation and scope-level limits

### ARIS

Key lesson:

- reviewer independence must be an explicit protocol
- output versioning prevents overwrite chaos
- review traces are needed for auditability

Implication:

- our review system needs a packet schema, trace schema, and versioned outputs

### Claw Code / Crush

Key lesson:

- excellent code-agent UX comes from explicit workspace/session/permission boundaries
- parity and doctor surfaces matter
- provider/config ergonomics are part of the kernel

Implication:

- our CLI must expose doctor/inspect/resume/compact as first-class surfaces

### GPT Researcher / Autonomous Researcher

Key lesson:

- research should be planner/executor/publisher, with explicit waves and evidence
- structured events and isolated workers are what make multi-agent research tractable

Implication:

- our research engine needs explicit wave, evidence, and publication contracts

## 2. State Machine Hardening

`ProjectState.current_stage` is not enough by itself.

Add:

- `stage_owner` - who currently owns the transition authority
- `stage_mode` - one of `linear`, `parallel`, `recovering`
- `substates` - keyed state for sessions, agents, reviews, branches, memory, repo
- `transition_log` - append-only transition history

### Stage ownership rules

- `runtime` owns `booting`, `ready`, `idle`, `recovering`, `shutting_down`
- `planner` owns `intake`, `planning`
- `executor` owns `executing`
- `reviewer` owns `reviewing`
- `synthesizer` owns `consolidating`, `promoting`, `compacting`
- `branch coordinator` owns `exploring`, `debating`

The top-level stage is therefore a coordinator view, not a complete hidden state.

## 3. `.pmcli` Versioned Protocol

Add protocol fields:

- `schema_version`
- `runtime_version`
- `migration_version`
- `retention_policy`
- `atomic_write_policy`
- `conformance_line`

### Required protocol artifacts

- `project_state.schema.json`
- `event.schema.json`
- `session.schema.json`
- `review.schema.json`
- `memory.schema.json`
- `artifact.schema.json`
- `branch.schema.json`

### Conformance kit

Add a compatibility directory similar to memoryOSS:

```text
schemas/
tests/conformance/
tests/golden/
```

Optional exported `.pmcli/conformance_snapshot/` artifacts may be produced for
debugging or audit, but they are derived exports only and not schema
authority.

Every release should verify that:

- old readable artifacts still load
- additive fields are ignored safely
- breaking changes require a new line/version

## 4. Memory Plane Hardening

Add these runtime services:

- `WriteLaneCoordinator`
- `SessionFlushTracker`
- `IndexWorker`
- `RecallRouter`
- `MemoryRollupScheduler`

### Retrieval modes

- `pointer_first` - closet/index pointer scan first
- `verbatim_followup` - open the referenced drawer/raw artifact next
- `hybrid` - lexical + semantic + temporal + graph routing

### Retrieval explainability

Every injected memory should expose:

- `recall_reason`
- `matched_via`
- `source_artifacts`
- `source_span`
- `hydrated_from_pointer`
- `degraded_reasons`

### Memory write flow

1. gate write through policy
2. snapshot current state
3. append candidate memory
4. enqueue index update
5. compact or roll up if needed
6. persist invalidation or supersession links

### Working memory contract

- max items per scope
- explicit flush on session end
- explicit drop on scope change
- never auto-promote to durable memory without a promotion rule

### Silent summary promotion

Silent summaries must flow through:

1. candidate summary write
2. supporting evidence attachment
3. promotion or rejection

This keeps silent summarization auditable.

## 5. Review Protocol Hardening

Add a machine-readable `ReviewPacket`:

```text
ReviewPacket {
  review_id
  target_paths[]
  objective
  reviewer_role
  review_model
  fresh_thread
  blinded
  blinding_policy
  blind_context[]
  banned_context[]
  redaction {
    schema_version
    policy
    allowed_count
    redacted_count
    redacted_sources[]
  }
  compare_against?
  evidence_required[]
  output_schema
  trace_id
}
```

### Review rules

- a reviewer reads primary files directly
- executor summaries are banned by default and are represented only as redacted
  sources unless explicit compare mode requires a linked prior review
- every review runs in a fresh thread by default
- every review emits a trace artifact

### Review trace requirements

- prompt snapshot
- file list
- model name
- reasoning effort
- thread id
- response text
- verdict summary

## 6. Branch Search Hardening

`SearchBatch` needs adjudication metadata:

- `score_provenance`
- `review_result`
- `acceptance_rule`
- `archive_rule`
- `merge_rule`

`BranchRun` needs:

- `evidence_bundle`
- `review_packet_id`
- `promotion_decision`
- `superseded_by`
- `base_revision`
- `refresh_state`
- `gc_state`

### Winner logic

A branch can win only if:

- it satisfies the primary metric
- it passes review
- it does not violate repo governance
- it can be materialized into a canonical artifact family

### Worktree lifecycle

Every branch workspace must explicitly support:

- allocation
- refresh/reconcile
- merge-conflict state
- archive
- garbage collection

## 7. Research Skill Contract Hardening

Each research skill must declare:

- `inputs`
- `outputs`
- `artifact_family`
- `stage`
- `preconditions`
- `evaluation_hooks`
- `citation_rules`
- `recovery_rules`
- `manifest_version`
- `required_agents`
- `reviewer_policy`

### Workflow graph

The research engine should be a DAG, not a flat checklist.

Core edges:

- literature -> idea
- idea -> novelty
- novelty -> refine
- refine -> design-doc-sync
- design-doc-sync -> experiment plan
- experiment plan -> implement-solution
- implement-solution -> run
- run -> monitor
- monitor -> result-to-claim
- result -> design-doc-sync or implement-solution or experiment plan
- revision -> write
- write -> compile
- compile -> review

## 8. Repo Governance Hardening

The artifact system needs:

- canonical path rule
- latest pointer rule
- archive path rule
- supersession chain rule
- manifest versioning
- conflict resolution policy

No artifact family should exist without a manifest entry.

## 9. What This Changes In The Architecture

After this hardening pass, the architecture should be read as:

- a versioned runtime protocol
- a bounded memory system with explicit write/retrieval lanes
- a review system with traceable independence
- a branch search system with adjudication
- a research workflow DAG with output contracts
- a kernel product surface with explicit parity and operator contracts

That is the missing difference between a strong concept doc and a system that
can actually be implemented safely.
